//! CLI 诊断保留源码位置和记录坐标，未知位置使用空值。

use std::path::Path;

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::{Diagnostic, DiagnosticLogicalLocation, Label, SourceLocation};

use super::RecordTarget;

#[derive(Debug, Clone, Serialize, serde::Deserialize, JsonSchema)]
pub struct CommandPosition {
    pub line: usize,
    pub character: usize,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, JsonSchema)]
pub struct CommandRange {
    pub start: CommandPosition,
    pub end: CommandPosition,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, JsonSchema)]
pub struct CommandLocation {
    pub path: String,
    pub range: Option<CommandRange>,
    pub config_path: Option<Vec<String>>,
}

#[derive(Debug, Serialize, serde::Deserialize, JsonSchema)]
pub struct CommandLabel {
    pub location: CommandLocation,
    pub message: Option<String>,
}

#[derive(Debug, Serialize, serde::Deserialize, JsonSchema)]
pub struct CommandContext {
    pub kind: String,
    pub name: Option<String>,
    pub expression: Option<String>,
    pub quantifier: Option<String>,
    pub binding: Option<String>,
    pub item: Option<String>,
    pub dimension: Option<String>,
    pub variant: Option<String>,
}

#[derive(Debug, Serialize, serde::Deserialize, JsonSchema)]
pub struct CommandDiagnostic {
    pub code: String,
    pub stage: String,
    pub severity: String,
    pub message: String,
    pub location: Option<CommandLocation>,
    pub record: Option<RecordTarget>,
    pub field_path: Option<String>,
    pub related: Vec<CommandLabel>,
    pub contexts: Vec<CommandContext>,
    pub explanation: Option<Value>,
}

impl CommandLocation {
    pub(crate) fn from_source(source: &SourceLocation, root: Option<&Path>) -> Self {
        let path = match source {
            SourceLocation::FileSpan { path, .. }
            | SourceLocation::Artifact { path }
            | SourceLocation::ProjectConfig { path, .. } => path,
        };
        let range = match source {
            SourceLocation::FileSpan { .. } => {
                let range = source.text_range();
                Some(CommandRange {
                    start: CommandPosition {
                        line: range.start.line,
                        character: range.start.character,
                    },
                    end: CommandPosition {
                        line: range.end.line,
                        character: range.end.character,
                    },
                })
            }
            _ => None,
        };
        Self {
            path: root.map_or_else(
                || crate::path_to_slash(path),
                |root| crate::project_path(root, path),
            ),
            range,
            config_path: match source {
                SourceLocation::ProjectConfig { key_path, .. } => Some(key_path.clone()),
                _ => None,
            },
        }
    }
}

impl CommandLabel {
    fn from_label(label: &Label, root: Option<&Path>) -> Self {
        Self {
            location: CommandLocation::from_source(&label.location, root),
            message: label.message.clone(),
        }
    }
}

impl CommandDiagnostic {
    pub(crate) fn new(
        diagnostic: &Diagnostic,
        logical: Option<&DiagnosticLogicalLocation>,
        root: Option<&Path>,
        explanation: Option<&Value>,
    ) -> Self {
        Self {
            code: diagnostic.code.clone(),
            stage: diagnostic.stage.clone(),
            severity: match diagnostic.severity {
                crate::Severity::Error => "error",
                crate::Severity::Warning => "warning",
                crate::Severity::Info => "info",
            }
            .into(),
            message: diagnostic.message.clone(),
            location: diagnostic
                .primary
                .as_ref()
                .map(|label| CommandLocation::from_source(&label.location, root)),
            record: logical.and_then(|location| {
                Some(RecordTarget {
                    actual_type: location.actual_type.clone()?,
                    id: location.record_id.clone()?,
                })
            }),
            field_path: logical.and_then(|location| location.field_path.clone()),
            related: diagnostic
                .related
                .iter()
                .map(|label| CommandLabel::from_label(label, root))
                .collect(),
            contexts: diagnostic
                .contexts
                .iter()
                .map(|context| CommandContext {
                    kind: context.kind.clone(),
                    name: context.name.clone(),
                    expression: context.expression.clone(),
                    quantifier: context.quantifier.clone(),
                    binding: context.binding.clone(),
                    item: context.item.clone(),
                    dimension: context.dimension.clone(),
                    variant: context.variant.clone(),
                })
                .collect(),
            explanation: explanation.cloned(),
        }
    }
}
