use std::path::Path;

use crate::project::{
    normalize_path, path_is_same_or_descendant, resolve_project_relative,
    schema_path_policy::SchemaPathPolicy, DimensionConfig, OutputConfig, ProjectConfig,
    SchemaConfig, SourceConfig,
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

    fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
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
    for (name, dimension) in &config.dimensions {
        diagnostics.extend(validate_dimension_collecting(name, dimension));
        if let Some(dir) = &dimension.dir {
            let resolved = resolve_project_relative(root_dir, dir);
            if resolved.exists() && !resolved.is_dir() {
                diagnostics.push(ProjectDiagnostic::new(
                    "dimension dir must be a directory",
                    ["dimensions", name, "dir"],
                ));
            }
            if config.schema.paths().iter().any(|schema| {
                let schema = resolve_project_relative(root_dir, schema);
                path_is_same_or_descendant(&resolved, &schema)
                    || path_is_same_or_descendant(&schema, &resolved)
            }) {
                diagnostics.push(ProjectDiagnostic::new(
                    "dimension directory must not overlap schema inputs",
                    ["dimensions", name, "dir"],
                ));
            }
            for (other_name, other) in config.dimensions.range(..name.clone()) {
                if other.dir.as_ref().is_some_and(|other_dir| {
                    let other = resolve_project_relative(root_dir, other_dir);
                    path_is_same_or_descendant(&resolved, &other)
                        || path_is_same_or_descendant(&other, &resolved)
                }) {
                    diagnostics.push(ProjectDiagnostic::new(
                        format!("dimension directory overlaps dimensions.{other_name}.dir"),
                        ["dimensions", name, "dir"],
                    ));
                }
            }
        }
    }
    diagnostics
}

fn validate_dimension_collecting(
    dimension: &str,
    config: &DimensionConfig,
) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    if !coflow_language::lexical::is_cft_identifier(dimension) {
        diagnostics.push(
            ProjectDiagnostic::new(
                format!("dimension name `{dimension}` is not a valid CFT identifier"),
                ["dimensions", dimension],
            )
            .with_code("DIM-CONFIG-002"),
        );
    }
    if config
        .dir
        .as_ref()
        .is_some_and(|dir| dir.as_os_str().is_empty())
    {
        diagnostics.push(ProjectDiagnostic::new(
            "dimension dir is empty",
            ["dimensions", dimension, "dir"],
        ));
    }
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
