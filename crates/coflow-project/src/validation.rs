use coflow_api::SourceLocationSpec;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::{
    normalize_path, path_to_slash, resolve_project_relative, schema_path_policy::SchemaPathPolicy,
    DimensionConfig, OutputsConfig, ProjectConfig, SchemaConfig, SourceConfig,
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
    diagnostics.extend(validate_dimensions_collecting(&config.dimensions));
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
        let SourceLocationSpec::Path(path) = source.location() else {
            continue;
        };
        let source_path = normalize_path(&resolve_project_relative(root_dir, path));
        for (dimension, out_dir) in &dimension_dirs {
            if source_path == *out_dir || source_path.starts_with(out_dir) {
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
    dimensions: &BTreeMap<String, DimensionConfig>,
) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    if let Some(config) = dimensions.get("language") {
        diagnostics.extend(validate_language_dimension_collecting(config));
    }
    diagnostics
}

fn validate_language_dimension_collecting(config: &DimensionConfig) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    if config.out_dir.is_none() {
        diagnostics.push(
            ProjectDiagnostic::new(
                "dimensions.language.out_dir is required",
                ["dimensions", "language", "out_dir"],
            )
            .with_code("DIM-CONFIG-003"),
        );
    }
    if config.variants.is_empty() {
        diagnostics.push(
            ProjectDiagnostic::new(
                "dimensions.language.variants must not be empty",
                ["dimensions", "language", "variants"],
            )
            .with_code("DIM-CONFIG-002"),
        );
    }
    let mut seen = BTreeSet::new();
    for (index, variant) in config.variants.iter().enumerate() {
        let key_path = vec![
            "dimensions".to_string(),
            "language".to_string(),
            "variants".to_string(),
            index.to_string(),
        ];
        if variant == "default" {
            diagnostics.push(
                ProjectDiagnostic::new(
                    "dimensions.language.variants cannot include reserved variant `default`",
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
                        "dimensions.language.variants[{index}] `{variant}` is not a valid CFT identifier"
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
                    format!("dimensions.language.variants contains duplicate variant `{variant}`"),
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
            SourceLocationSpec::Uri(_) => {}
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
            SourceLocationSpec::Uri(uri) if uri.trim().is_empty() => {
                diagnostics.push(ProjectDiagnostic::new(
                    format!("{source_label}.url is empty"),
                    [
                        "sources".to_string(),
                        source_index_key.clone(),
                        "url".to_string(),
                    ],
                ));
            }
            SourceLocationSpec::Path(_) | SourceLocationSpec::Uri(_) => {}
        }
    }
    diagnostics
}

fn validate_outputs_collecting(outputs: &OutputsConfig) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    if let Some(data) = &outputs.data {
        if data.output_type.trim().is_empty() {
            diagnostics.push(ProjectDiagnostic::new(
                "outputs.data.type is empty",
                ["outputs", "data", "type"],
            ));
        }
        if let Err(err) = validate_output_dir("outputs.data.dir", &data.dir) {
            diagnostics.push(ProjectDiagnostic::new(err, ["outputs", "data", "dir"]));
        }
    }
    if let Some(code) = &outputs.code {
        if code.output_type.trim().is_empty() {
            diagnostics.push(ProjectDiagnostic::new(
                "outputs.code.type is empty",
                ["outputs", "code", "type"],
            ));
        }
        if let Err(err) = validate_output_dir("outputs.code.dir", &code.dir) {
            diagnostics.push(ProjectDiagnostic::new(err, ["outputs", "code", "dir"]));
        }
    }
    diagnostics
}

pub(super) fn validate_for_codegen_collecting(outputs: &OutputsConfig) -> Vec<ProjectDiagnostic> {
    let mut diagnostics = Vec::new();
    match outputs.code.as_ref() {
        Some(code) => {
            if code.output_type.trim().is_empty() {
                diagnostics.push(ProjectDiagnostic::new(
                    "coflow.yaml outputs.code.type is empty",
                    ["outputs", "code", "type"],
                ));
            }
            if let Err(err) = validate_output_dir("outputs.code.dir", &code.dir) {
                diagnostics.push(ProjectDiagnostic::new(err, ["outputs", "code", "dir"]));
            }
        }
        None => diagnostics.push(ProjectDiagnostic::new(
            "coflow.yaml missing outputs.code",
            ["outputs", "code"],
        )),
    }
    match outputs.data.as_ref() {
        Some(data) => {
            if data.output_type.trim().is_empty() {
                diagnostics.push(ProjectDiagnostic::new(
                    "coflow.yaml outputs.data.type is empty",
                    ["outputs", "data", "type"],
                ));
            }
            if let Err(err) = validate_output_dir("outputs.data.dir", &data.dir) {
                diagnostics.push(ProjectDiagnostic::new(err, ["outputs", "data", "dir"]));
            }
        }
        None => diagnostics.push(ProjectDiagnostic::new(
            "coflow.yaml missing outputs.data",
            ["outputs", "data"],
        )),
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
