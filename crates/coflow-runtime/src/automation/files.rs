//! 文件摘要使用语法 AST，不要求 Schema 或数据模型构建成功。

use super::{Failure, InputFile, InputSnapshot, Result};
use crate::{Diagnostic, Label, SourceLocation, TextRange};
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DataFileRole {
    Base,
    Dimension,
    Mixed,
    Empty,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct FileDimensionSummary {
    pub dimension: String,
    pub variant: String,
    pub records: usize,
    pub types: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DataFileSummary {
    pub role: DataFileRole,
    pub base_records: usize,
    pub types: Vec<String>,
    pub dimensions: Vec<FileDimensionSummary>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectFile {
    #[serde(flatten)]
    pub file: InputFile,
    pub metadata_complete: bool,
    pub data: Option<DataFileSummary>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProjectFiles {
    pub files: Vec<ProjectFile>,
}

pub(super) fn inspect(
    snapshot: &InputSnapshot,
    kind: Option<&str>,
) -> Result<(ProjectFiles, Vec<Diagnostic>)> {
    let mut files = Vec::new();
    let mut diagnostics = Vec::new();
    for file in snapshot.files.iter().filter(|file| {
        ["schema", "data"].contains(&file.kind.as_str())
            && kind.is_none_or(|kind| file.kind == kind)
    }) {
        let mut entry = ProjectFile {
            file: file.clone(),
            metadata_complete: true,
            data: None,
        };
        if file.kind == "data" {
            let source = std::str::from_utf8(&file.contents)
                .map_err(|error| Failure::request(error.to_string()))?;
            let (ast, errors) = coflow_language::cfd::parse_cfd(source);
            // 语法失败时不能把恢复后的 AST 当作完整摘要；文件身份与指纹仍可返回。
            if errors.is_empty() {
                let role = match (ast.records.is_empty(), ast.dimensions.is_empty()) {
                    (false, true) => DataFileRole::Base,
                    (true, false) => DataFileRole::Dimension,
                    (false, false) => DataFileRole::Mixed,
                    (true, true) => DataFileRole::Empty,
                };
                let types: BTreeSet<_> = ast
                    .records
                    .iter()
                    .chain(ast.dimensions.iter().flat_map(|block| &block.records))
                    .map(|record| record.type_name.clone())
                    .collect();
                let mut dimensions: Vec<_> = ast
                    .dimensions
                    .into_iter()
                    .map(|block| FileDimensionSummary {
                        dimension: block.dimension,
                        variant: block.variant,
                        records: block.records.len(),
                        types: block
                            .records
                            .into_iter()
                            .map(|record| record.type_name)
                            .collect::<BTreeSet<_>>()
                            .into_iter()
                            .collect(),
                    })
                    .collect();
                dimensions
                    .sort_by(|a, b| (&a.dimension, &a.variant).cmp(&(&b.dimension, &b.variant)));
                entry.data = Some(DataFileSummary {
                    role,
                    base_records: ast.records.len(),
                    types: types.into_iter().collect(),
                    dimensions,
                });
            } else {
                entry.metadata_complete = false;
                diagnostics.extend(errors.into_iter().map(|error| {
                    let range =
                        TextRange::from_byte_offsets(source, error.span.start, error.span.end);
                    Diagnostic::error("CFD-TEXT-Syntax", "CFD", error.message).with_primary(Label {
                        location: SourceLocation::FileSpan {
                            path: file.absolute.clone(),
                            start_line: range.start.line,
                            start_character: range.start.character,
                            end_line: range.end.line,
                            end_character: range.end.character,
                        },
                        message: None,
                    })
                }));
            }
        }
        files.push(entry);
    }
    Ok((ProjectFiles { files }, diagnostics))
}
