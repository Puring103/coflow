use atomicwrites::{AllowOverwrite, AtomicFile};
use coflow_api::DiagnosticSet;
use coflow_project::Project;
use include_dir::{include_dir, Dir, DirEntry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::diagnostics::{cli_error, cli_file_error};

const MANIFEST_SCHEMA_VERSION: u32 = 1;
const SKILL_NAMES: [&str; 3] = ["coflow-data", "coflow-schema", "coflow-workflow"];
const MANIFEST_RELATIVE_PATH: &str = ".coflow/skill-installs.json";

static BUNDLED_SKILLS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/skills");

/// Result of installing, uninstalling, or inspecting bundled skills.
#[derive(Debug, Serialize)]
pub struct SkillReport {
    pub operation: String,
    pub scope: String,
    pub bundle_version: String,
    pub targets: Vec<SkillTargetReport>,
}

/// Status of one agent skill directory.
#[derive(Debug, Serialize)]
pub struct SkillTargetReport {
    pub path: PathBuf,
    pub agents: Vec<String>,
    pub installed: bool,
}

#[derive(Debug, Clone)]
struct InstallTarget {
    path: PathBuf,
    agents: BTreeSet<String>,
}

#[derive(Debug)]
struct GlobalContext {
    home: PathBuf,
    config_home: PathBuf,
    claude_home: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct InstallManifest {
    schema_version: u32,
    bundle_version: String,
    targets: Vec<PathBuf>,
}

/// Install bundled skills into a Coflow project's `.agents/skills` directory.
///
/// # Errors
/// Returns diagnostics when the project cannot be resolved or files cannot be written.
pub fn install_project(config_or_dir: Option<&Path>) -> Result<SkillReport, DiagnosticSet> {
    let project = Project::open_schema_only(config_or_dir)?;
    let target = project_target(&project.root_dir);
    install_targets("project", vec![target])
}

/// Install bundled skills for the current user and detected agents.
///
/// # Errors
/// Returns diagnostics when the user home cannot be resolved or files cannot be written.
pub fn install_global() -> Result<SkillReport, DiagnosticSet> {
    install_global_in(&GlobalContext::current()?)
}

/// Remove bundled skills from a Coflow project.
///
/// # Errors
/// Returns diagnostics when the project cannot be resolved or files cannot be removed.
pub fn uninstall_project(config_or_dir: Option<&Path>) -> Result<SkillReport, DiagnosticSet> {
    let project = Project::open_schema_only(config_or_dir)?;
    uninstall_targets("project", vec![project_target(&project.root_dir)])
}

/// Remove global bundled skills previously installed for the current user.
///
/// # Errors
/// Returns diagnostics when the user home cannot be resolved or files cannot be removed.
pub fn uninstall_global() -> Result<SkillReport, DiagnosticSet> {
    uninstall_global_in(&GlobalContext::current()?)
}

/// Inspect bundled skill installation status for a Coflow project.
///
/// # Errors
/// Returns diagnostics when the project cannot be resolved.
pub fn status_project(config_or_dir: Option<&Path>) -> Result<SkillReport, DiagnosticSet> {
    let project = Project::open_schema_only(config_or_dir)?;
    Ok(report(
        "status",
        "project",
        vec![project_target(&project.root_dir)],
    ))
}

/// Inspect global bundled skill installation status for the current user.
///
/// # Errors
/// Returns diagnostics when the user home or installation manifest cannot be read.
pub fn status_global() -> Result<SkillReport, DiagnosticSet> {
    status_global_in(&GlobalContext::current()?)
}

impl GlobalContext {
    fn current() -> Result<Self, DiagnosticSet> {
        let home = dirs::home_dir().ok_or_else(|| {
            cli_error(
                "SKILL-HOME",
                "failed to resolve the current user's home directory",
            )
        })?;
        let config_home =
            absolute_env_path("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"));
        let claude_home =
            absolute_env_path("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home.join(".claude"));
        Ok(Self {
            home,
            config_home,
            claude_home,
        })
    }

    #[cfg(test)]
    fn for_home(home: PathBuf) -> Self {
        Self {
            config_home: home.join(".config"),
            claude_home: home.join(".claude"),
            home,
        }
    }
}

fn absolute_env_path(name: &str) -> Option<PathBuf> {
    let value = env::var_os(name)?;
    let path = PathBuf::from(value);
    path.is_absolute().then_some(path)
}

fn project_target(root: &Path) -> InstallTarget {
    InstallTarget {
        path: root.join(".agents/skills"),
        agents: BTreeSet::from(["project agents".to_string()]),
    }
}

fn install_global_in(context: &GlobalContext) -> Result<SkillReport, DiagnosticSet> {
    let targets = detected_global_targets(context);
    let result = install_targets("global", targets.clone())?;
    write_manifest(context, &targets)?;
    Ok(result)
}

fn uninstall_global_in(context: &GlobalContext) -> Result<SkillReport, DiagnosticSet> {
    let mut targets = detected_global_targets(context);
    if let Some(manifest) = read_manifest(context)? {
        let allowed = all_global_targets(context)
            .into_iter()
            .map(|target| target.path)
            .collect::<BTreeSet<_>>();
        for path in manifest.targets {
            if allowed.contains(&path) {
                merge_target(&mut targets, path, "previous installation");
            }
        }
    }
    let result = uninstall_targets("global", targets)?;
    remove_manifest(context)?;
    Ok(result)
}

fn status_global_in(context: &GlobalContext) -> Result<SkillReport, DiagnosticSet> {
    let mut targets = detected_global_targets(context);
    if let Some(manifest) = read_manifest(context)? {
        let allowed = all_global_targets(context)
            .into_iter()
            .map(|target| target.path)
            .collect::<BTreeSet<_>>();
        for path in manifest.targets {
            if allowed.contains(&path) {
                merge_target(&mut targets, path, "previous installation");
            }
        }
    }
    Ok(report("status", "global", targets))
}

fn detected_global_targets(context: &GlobalContext) -> Vec<InstallTarget> {
    global_targets(context, true)
}

fn all_global_targets(context: &GlobalContext) -> Vec<InstallTarget> {
    global_targets(context, false)
}

fn global_targets(context: &GlobalContext, detected_only: bool) -> Vec<InstallTarget> {
    let candidates = [
        (
            context.home.join(".agents/skills"),
            context.home.join(".agents"),
            "Codex and universal agents",
            true,
        ),
        (
            context.claude_home.join("skills"),
            context.claude_home.clone(),
            "Claude Code",
            false,
        ),
        (
            context.home.join(".cursor/skills"),
            context.home.join(".cursor"),
            "Cursor",
            false,
        ),
        (
            context.home.join(".gemini/skills"),
            context.home.join(".gemini"),
            "Gemini CLI",
            false,
        ),
        (
            context.home.join(".copilot/skills"),
            context.home.join(".copilot"),
            "GitHub Copilot",
            false,
        ),
        (
            context.config_home.join("opencode/skills"),
            context.config_home.join("opencode"),
            "OpenCode",
            false,
        ),
        (
            context.home.join(".codeium/windsurf/skills"),
            context.home.join(".codeium/windsurf"),
            "Windsurf",
            false,
        ),
    ];

    let mut targets = Vec::new();
    for (path, detection_root, agent, always_install) in candidates {
        if !detected_only || always_install || detection_root.exists() {
            merge_target(&mut targets, path, agent);
        }
    }
    targets
}

fn merge_target(targets: &mut Vec<InstallTarget>, path: PathBuf, agent: &str) {
    if let Some(target) = targets.iter_mut().find(|target| target.path == path) {
        target.agents.insert(agent.to_string());
    } else {
        targets.push(InstallTarget {
            path,
            agents: BTreeSet::from([agent.to_string()]),
        });
    }
}

fn install_targets(scope: &str, targets: Vec<InstallTarget>) -> Result<SkillReport, DiagnosticSet> {
    for target in &targets {
        fs::create_dir_all(&target.path).map_err(|error| {
            file_error(
                &target.path,
                "SKILL-DIR-CREATE",
                format!("failed to create skill directory: {error}"),
            )
        })?;
        for skill_name in SKILL_NAMES {
            install_skill(&target.path, skill_name)?;
        }
    }
    Ok(report("install", scope, targets))
}

fn uninstall_targets(
    scope: &str,
    targets: Vec<InstallTarget>,
) -> Result<SkillReport, DiagnosticSet> {
    for target in &targets {
        for skill_name in SKILL_NAMES {
            let path = target.path.join(skill_name);
            if path.exists() {
                fs::remove_dir_all(&path).map_err(|error| {
                    file_error(
                        &path,
                        "SKILL-REMOVE",
                        format!("failed to remove bundled skill: {error}"),
                    )
                })?;
            }
        }
        remove_if_empty(&target.path)?;
    }
    Ok(report("uninstall", scope, targets))
}

fn install_skill(target_root: &Path, skill_name: &str) -> Result<(), DiagnosticSet> {
    let source = BUNDLED_SKILLS.get_dir(skill_name).ok_or_else(|| {
        cli_error(
            "SKILL-BUNDLE",
            format!("bundled skill `{skill_name}` is missing"),
        )
    })?;
    if !has_direct_file(source, "SKILL.md") {
        return Err(cli_error(
            "SKILL-BUNDLE",
            format!("bundled skill `{skill_name}` has no SKILL.md"),
        ));
    }

    let token = unique_token();
    let staging = target_root.join(format!(".{skill_name}.coflow-staging-{token}"));
    let backup = target_root.join(format!(".{skill_name}.coflow-backup-{token}"));
    let destination = target_root.join(skill_name);

    extract_dir(source, &staging).inspect_err(|_| {
        let _ = fs::remove_dir_all(&staging);
    })?;

    let had_destination = destination.exists();
    if had_destination {
        fs::rename(&destination, &backup).map_err(|error| {
            let _ = fs::remove_dir_all(&staging);
            file_error(
                &destination,
                "SKILL-SWAP",
                format!("failed to stage the existing skill for replacement: {error}"),
            )
        })?;
    }

    if let Err(error) = fs::rename(&staging, &destination) {
        if had_destination {
            let _ = fs::rename(&backup, &destination);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(file_error(
            &destination,
            "SKILL-SWAP",
            format!("failed to activate bundled skill: {error}"),
        ));
    }

    if had_destination {
        fs::remove_dir_all(&backup).map_err(|error| {
            file_error(
                &backup,
                "SKILL-CLEANUP",
                format!("failed to remove replaced skill backup: {error}"),
            )
        })?;
    }
    Ok(())
}

fn has_direct_file(directory: &Dir<'_>, name: &str) -> bool {
    directory.entries().iter().any(|entry| {
        matches!(
            entry,
            DirEntry::File(file) if file.path().file_name().is_some_and(|file_name| file_name == name)
        )
    })
}

fn extract_dir(source: &Dir<'_>, destination: &Path) -> Result<(), DiagnosticSet> {
    fs::create_dir_all(destination).map_err(|error| {
        file_error(
            destination,
            "SKILL-DIR-CREATE",
            format!("failed to create bundled skill directory: {error}"),
        )
    })?;
    for entry in source.entries() {
        match entry {
            DirEntry::Dir(directory) => {
                let name = entry_name(directory.path())?;
                extract_dir(directory, &destination.join(name))?;
            }
            DirEntry::File(file) => {
                let name = entry_name(file.path())?;
                let path = destination.join(name);
                fs::write(&path, file.contents()).map_err(|error| {
                    file_error(
                        &path,
                        "SKILL-FILE-WRITE",
                        format!("failed to write bundled skill file: {error}"),
                    )
                })?;
            }
        }
    }
    Ok(())
}

fn entry_name(path: &Path) -> Result<&std::ffi::OsStr, DiagnosticSet> {
    path.file_name().ok_or_else(|| {
        cli_error(
            "SKILL-BUNDLE",
            format!("bundled skill entry `{}` has no file name", path.display()),
        )
    })
}

fn unique_token() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{timestamp}", std::process::id())
}

fn report(operation: &str, scope: &str, targets: Vec<InstallTarget>) -> SkillReport {
    SkillReport {
        operation: operation.to_string(),
        scope: scope.to_string(),
        bundle_version: env!("CARGO_PKG_VERSION").to_string(),
        targets: targets
            .into_iter()
            .map(|target| SkillTargetReport {
                installed: skills_installed(&target.path),
                path: target.path,
                agents: target.agents.into_iter().collect(),
            })
            .collect(),
    }
}

fn skills_installed(target: &Path) -> bool {
    SKILL_NAMES
        .iter()
        .all(|skill| target.join(skill).join("SKILL.md").is_file())
}

fn manifest_path(context: &GlobalContext) -> PathBuf {
    context.home.join(MANIFEST_RELATIVE_PATH)
}

fn write_manifest(context: &GlobalContext, targets: &[InstallTarget]) -> Result<(), DiagnosticSet> {
    let path = manifest_path(context);
    let parent = path.parent().ok_or_else(|| {
        cli_error(
            "SKILL-MANIFEST",
            format!(
                "skill manifest `{}` has no parent directory",
                path.display()
            ),
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        file_error(
            parent,
            "SKILL-MANIFEST",
            format!("failed to create skill manifest directory: {error}"),
        )
    })?;
    let manifest = InstallManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        bundle_version: env!("CARGO_PKG_VERSION").to_string(),
        targets: targets.iter().map(|target| target.path.clone()).collect(),
    };
    let contents = serde_json::to_vec_pretty(&manifest).map_err(|error| {
        file_error(
            &path,
            "SKILL-MANIFEST",
            format!("failed to serialize skill manifest: {error}"),
        )
    })?;
    AtomicFile::new(&path, AllowOverwrite)
        .write(|file| file.write_all(&contents))
        .map_err(|error| {
            file_error(
                &path,
                "SKILL-MANIFEST",
                format!("failed to write skill manifest: {error}"),
            )
        })
}

fn read_manifest(context: &GlobalContext) -> Result<Option<InstallManifest>, DiagnosticSet> {
    let path = manifest_path(context);
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read(&path).map_err(|error| {
        file_error(
            &path,
            "SKILL-MANIFEST",
            format!("failed to read skill manifest: {error}"),
        )
    })?;
    let manifest: InstallManifest = serde_json::from_slice(&contents).map_err(|error| {
        file_error(
            &path,
            "SKILL-MANIFEST",
            format!("failed to parse skill manifest: {error}"),
        )
    })?;
    if manifest.schema_version != MANIFEST_SCHEMA_VERSION {
        return Err(file_error(
            &path,
            "SKILL-MANIFEST",
            format!(
                "unsupported skill manifest schema version {}",
                manifest.schema_version
            ),
        ));
    }
    Ok(Some(manifest))
}

fn remove_manifest(context: &GlobalContext) -> Result<(), DiagnosticSet> {
    let path = manifest_path(context);
    if path.exists() {
        fs::remove_file(&path).map_err(|error| {
            file_error(
                &path,
                "SKILL-MANIFEST",
                format!("failed to remove skill manifest: {error}"),
            )
        })?;
    }
    if let Some(parent) = path.parent() {
        remove_if_empty(parent)?;
    }
    Ok(())
}

fn remove_if_empty(path: &Path) -> Result<(), DiagnosticSet> {
    if !path.is_dir() {
        return Ok(());
    }
    let mut entries = fs::read_dir(path).map_err(|error| {
        file_error(
            path,
            "SKILL-DIR-READ",
            format!("failed to inspect skill directory: {error}"),
        )
    })?;
    if entries.next().is_none() {
        fs::remove_dir(path).map_err(|error| {
            file_error(
                path,
                "SKILL-DIR-REMOVE",
                format!("failed to remove empty skill directory: {error}"),
            )
        })?;
    }
    Ok(())
}

fn file_error(path: &Path, code: &str, message: String) -> DiagnosticSet {
    cli_file_error(path, code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verify(condition: bool, message: &'static str) -> Result<(), String> {
        if condition {
            Ok(())
        } else {
            Err(message.to_string())
        }
    }

    #[test]
    fn embedded_bundle_contains_expected_skills() -> Result<(), String> {
        for name in SKILL_NAMES {
            let skill = BUNDLED_SKILLS
                .get_dir(name)
                .ok_or_else(|| format!("missing embedded skill directory: {name}"))?;
            verify(
                has_direct_file(skill, "SKILL.md"),
                "missing bundled SKILL.md",
            )?;
        }
        Ok(())
    }

    #[test]
    fn global_install_targets_universal_and_detected_agents() -> Result<(), String> {
        let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
        let home = temp.path().join("home");
        fs::create_dir_all(home.join(".cursor")).map_err(|error| error.to_string())?;
        let context = GlobalContext::for_home(home.clone());

        let installed = install_global_in(&context).map_err(|error| error.to_string())?;

        verify(installed.targets.len() == 2, "unexpected install targets")?;
        verify(
            home.join(".agents/skills/coflow-data/SKILL.md").is_file(),
            "universal skill was not installed",
        )?;
        verify(
            home.join(".cursor/skills/coflow-workflow/SKILL.md")
                .is_file(),
            "detected Cursor skill was not installed",
        )?;
        verify(
            !home.join(".claude/skills/coflow-data").exists(),
            "undetected Claude target was installed",
        )?;
        verify(
            manifest_path(&context).is_file(),
            "install manifest was not written",
        )?;

        let removed = uninstall_global_in(&context).map_err(|error| error.to_string())?;
        verify(removed.targets.len() == 2, "unexpected uninstall targets")?;
        verify(
            !home.join(".agents/skills/coflow-data").exists(),
            "universal skill was not removed",
        )?;
        verify(
            !home.join(".cursor/skills/coflow-workflow").exists(),
            "Cursor skill was not removed",
        )?;
        verify(
            !manifest_path(&context).exists(),
            "install manifest was not removed",
        )?;
        Ok(())
    }

    #[test]
    fn reinstall_replaces_managed_skill_contents() -> Result<(), String> {
        let temp = tempfile::tempdir().map_err(|error| error.to_string())?;
        let target = InstallTarget {
            path: temp.path().join(".agents/skills"),
            agents: BTreeSet::from(["test".to_string()]),
        };
        install_targets("project", vec![target.clone()]).map_err(|error| error.to_string())?;
        let extra = target.path.join("coflow-data/extra.txt");
        fs::write(&extra, "local change").map_err(|error| error.to_string())?;

        install_targets("project", vec![target.clone()]).map_err(|error| error.to_string())?;

        verify(!extra.exists(), "reinstall retained stale skill content")?;
        verify(
            skills_installed(&target.path),
            "reinstall did not activate all skills",
        )?;
        Ok(())
    }
}
