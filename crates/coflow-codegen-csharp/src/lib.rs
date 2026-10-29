//! Lightweight C# business types and typed descriptors for direct CFD loading.

mod emit;
mod names;

use coflow_codegen::{
    CodeArtifactFile, CodeArtifactSet, CodeGenerator, CodegenDescriptor, CodegenError,
    CodegenInput, IdAsEnumValues,
};
use coflow_language::cft::CftSchema;
use serde::Deserialize;
use std::{fmt, path::PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub relative_path: PathBuf,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsharpCodegenError(String);
impl CsharpCodegenError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
impl fmt::Display for CsharpCodegenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
impl std::error::Error for CsharpCodegenError {}

/// Generates business declarations and a typed CFD loading entry point.
///
/// # Errors
/// Returns an error for names or data types that cannot be emitted as C#.
pub fn generate_csharp(schema: &CftSchema) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    emit::generate(schema, "", &IdAsEnumValues::new())
}

pub const CSHARP_CFD_CODEGEN_DESCRIPTOR: CodegenDescriptor = CodegenDescriptor {
    id: "csharp",
    language: "csharp",
    file_extensions: &["cs"],
    runtime_package: "Coflow.Loader",
    runtime_version: "0.14.0",
    needs_model: true,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct CsharpCfdCodeGenerator;
impl CodeGenerator for CsharpCfdCodeGenerator {
    fn descriptor(&self) -> &'static CodegenDescriptor {
        &CSHARP_CFD_CODEGEN_DESCRIPTOR
    }
    fn generate(&self, input: CodegenInput<'_>) -> Result<CodeArtifactSet, CodegenError> {
        let options =
            CsharpOptions::deserialize(input.target.options.clone()).map_err(|error| {
                CodegenError::Message(format!("invalid C# output options: {error}"))
            })?;
        // 仅收集当前记录使用的映射；多输出与历史条目无需整份复制后再删除。
        let mut active_values = IdAsEnumValues::new();
        for en in input.schema.all_enums() {
            let Some(record_type) = input.schema.type_for_id_as_enum(&en.name) else {
                continue;
            };
            let model = input.model.ok_or_else(|| {
                CodegenError::Message("@idAsEnum code generation requires CFD data".to_string())
            })?;
            let entries = input
                .id_as_enum_values
                .get(en.name.as_str())
                .ok_or_else(|| {
                    CodegenError::Message(format!(
                        "missing stable values for @idAsEnum enum `{}`",
                        en.name
                    ))
                })?;
            let mut active = std::collections::BTreeMap::new();
            for (_, record) in model.records_assignable_to(input.schema, &record_type.name) {
                let id = record.id();
                let value = entries.get(id).ok_or_else(|| {
                    CodegenError::Message(format!("missing stable value for `{}::{id}`", en.name))
                })?;
                active.insert(id.to_owned(), *value);
            }
            active_values.insert(en.name.to_string(), active);
        }
        let files = emit::generate(input.schema, &options.namespace, &active_values)
            .map_err(|error| CodegenError::Message(error.to_string()))?;
        CodeArtifactSet::new(
            files
                .into_iter()
                .map(|file| CodeArtifactFile {
                    relative_path: file.relative_path,
                    contents: file.contents,
                })
                .collect(),
        )
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CsharpOptions {
    namespace: String,
}

#[cfg(test)]
mod tests;
