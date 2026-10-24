//! 项目查询输出保持配置的声明形状，公共 DTO 不暴露内部配置类型。
use super::Result;
use crate::Project;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SchemaInputs {
    Path(String),
    Paths(Vec<String>),
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct CodegenTarget {
    pub language: String,
    pub dir: String,
    #[serde(flatten)]
    pub options: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectDimension {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectConfiguration {
    pub schema: SchemaInputs,
    pub data: Vec<String>,
    pub codegen: Vec<CodegenTarget>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dimensions: BTreeMap<String, ProjectDimension>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectInspection {
    pub config: ProjectConfiguration,
    pub config_path: String,
    pub input_roots: Vec<String>,
}

pub(super) fn inspect(project: &Project) -> Result<ProjectInspection> {
    let config = project.config();
    let paths = config
        .schema
        .paths()
        .iter()
        .map(|path| crate::path_to_slash(path))
        .collect::<Vec<_>>();
    let schema = if config.schema.is_list_shape() || paths.len() != 1 {
        SchemaInputs::Paths(paths)
    } else {
        SchemaInputs::Path(paths[0].clone())
    };
    let codegen = config
        .codegen
        .iter()
        .map(|target| {
            Ok(CodegenTarget {
                language: target.language.clone(),
                dir: crate::path_to_slash(&target.dir),
                options: serde_json::from_value(target.options().clone())?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ProjectInspection {
        config: ProjectConfiguration {
            schema,
            data: config
                .data
                .iter()
                .map(|source| crate::path_to_slash(source.path()))
                .collect(),
            codegen,
            dimensions: config
                .dimensions
                .iter()
                .map(|(name, dimension)| {
                    (
                        name.clone(),
                        ProjectDimension {
                            dir: dimension.dir.as_deref().map(crate::path_to_slash),
                            display_name: dimension.display_name.clone(),
                        },
                    )
                })
                .collect(),
        },
        config_path: crate::project_path(project.root_dir(), project.config_path()),
        input_roots: project
            .source_roots()
            .iter()
            .map(|path| crate::path_to_slash(path))
            .collect(),
    })
}
