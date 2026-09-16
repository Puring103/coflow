use std::path::Path;

use crate::project::{
    normalize_path, path_is_same_or_descendant, resolve_project_relative,
    schema_path_policy::SchemaPathPolicy, OutputConfig, ProjectConfig, SchemaConfig, SourceConfig,
};

pub(super) struct ProjectDiagnostic {
    pub(super) code: Option<String>,
    pub(super) message: String,
    pub(super) key_path: Vec<String>,
}

impl ProjectDiagnostic {
    fn new(
        message: impl Into<String>,
        key_path: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            code: None,
            message: message.into(),
            key_path: key_path.into_iter().map(Into::into).collect(),
        }
    }
}

pub(super) fn validate_project_config_schema_only_collecting(
    root_dir: &Path,
    config: &ProjectConfig,
) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    diagnostics.extend(validate_schema_config_collecting(root_dir, &config.schema));
    diagnostics.extend(validate_codegen_collecting(&config.codegen));
    diagnostics.extend(validate_source_shapes_collecting(&config.data));
    diagnostics
}

fn validate_schema_config_collecting(
    root_dir: &Path,
    schema: &SchemaConfig,
) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    let policy = SchemaPathPolicy::new(root_dir);
    if schema.paths().is_empty() {
        diagnostics.push(ProjectDiagnostic::new("schema list is empty", ["schema"]));
    }
    for (index, path) in schema.paths().iter().enumerate() {
        let (label, key_path) = if schema.is_list_shape() {
            (
                format!("schema[{index}]"),
                vec!["schema".to_string(), index.to_string()],
            )
        } else {
            ("schema".to_string(), vec!["schema".to_string()])
        };
        if let Err(err) = policy.validate_config_path(path, &label) {
            diagnostics.push(ProjectDiagnostic::new(err, key_path));
        }
    }
    diagnostics
}

pub(super) fn validate_sources_collecting(
    root_dir: &Path,
    sources: &[SourceConfig],
) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = validate_source_shapes_collecting(sources);
    for (source_index, source) in sources.iter().enumerate() {
        let source_label = format!("data[{source_index}]");
        let source_index_key = source_index.to_string();
        let path = source.location();
        let resolved = resolve_project_relative(root_dir, path);
        if !resolved.is_file() && !resolved.is_dir() {
            diagnostics.push(ProjectDiagnostic::new(
                format!("{source_label}.path `{}` does not exist", path.display()),
                [
                    "data".to_string(),
                    source_index_key.clone(),
                    "path".to_string(),
                ],
            ));
        }
    }
    diagnostics
}

fn validate_source_shapes_collecting(sources: &[SourceConfig]) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    for (source_index, source) in sources.iter().enumerate() {
        let source_label = format!("data[{source_index}]");
        let source_index_key = source_index.to_string();
        if source.location().as_os_str().is_empty() {
            diagnostics.push(ProjectDiagnostic::new(
                format!("{source_label}.path is empty"),
                [
                    "data".to_string(),
                    source_index_key.clone(),
                    "path".to_string(),
                ],
            ));
        }
    }
    diagnostics
}

pub(super) fn validate_codegen_collecting(codegen: &[OutputConfig]) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    if codegen.is_empty() {
        diagnostics.push(ProjectDiagnostic::new(
            "coflow.yaml missing codegen target",
            ["codegen"],
        ));
    }
    let mut directories = Vec::new();
    for (index, target) in codegen.iter().enumerate() {
        let label = format!("codegen[{index}]");
        if target.language.trim().is_empty() {
            diagnostics.push(ProjectDiagnostic::new(
                format!("{label}.language is empty"),
                ["codegen", &index.to_string(), "language"],
            ));
        }
        if let Err(err) = validate_output_dir(&format!("{label}.dir"), &target.dir) {
            diagnostics.push(ProjectDiagnostic::new(
                err,
                ["codegen", &index.to_string(), "dir"],
            ));
        }
        directories.push((index, normalize_path(&target.dir)));
    }
    for (position, (left_index, left)) in directories.iter().enumerate() {
        for (right_index, right) in directories.iter().skip(position + 1) {
            if path_is_same_or_descendant(left, right) || path_is_same_or_descendant(right, left) {
                diagnostics.push(ProjectDiagnostic::new(
                    format!("codegen[{right_index}].dir overlaps codegen[{left_index}].dir"),
                    ["codegen", &right_index.to_string(), "dir"],
                ));
            }
        }
    }
    diagnostics
}

fn validate_output_dir(label: &str, path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        Err(format!("{label} is empty"))
    } else {
        Ok(())
    }
}
