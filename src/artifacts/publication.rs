use super::diagnostic_set;
use super::fault::{self, Point};
use super::staging::{PublishedArtifactDir, StagedArtifactDir};
use atomicwrites::{AllowOverwrite, AtomicFile};
use coflow_api::DiagnosticSet;
use coflow_project::Project;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DATA_OUTPUT_SLOT: &str = "data";
pub const CODE_OUTPUT_SLOT: &str = "code";

const MANIFEST_VERSION: u32 = 1;
const STATE_DIR: &str = ".coflow/artifacts";
const ACTIVE_MANIFEST: &str = "active.json";
const ENUM_LOCKFILE_NAME: &str = "coflow.enum.lock.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ArtifactManifest {
    version: u32,
    revision: String,
    outputs: BTreeMap<String, PublishedArtifactDir>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enum_lock: Option<Value>,
}

#[derive(Debug, Clone)]
pub enum EnumLockUpdate {
    Preserve,
    Replace(Value),
}

#[derive(Debug, Clone)]
pub struct PublishedArtifactSnapshot {
    manifest: ArtifactManifest,
}

impl PublishedArtifactSnapshot {
    pub fn output_dir(&self, slot: &str) -> Result<&Path, DiagnosticSet> {
        self.manifest
            .outputs
            .get(slot)
            .map(|output| output.requested_dir.as_path())
            .ok_or_else(|| {
                diagnostic_set(
                    PathBuf::from(slot),
                    format!("active artifact manifest has no `{slot}` output"),
                )
            })
    }
}

pub fn publish_artifacts(
    project: &Project,
    staged_outputs: Vec<(&str, StagedArtifactDir)>,
    removed_outputs: &[&str],
    enum_lock_update: EnumLockUpdate,
) -> Result<PublishedArtifactSnapshot, DiagnosticSet> {
    validate_publication_slots(project, &staged_outputs, removed_outputs)?;
    let mut manifest = load_active_manifest(project)?.unwrap_or_else(empty_manifest);
    if manifest.enum_lock.is_none() {
        manifest.enum_lock = read_versioned_enum_lock(project)?;
    }
    let mut pending_generations = PendingGenerations::default();
    let mut requested_outputs = Vec::with_capacity(staged_outputs.len());
    for (slot, staged) in staged_outputs {
        let (generation, requested_output) = staged.seal()?;
        pending_generations.track(&generation);
        manifest.outputs.insert(slot.to_string(), generation);
        requested_outputs.push(requested_output);
    }
    for slot in removed_outputs {
        manifest.outputs.remove(*slot);
    }
    let persist_enum_lock = matches!(&enum_lock_update, EnumLockUpdate::Replace(_));
    if let EnumLockUpdate::Replace(value) = enum_lock_update {
        manifest.enum_lock = Some(value);
    }
    manifest.version = MANIFEST_VERSION;
    manifest.revision = unique_revision();

    for output in &mut requested_outputs {
        output.publish()?;
    }

    if persist_enum_lock {
        write_versioned_enum_lock(
            project,
            manifest.enum_lock.as_ref().ok_or_else(|| {
                diagnostic_set(
                    manifest_path(project),
                    "active artifact manifest is missing replacement enum lock state",
                )
            })?,
        )?;
    }
    write_active_manifest(project, &manifest)?;
    for output in &mut requested_outputs {
        output.activate();
    }
    pending_generations.activate();
    Ok(PublishedArtifactSnapshot { manifest })
}

#[derive(Debug, Default)]
struct PendingGenerations {
    directories: Vec<PathBuf>,
    activated: bool,
}

impl PendingGenerations {
    fn track(&mut self, generation: &PublishedArtifactDir) {
        self.directories.push(generation.generation_dir.clone());
    }

    const fn activate(&mut self) {
        self.activated = true;
    }
}

impl Drop for PendingGenerations {
    fn drop(&mut self) {
        if !self.activated {
            for directory in &self.directories {
                let _ = fs::remove_dir_all(directory);
            }
        }
    }
}

fn validate_publication_slots(
    project: &Project,
    staged_outputs: &[(&str, StagedArtifactDir)],
    removed_outputs: &[&str],
) -> Result<(), DiagnosticSet> {
    let mut staged_slots = BTreeSet::new();
    for (slot, _) in staged_outputs {
        if slot.is_empty() || !staged_slots.insert(*slot) {
            return Err(diagnostic_set(
                manifest_path(project),
                format!("artifact publication contains invalid or duplicate `{slot}` output"),
            ));
        }
    }
    let mut removed_slots = BTreeSet::new();
    for slot in removed_outputs {
        if slot.is_empty() || !removed_slots.insert(*slot) || staged_slots.contains(*slot) {
            return Err(diagnostic_set(
                manifest_path(project),
                format!("artifact publication contains conflicting `{slot}` removal"),
            ));
        }
    }
    Ok(())
}

pub fn read_active_enum_lock(project: &Project) -> Result<Option<Value>, DiagnosticSet> {
    if let Some(value) = load_active_manifest(project)?.and_then(|manifest| manifest.enum_lock) {
        return Ok(Some(value));
    }
    read_versioned_enum_lock(project)
}

fn read_versioned_enum_lock(project: &Project) -> Result<Option<Value>, DiagnosticSet> {
    let path = enum_lockfile_path(project);
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read(&path).map_err(|err| {
        diagnostic_set(
            &path,
            format!(
                "failed to read @idAsEnum lockfile `{}`: {err}",
                path.display()
            ),
        )
    })?;
    serde_json::from_slice(&contents).map(Some).map_err(|err| {
        diagnostic_set(
            &path,
            format!(
                "failed to parse @idAsEnum lockfile `{}`: {err}",
                path.display()
            ),
        )
    })
}

const fn empty_manifest() -> ArtifactManifest {
    ArtifactManifest {
        version: MANIFEST_VERSION,
        revision: String::new(),
        outputs: BTreeMap::new(),
        enum_lock: None,
    }
}

fn load_active_manifest(project: &Project) -> Result<Option<ArtifactManifest>, DiagnosticSet> {
    let path = manifest_path(project);
    if !path.exists() {
        return Ok(None);
    }
    let contents = fault::check(Point::ReadActiveManifest)
        .and_then(|()| fs::read(&path))
        .map_err(|err| {
            diagnostic_set(
                &path,
                format!(
                    "failed to read active artifact manifest `{}`: {err}",
                    path.display()
                ),
            )
        })?;
    let manifest: ArtifactManifest = serde_json::from_slice(&contents).map_err(|err| {
        diagnostic_set(
            &path,
            format!(
                "failed to parse active artifact manifest `{}`: {err}",
                path.display()
            ),
        )
    })?;
    validate_manifest(&path, &manifest)?;
    Ok(Some(manifest))
}

fn validate_manifest(path: &Path, manifest: &ArtifactManifest) -> Result<(), DiagnosticSet> {
    if manifest.version != MANIFEST_VERSION {
        return Err(diagnostic_set(
            path,
            format!(
                "unsupported active artifact manifest version `{}`",
                manifest.version
            ),
        ));
    }
    for (slot, output) in &manifest.outputs {
        fault::check(Point::ValidateActiveGeneration).map_err(|err| {
            diagnostic_set(
                path,
                format!(
                    "failed to validate active `{slot}` artifact generation `{}`: {err}",
                    output.generation_dir.display()
                ),
            )
        })?;
        if !output.generation_dir.is_dir() {
            return Err(diagnostic_set(
                path,
                format!(
                    "active `{slot}` artifact generation `{}` is missing or not a directory",
                    output.generation_dir.display()
                ),
            ));
        }
    }
    Ok(())
}

fn write_active_manifest(
    project: &Project,
    manifest: &ArtifactManifest,
) -> Result<(), DiagnosticSet> {
    let path = manifest_path(project);
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fault::check(Point::CreateArtifactStateDirectory).map_err(|err| {
        diagnostic_set(
            &path,
            format!(
                "failed to create artifact state directory `{}`: {err}",
                parent.display()
            ),
        )
    })?;
    fs::create_dir_all(parent).map_err(|err| {
        diagnostic_set(
            &path,
            format!(
                "failed to create artifact state directory `{}`: {err}",
                parent.display()
            ),
        )
    })?;
    let contents = serde_json::to_vec_pretty(manifest).map_err(|err| {
        diagnostic_set(
            &path,
            format!("failed to serialize active artifact manifest: {err}"),
        )
    })?;
    AtomicFile::new(&path, AllowOverwrite)
        .write(|file| {
            fault::check(Point::WriteActiveManifest)?;
            file.write_all(&contents)
        })
        .map_err(|err| {
            diagnostic_set(
                &path,
                format!(
                    "failed to activate artifact manifest `{}`: {err}",
                    path.display()
                ),
            )
        })
}

fn write_versioned_enum_lock(project: &Project, lock: &Value) -> Result<(), DiagnosticSet> {
    let path = enum_lockfile_path(project);
    let contents = serde_json::to_vec_pretty(lock).map_err(|err| {
        diagnostic_set(
            &path,
            format!("failed to serialize @idAsEnum lockfile: {err}"),
        )
    })?;
    AtomicFile::new(&path, AllowOverwrite)
        .write(|file| {
            fault::check(Point::WriteEnumLockMirror)?;
            file.write_all(&contents)
        })
        .map_err(|err| {
            diagnostic_set(
                &path,
                format!(
                    "failed to update @idAsEnum lockfile `{}`: {err}",
                    path.display()
                ),
            )
        })
}

fn manifest_path(project: &Project) -> PathBuf {
    artifact_state_dir(project).join(ACTIVE_MANIFEST)
}

pub(crate) fn artifact_state_dir(project: &Project) -> PathBuf {
    project
        .config_path
        .parent()
        .unwrap_or(&project.root_dir)
        .join(STATE_DIR)
}

pub fn enum_lockfile_path(project: &Project) -> PathBuf {
    project
        .config_path
        .parent()
        .unwrap_or(&project.root_dir)
        .join(ENUM_LOCKFILE_NAME)
}

pub(crate) fn clean_history(project: &Project) -> Result<(usize, usize), DiagnosticSet> {
    let active = load_active_manifest(project)?
        .into_iter()
        .flat_map(|manifest| manifest.outputs.into_values())
        .map(|output| output.generation_dir)
        .collect::<BTreeSet<_>>();
    let state_dir = artifact_state_dir(project);
    let generations_removed = clean_children(&state_dir.join("generations"), &active)?;
    let staging_removed = clean_children(&state_dir.join("staging"), &BTreeSet::new())?;
    Ok((generations_removed, staging_removed))
}

fn clean_children(parent: &Path, preserved: &BTreeSet<PathBuf>) -> Result<usize, DiagnosticSet> {
    if !parent.exists() {
        return Ok(0);
    }
    let entries = fs::read_dir(parent).map_err(|err| {
        diagnostic_set(
            parent,
            format!(
                "failed to read artifact history `{}`: {err}",
                parent.display()
            ),
        )
    })?;
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(|err| {
            diagnostic_set(
                parent,
                format!(
                    "failed to read artifact history `{}`: {err}",
                    parent.display()
                ),
            )
        })?;
        let path = entry.path();
        if preserved.contains(&path) {
            continue;
        }
        let file_type = entry.file_type().map_err(|err| {
            diagnostic_set(
                &path,
                format!(
                    "failed to inspect artifact history `{}`: {err}",
                    path.display()
                ),
            )
        })?;
        let result = if file_type.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        result.map_err(|err| {
            diagnostic_set(
                &path,
                format!(
                    "failed to remove artifact history `{}`: {err}",
                    path.display()
                ),
            )
        })?;
        removed += 1;
    }
    Ok(removed)
}

fn unique_revision() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}-{timestamp}", std::process::id())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{
        publish_artifacts, read_active_enum_lock, EnumLockUpdate, DATA_OUTPUT_SLOT,
        ENUM_LOCKFILE_NAME,
    };
    use crate::artifacts::fault::{self, ALL_POINTS};
    use crate::artifacts::staging::stage_artifact_set;
    use coflow_api::{ArtifactFile, ArtifactSet};
    use coflow_project::Project;
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn every_reported_filesystem_failure_preserves_the_active_snapshot() {
        for point in ALL_POINTS {
            assert_failure_preserves_active_snapshot(point);
        }
    }

    fn assert_failure_preserves_active_snapshot(point: fault::Point) {
        let root = test_project_root(point);
        let project = open_test_project(&root);
        let requested = root.join("generated/data");
        let old_lock = json!({"ItemId": {"old": 0}});
        let new_lock = json!({"ItemId": {"new": 1}});
        let state_dir = super::artifact_state_dir(&project);
        let baseline = stage_artifact_set(
            &state_dir,
            DATA_OUTPUT_SLOT,
            &requested,
            artifact_set("old"),
        )
        .expect("stage baseline artifacts");
        let baseline = publish_artifacts(
            &project,
            vec![(DATA_OUTPUT_SLOT, baseline)],
            &[],
            EnumLockUpdate::Replace(old_lock.clone()),
        )
        .expect("publish baseline artifacts");
        assert_eq!(
            baseline
                .output_dir(DATA_OUTPUT_SLOT)
                .expect("baseline requested output"),
            requested
        );
        let old_generation = baseline
            .manifest
            .outputs
            .get(DATA_OUTPUT_SLOT)
            .expect("baseline output")
            .generation_dir
            .clone();
        let old_requested = std::fs::read(requested.join("nested/value.txt"))
            .expect("read baseline requested output");
        let manifest_path = root.join(".coflow/artifacts/active.json");
        let old_manifest = std::fs::read(&manifest_path).expect("read baseline manifest");

        let result = {
            let _injection = fault::inject(point);
            stage_artifact_set(
                &state_dir,
                DATA_OUTPUT_SLOT,
                &requested,
                artifact_set("new"),
            )
            .and_then(|staged| {
                publish_artifacts(
                    &project,
                    vec![(DATA_OUTPUT_SLOT, staged)],
                    &[],
                    EnumLockUpdate::Replace(new_lock.clone()),
                )
                .map(|_| ())
            })
        };

        assert!(result.is_err(), "{point:?} did not inject a failure");
        assert_eq!(
            std::fs::read(&manifest_path).expect("read active manifest after failure"),
            old_manifest,
            "{point:?} changed active manifest bytes"
        );
        assert_eq!(
            std::fs::read_to_string(old_generation.join("nested/value.txt"))
                .expect("read old active artifact"),
            "old",
            "{point:?} changed the active generation"
        );
        assert_eq!(
            std::fs::read(requested.join("nested/value.txt"))
                .expect("read requested output after failure"),
            old_requested,
            "{point:?} changed the requested output"
        );
        assert_eq!(
            read_active_enum_lock(&project).expect("read active enum lock"),
            Some(old_lock),
            "{point:?} exposed the non-authoritative enum lock mirror"
        );
        if point == fault::Point::WriteActiveManifest {
            let mirror: serde_json::Value = serde_json::from_slice(
                &std::fs::read(root.join(ENUM_LOCKFILE_NAME)).expect("read enum lock mirror"),
            )
            .expect("parse enum lock mirror");
            assert_eq!(
                mirror, new_lock,
                "the test must prove that an ahead mirror remains non-authoritative"
            );
        }
        assert_eq!(
            generation_count(&state_dir.join("generations")),
            1,
            "{point:?} leaked an unactivated generation"
        );
        std::fs::remove_dir_all(root).expect("remove test project");
    }

    fn test_project_root(point: fault::Point) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "coflow-artifact-fault-{point:?}-{}",
            super::unique_revision()
        ))
    }

    fn open_test_project(root: &Path) -> Project {
        std::fs::create_dir_all(root).expect("create test project");
        std::fs::write(root.join("coflow.yaml"), "schema: schema/\n").expect("write test config");
        Project::open_schema_only(Some(root)).expect("open test project")
    }

    fn artifact_set(contents: &str) -> ArtifactSet {
        ArtifactSet::new(vec![ArtifactFile::text("nested/value.txt", contents)])
            .expect("valid artifact set")
    }

    fn generation_count(parent: &Path) -> usize {
        std::fs::read_dir(parent)
            .expect("read output parent")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
            .count()
    }
}
