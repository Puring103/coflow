use coflow_api::SourceLocationSpec;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::{
    normalize_path, path_is_same_or_descendant, path_to_slash, resolve_project_relative,
    schema_path_policy::SchemaPathPolicy, DimensionConfig, OutputsConfig, ProjectConfig,
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
    diagnostics.extend(validate_outputs_collecting(&config.outputs));
    diagnostics.extend(validate_source_shapes_collecting(&config.sources));
    diagnostics.extend(validate_dimensions_collecting(root_dir, &config.dimensions));
    diagnostics.extend(validate_dimension_source_overlap_collecting(
        root_dir,
        &config.sources,
        &config.dimensions,
    ));
    diagnostics
}

fn validate_dimension_source_overlap_collecting(
    root_dir: &Path,
    sources: &[SourceConfig],
    dimensions: &BTreeMap<String, DimensionConfig>,
) -> Vec<ProjectDiagnostic> {
    let dimension_dirs = dimensions
        .iter()
        .filter_map(|(dimension, config)| {
            config.out_dir.as_ref().map(|out_dir| {
                (
                    dimension.as_str(),
                    normalize_path(&resolve_project_relative(root_dir, out_dir)),
                )
            })
        })
        .collect::<Vec<_>>();
    if dimension_dirs.is_empty() {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        let SourceLocationSpec::Path(path) = source.location();
        let source_path = normalize_path(&resolve_project_relative(root_dir, path));
        for (dimension, out_dir) in &dimension_dirs {
            if path_is_same_or_descendant(&source_path, out_dir) {
                diagnostics.push(
                    ProjectDiagnostic::new(
                        format!(
                            "source `{}` is inside dimensions.{dimension}.out_dir and is managed by Coflow; remove it from sources",
                            path_to_slash(path)
                        ),
                        ["sources".to_string(), index.to_string()],
                    )
                    .with_code("DIM-SOURCE-003"),
                );
            }
        }
    }
    diagnostics
}

fn validate_dimensions_collecting(
    root_dir: &Path,
    dimensions: &BTreeMap<String, DimensionConfig>,
) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    let mut owned_dirs = Vec::new();
    for (name, config) in dimensions {
        diagnostics.extend(validate_dimension_collecting(name, config));
        if let Some(out_dir) = &config.out_dir {
            owned_dirs.push((
                name,
                normalize_path(&resolve_project_relative(root_dir, out_dir)),
            ));
        }
    }
    for (index, (name, path)) in owned_dirs.iter().enumerate() {
        for (other_name, other_path) in owned_dirs.iter().skip(index + 1) {
            if path_is_same_or_descendant(path, other_path)
                || path_is_same_or_descendant(other_path, path)
            {
                diagnostics.push(
                    ProjectDiagnostic::new(
                        format!(
                            "dimensions.{other_name}.out_dir overlaps dimensions.{name}.out_dir; every dimension requires an exclusive managed directory"
                        ),
                        ["dimensions", other_name.as_str(), "out_dir"],
                    )
                    .with_code("DIM-SOURCE-007"),
                );
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
    if !coflow_cft::is_cft_identifier(dimension) {
        diagnostics.push(
            ProjectDiagnostic::new(
                format!("dimension name `{dimension}` is not a valid CFT identifier"),
                ["dimensions", dimension],
            )
            .with_code("DIM-CONFIG-002"),
        );
    }
    if config.out_dir.is_none() {
        diagnostics.push(
            ProjectDiagnostic::new(
                format!("dimensions.{dimension}.out_dir is required"),
                ["dimensions", dimension, "out_dir"],
            )
            .with_code("DIM-CONFIG-003"),
        );
    }
    if config.variants.is_empty() {
        diagnostics.push(
            ProjectDiagnostic::new(
                format!("dimensions.{dimension}.variants must not be empty"),
                ["dimensions", dimension, "variants"],
            )
            .with_code("DIM-CONFIG-002"),
        );
    }
    let mut seen = BTreeSet::new();
    for (index, variant) in config.variants.iter().enumerate() {
        let key_path = vec![
            "dimensions".to_string(),
            dimension.to_string(),
            "variants".to_string(),
            index.to_string(),
        ];
        if variant == "default" {
            diagnostics.push(
                ProjectDiagnostic::new(
                    format!(
                        "dimensions.{dimension}.variants cannot include reserved variant `default`"
                    ),
                    key_path.clone(),
                )
                .with_code("DIM-CONFIG-002"),
            );
            continue;
        }
        if !coflow_cft::is_cft_identifier(variant) {
            diagnostics.push(
                ProjectDiagnostic::new(
                    format!(
                        "dimensions.{dimension}.variants[{index}] `{variant}` is not a valid CFT identifier"
                    ),
                    key_path.clone(),
                )
                .with_code("DIM-CONFIG-002"),
            );
            continue;
        }
        if !seen.insert(variant.clone()) {
            diagnostics.push(
                ProjectDiagnostic::new(
                    format!(
                        "dimensions.{dimension}.variants contains duplicate variant `{variant}`"
                    ),
                    key_path,
                )
                .with_code("DIM-CONFIG-002"),
            );
        }
    }
    diagnostics
}

fn validate_schema_config_collecting(
    root_dir: &Path,
    schema: &SchemaConfig,
) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    let policy = SchemaPathPolicy::new(root_dir);
    match schema {
        SchemaConfig::One(path) => {
            if let Err(err) = policy.validate_config_path(path, "schema") {
                diagnostics.push(ProjectDiagnostic::new(err, ["schema"]));
            }
        }
        SchemaConfig::Many(paths) => {
            if paths.is_empty() {
                diagnostics.push(ProjectDiagnostic::new("schema list is empty", ["schema"]));
            }
            for (index, path) in paths.iter().enumerate() {
                if let Err(err) = policy.validate_config_path(path, &format!("schema[{index}]")) {
                    diagnostics.push(ProjectDiagnostic::new(
                        err,
                        ["schema".to_string(), index.to_string()],
                    ));
                }
            }
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
        let source_label = format!("sources[{source_index}]");
        let source_index_key = source_index.to_string();
        match &source.location {
            SourceLocationSpec::Path(path) => {
                let resolved = resolve_project_relative(root_dir, path);
                if !resolved.is_file() && !resolved.is_dir() {
                    diagnostics.push(ProjectDiagnostic::new(
                        format!("{source_label}.path `{}` does not exist", path.display()),
                        [
                            "sources".to_string(),
                            source_index_key.clone(),
                            "path".to_string(),
                        ],
                    ));
                }
            }
        }
    }
    diagnostics
}

fn validate_source_shapes_collecting(sources: &[SourceConfig]) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    for (source_index, source) in sources.iter().enumerate() {
        let source_label = format!("sources[{source_index}]");
        let source_index_key = source_index.to_string();
        if source
            .source_type
            .as_ref()
            .is_some_and(|source_type| source_type.trim().is_empty())
        {
            diagnostics.push(ProjectDiagnostic::new(
                format!("{source_label}.type is empty"),
                [
                    "sources".to_string(),
                    source_index_key.clone(),
                    "type".to_string(),
                ],
            ));
        }
        match &source.location {
            SourceLocationSpec::Path(path) if path.as_os_str().is_empty() => {
                diagnostics.push(ProjectDiagnostic::new(
                    format!("{source_label}.path is empty"),
                    [
                        "sources".to_string(),
                        source_index_key.clone(),
                        "path".to_string(),
                    ],
                ));
            }
            SourceLocationSpec::Path(_) => {}
        }
    }
    diagnostics
}

fn validate_outputs_collecting(outputs: &OutputsConfig) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    for (index, target) in outputs.targets().iter().enumerate() {
        let data = &target.data;
        let data_label = output_label(outputs, index, "data");
        if data.output_type.trim().is_empty() {
            diagnostics.push(ProjectDiagnostic::new(
                format!("{data_label}.type is empty"),
                output_key_path(outputs, index, "data", Some("type")),
            ));
        }
        if let Err(err) = validate_output_dir(&format!("{data_label}.dir"), &data.dir) {
            diagnostics.push(ProjectDiagnostic::new(
                err,
                output_key_path(outputs, index, "data", Some("dir")),
            ));
        }
        if let Some(code) = &target.code {
            let code_label = output_label(outputs, index, "code");
            if code.output_type.trim().is_empty() {
                diagnostics.push(ProjectDiagnostic::new(
                    format!("{code_label}.type is empty"),
                    output_key_path(outputs, index, "code", Some("type")),
                ));
            }
            if let Err(err) = validate_output_dir(&format!("{code_label}.dir"), &code.dir) {
                diagnostics.push(ProjectDiagnostic::new(
                    err,
                    output_key_path(outputs, index, "code", Some("dir")),
                ));
            }
        } else if target.loader.is_some() {
            diagnostics.push(ProjectDiagnostic::new(
                format!(
                    "{}.loader requires code output configuration",
                    output_target_label(outputs, index)
                ),
                output_key_path(outputs, index, "loader", None),
            ));
        }
        if target
            .loader
            .as_ref()
            .is_some_and(|loader| loader.loader_type.trim().is_empty())
        {
            diagnostics.push(ProjectDiagnostic::new(
                format!(
                    "{}.loader.type is empty",
                    output_target_label(outputs, index)
                ),
                output_key_path(outputs, index, "loader", Some("type")),
            ));
        }
    }
    diagnostics
}

pub(super) fn validate_for_codegen_collecting(outputs: &OutputsConfig) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = validate_outputs_collecting(outputs);
    if !outputs.targets().iter().any(|target| target.code.is_some()) {
        diagnostics.push(ProjectDiagnostic::new(
            "coflow.yaml missing outputs.code",
            ["outputs", "code"],
        ));
    }
    if outputs.targets().is_empty() {
        diagnostics.push(ProjectDiagnostic::new(
            "coflow.yaml missing outputs.data",
            ["outputs", "data"],
        ));
    }
    diagnostics
}

fn output_target_label(outputs: &OutputsConfig, index: usize) -> String {
    if outputs.is_legacy_shape() {
        "outputs".to_string()
    } else {
        format!("outputs[{index}]")
    }
}

fn output_label(outputs: &OutputsConfig, index: usize, component: &str) -> String {
    format!("{}.{component}", output_target_label(outputs, index))
}

fn output_key_path(
    outputs: &OutputsConfig,
    index: usize,
    component: &str,
    field: Option<&str>,
) -> Vec<String> {
    let mut path = vec!["outputs".to_string()];
    if !outputs.is_legacy_shape() {
        path.push(index.to_string());
    }
    path.push(component.to_string());
    if let Some(field) = field {
        path.push(field.to_string());
    }
    path
}

fn validate_output_dir(label: &str, path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        Err(format!("{label} is empty"))
    } else {
        Ok(())
    }
}
