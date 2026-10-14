use super::{diagnostics::file_error, Project, SourceConfig};
use crate::api::DiagnosticSet;
use coflow_staging::{StagedChange, StagedFile, StagedRemoval};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectInputKind {
    Schema,
    Data,
}

/// Adds an existing file or directory to a project's configured inputs.
///
/// # Errors
///
/// Returns diagnostics when the selected path is invalid, overlaps another
/// input, or the updated project configuration cannot be published.
pub fn add_project_input(
    config_path: &Path,
    kind: ProjectInputKind,
    selected_path: &Path,
) -> Result<(), DiagnosticSet> {
    let mut project = Project::open_schema_only(Some(config_path))?;
    let selected = crate::canonicalize_path(selected_path).map_err(|error| {
        file_error(
            selected_path,
            "PROJECT-CONFIG-WRITE",
            "PROJECT",
            format!("failed to resolve selected source: {error}"),
        )
    })?;
    let configured = portable_config_path(project.root_dir(), &selected)?;

    let configured_roots = project
        .config
        .schema
        .paths()
        .iter()
        .chain(project.config.data.iter().map(SourceConfig::path))
        .chain(
            project
                .config
                .dimensions
                .values()
                .filter_map(|dimension| dimension.dir.as_ref()),
        )
        .map(|path| project.resolve_path(path))
        .collect::<Vec<_>>();
    let already_in_kind = roots_for(&project, kind).iter().any(|root| {
        super::normalized_path_identity(root) == super::normalized_path_identity(&selected)
    });
    if already_in_kind {
        return Ok(());
    }
    if configured_roots.iter().any(|root| {
        super::normalized_path_identity(root) == super::normalized_path_identity(&selected)
            || super::path_is_same_or_descendant(&selected, root)
            || super::path_is_same_or_descendant(root, &selected)
    }) {
        return Err(file_error(
            selected_path,
            "PROJECT-CONFIG-OVERLAP",
            "PROJECT",
            "configured schema and data inputs must not contain one another",
        ));
    }

    // 配置层负责去重，避免前端状态或重复点击产生语义相同的输入项。
    let changed = match kind {
        ProjectInputKind::Schema => {
            if project.config.schema.paths().contains(&configured) {
                false
            } else {
                project.config.schema.paths.push(configured);
                project.config.schema.list_shape = true;
                true
            }
        }
        ProjectInputKind::Data => {
            if project
                .config
                .data
                .iter()
                .any(|source| source.path() == &configured)
            {
                false
            } else {
                project
                    .config
                    .data
                    .push(SourceConfig::from_path(configured));
                true
            }
        }
    };
    if !changed {
        return Ok(());
    }
    let diagnostics = match kind {
        ProjectInputKind::Schema => project.schema_diagnostic_set(),
        ProjectInputKind::Data => project.data_diagnostic_set(),
    };
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    publish_config(config_path, &project)
}

/// Creates an empty CFT or CFD file under a configured input root.
///
/// # Errors
///
/// Returns diagnostics when the parent or file name is invalid, or staging
/// and publishing the file fails.
pub fn create_project_file(
    config_path: &Path,
    kind: ProjectInputKind,
    parent_path: &Path,
    file_name: &str,
) -> Result<(), DiagnosticSet> {
    let project = Project::open_schema_only(Some(config_path))?;
    let parent = project.resolve_path(parent_path);
    let parent = crate::canonicalize_path(&parent).map_err(|error| {
        file_error(
            &parent,
            "PROJECT-FILE-CREATE",
            "PROJECT",
            format!("failed to resolve parent directory: {error}"),
        )
    })?;
    let name = Path::new(file_name);
    if name.file_name().is_none() || name.components().count() != 1 {
        return Err(file_error(
            &parent,
            "PROJECT-FILE-CREATE",
            "PROJECT",
            "file name must not contain a directory path",
        ));
    }
    let expected_extension = match kind {
        ProjectInputKind::Schema => "cft",
        ProjectInputKind::Data => "cfd",
    };
    if name.extension().and_then(|value| value.to_str()) != Some(expected_extension) {
        return Err(file_error(
            name,
            "PROJECT-FILE-CREATE",
            "PROJECT",
            format!("file name must end with .{expected_extension}"),
        ));
    }
    if !roots_for(&project, kind)
        .iter()
        .any(|root| super::path_is_same_or_descendant(&parent, root))
    {
        return Err(file_error(
            &parent,
            "PROJECT-FILE-CREATE",
            "PROJECT",
            "target directory is outside the configured input roots",
        ));
    }
    let target = parent.join(name);
    let mut staged = StagedFile::create(&target, None, b"").map_err(|error| {
        file_error(&target, "PROJECT-FILE-CREATE", "PROJECT", error.to_string())
    })?;
    staged.publish().map_err(|error| {
        file_error(&target, "PROJECT-FILE-CREATE", "PROJECT", error.to_string())
    })?;
    staged.finish();
    Ok(())
}

/// Deletes a project file or configured input and updates the configuration.
///
/// # Errors
///
/// Returns diagnostics when the entry is outside the project inputs or the
/// staged deletion/configuration update cannot be published.
pub fn delete_project_entry(config_path: &Path, entry_path: &Path) -> Result<(), DiagnosticSet> {
    let mut project = Project::open_schema_only(Some(config_path))?;
    let target = project.resolve_path(entry_path);
    let target = crate::canonicalize_path(&target).map_err(|error| {
        file_error(
            &target,
            "PROJECT-FILE-DELETE",
            "PROJECT",
            format!("failed to resolve entry: {error}"),
        )
    })?;
    let root_dir = project.root_dir().to_path_buf();
    let mut removed_config = false;
    project.config.schema.paths.retain(|path| {
        let keep = !same_path(&root_dir, path, &target);
        removed_config |= !keep;
        keep
    });
    project.config.data.retain(|source| {
        let keep = !same_path(&root_dir, source.path(), &target);
        removed_config |= !keep;
        keep
    });
    for dimension in project.config.dimensions.values_mut() {
        if dimension
            .dir
            .as_ref()
            .is_some_and(|dir| same_path(&root_dir, dir, &target))
        {
            dimension.dir = None;
            removed_config = true;
        }
    }
    let configured_roots = roots_for(&project, ProjectInputKind::Schema)
        .into_iter()
        .chain(roots_for(&project, ProjectInputKind::Data))
        .collect::<Vec<_>>();
    let inside_project = super::path_is_same_or_descendant(&target, project.root_dir())
        && super::normalized_path_identity(&target)
            != super::normalized_path_identity(project.root_dir());
    if !removed_config
        && !inside_project
        && !configured_roots
            .iter()
            .any(|root| super::path_is_same_or_descendant(&target, root))
    {
        return Err(file_error(
            &target,
            "PROJECT-FILE-DELETE",
            "PROJECT",
            "entry is outside the configured input roots",
        ));
    }

    let mut removal = StagedRemoval::create(&target);
    removal.publish().map_err(|error| {
        file_error(&target, "PROJECT-FILE-DELETE", "PROJECT", error.to_string())
    })?;
    if removed_config {
        if let Err(mut error) = publish_config(config_path, &project) {
            if let Err(restoration) = removal.restore() {
                error.extend(file_error(&target, "PROJECT-FILE-RESTORE", "PROJECT", restoration.to_string()));
            }
            return Err(error);
        }
    }
    removal.finish();
    Ok(())
}

fn roots_for(project: &Project, kind: ProjectInputKind) -> Vec<PathBuf> {
    match kind {
        ProjectInputKind::Schema => project
            .config
            .schema
            .paths()
            .iter()
            .map(|path| project.resolve_path(path))
            .collect(),
        ProjectInputKind::Data => project
            .cfd_input_paths()
            .map(|path| project.resolve_path(path))
            .collect(),
    }
}

fn same_path(root: &Path, configured: &Path, target: &Path) -> bool {
    let resolved = if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        root.join(configured)
    };
    super::normalized_path_identity(&resolved) == super::normalized_path_identity(target)
}

fn publish_config(config_path: &Path, project: &Project) -> Result<(), DiagnosticSet> {
    let original = fs::read(config_path).map_err(|error| {
        file_error(
            config_path,
            "PROJECT-CONFIG-WRITE",
            "PROJECT",
            format!("failed to read project config: {error}"),
        )
    })?;
    // 所有配置路径统一相对项目根保存，项目外目录使用 ..，避免绑定本机绝对路径。
    let mut config = project.config.clone();
    for path in &mut config.schema.paths {
        *path = portable_config_path(project.root_dir(), path)?;
    }
    for source in &mut config.data {
        *source = SourceConfig::from_path(portable_config_path(project.root_dir(), source.path())?);
    }
    for output in &mut config.codegen {
        output.dir = portable_config_path(project.root_dir(), &output.dir)?;
    }
    for dimension in config.dimensions.values_mut() {
        if let Some(path) = &mut dimension.dir {
            *path = portable_config_path(project.root_dir(), path)?;
        }
    }
    let output = serde_yaml::to_string(&config).map_err(|error| {
        file_error(
            config_path,
            "PROJECT-CONFIG-WRITE",
            "PROJECT",
            format!("failed to encode project config: {error}"),
        )
    })?;
    let mut staged =
        StagedFile::create(config_path, Some(original), output.as_bytes()).map_err(|error| {
            file_error(
                config_path,
                "PROJECT-CONFIG-WRITE",
                "PROJECT",
                error.to_string(),
            )
        })?;
    staged.publish().map_err(|error| {
        file_error(
            config_path,
            "PROJECT-CONFIG-WRITE",
            "PROJECT",
            error.to_string(),
        )
    })?;
    staged.finish();
    Ok(())
}

fn portable_config_path(project_root: &Path, selected: &Path) -> Result<PathBuf, DiagnosticSet> {
    let root = super::normalize_path(project_root);
    let selected = super::resolve_project_relative(&root, selected);
    let root_parts = root.components().collect::<Vec<_>>();
    let selected_parts = selected.components().collect::<Vec<_>>();
    let common = root_parts.iter().zip(&selected_parts)
        .take_while(|(left, right)| left == right).count();
    if common == 0 {
        return Err(file_error(&selected, "PROJECT-CONFIG-WRITE", "PROJECT",
            "configured path must share a filesystem root with the project to be stored relatively"));
    }
    let mut relative = PathBuf::new();
    for _ in &root_parts[common..] {
        relative.push("..");
    }
    for part in &selected_parts[common..] {
        relative.push(part.as_os_str());
    }
    if relative.as_os_str().is_empty() { relative.push("."); }
    Ok(PathBuf::from(crate::path_to_slash(&relative)))
}
