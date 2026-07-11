use super::staging::{PublishedArtifactDir, StagedArtifactDir};
use super::diagnostic_set;
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
            .map(|output| output.generation_dir.as_path())
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
    for (slot, staged) in staged_outputs {
        let generation = staged.seal()?;
        manifest.outputs.insert(slot.to_string(), generation);
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

    write_active_manifest(project, &manifest)?;
    if persist_enum_lock {
        write_versioned_enum_lock(project, manifest.enum_lock.as_ref().ok_or_else(|| {
            diagnostic_set(
                manifest_path(project),
                "active artifact manifest is missing replacement enum lock state",
            )
        })?)?;
    }
    Ok(PublishedArtifactSnapshot { manifest })
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
            format!("failed to read @idAsEnum lockfile `{}`: {err}", path.display()),
        )
    })?;
    serde_json::from_slice(&contents).map(Some).map_err(|err| {
        diagnostic_set(
            &path,
            format!("failed to parse @idAsEnum lockfile `{}`: {err}", path.display()),
        )
    })
}

fn empty_manifest() -> ArtifactManifest {
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
    let contents = fs::read(&path).map_err(|err| {
        diagnostic_set(
            &path,
            format!("failed to read active artifact manifest `{}`: {err}", path.display()),
        )
    })?;
    let manifest: ArtifactManifest = serde_json::from_slice(&contents).map_err(|err| {
        diagnostic_set(
            &path,
            format!("failed to parse active artifact manifest `{}`: {err}", path.display()),
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
    fs::create_dir_all(parent).map_err(|err| {
        diagnostic_set(
            &path,
            format!("failed to create artifact state directory `{}`: {err}", parent.display()),
        )
    })?;
    let contents = serde_json::to_vec_pretty(manifest).map_err(|err| {
        diagnostic_set(
            &path,
            format!("failed to serialize active artifact manifest: {err}"),
        )
    })?;
    AtomicFile::new(&path, AllowOverwrite)
        .write(|file| file.write_all(&contents))
        .map_err(|err| {
            diagnostic_set(
                &path,
                format!("failed to activate artifact manifest `{}`: {err}", path.display()),
            )
        })?;

    let activated = fs::read(&path).map_err(|err| {
        diagnostic_set(
            &path,
            format!("failed to verify active artifact manifest `{}`: {err}", path.display()),
        )
    })?;
    if activated != contents {
        return Err(diagnostic_set(
            &path,
            format!("verification failed for active artifact manifest `{}`", path.display()),
        ));
    }
    Ok(())
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
        .write(|file| file.write_all(&contents))
        .map_err(|err| {
            diagnostic_set(
                &path,
                format!("failed to update @idAsEnum lockfile `{}`: {err}", path.display()),
            )
        })
}

fn manifest_path(project: &Project) -> PathBuf {
    project
        .config_path
        .parent()
        .unwrap_or(&project.root_dir)
        .join(STATE_DIR)
        .join(ACTIVE_MANIFEST)
}

pub fn enum_lockfile_path(project: &Project) -> PathBuf {
    project
        .config_path
        .parent()
        .unwrap_or(&project.root_dir)
        .join(ENUM_LOCKFILE_NAME)
}

fn unique_revision() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{}-{timestamp}", std::process::id())
}

#[cfg(test)]
mod tests {
    use atomicwrites::{AllowOverwrite, AtomicFile};
    use std::io::{self, Write};

    #[test]
    fn failed_atomic_manifest_write_preserves_active_bytes() {
        let root = std::env::temp_dir().join(format!(
            "coflow-artifact-manifest-{}",
            super::unique_revision()
        ));
        std::fs::create_dir_all(&root).expect("create test directory");
        let path = root.join("active.json");
        std::fs::write(&path, b"old").expect("write old manifest");

        let error = AtomicFile::new(&path, AllowOverwrite).write(|file| {
            file.write_all(b"new")?;
            Err::<(), _>(io::Error::other("injected manifest failure"))
        });

        assert!(error.is_err());
        assert_eq!(std::fs::read(&path).expect("read active manifest"), b"old");
        std::fs::remove_dir_all(root).expect("remove test directory");
    }
}
