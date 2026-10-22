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
    runtime_version: "0.11.0",
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
        // 锁文件保留已删除 id 的编号；输出仅包含本次数据中的有效记录 id。
        let mut active_values = input.id_as_enum_values.clone();
        for en in input.schema.all_enums() {
            let Some(record_type) = input.schema.type_for_id_as_enum(&en.name) else {
                continue;
            };
            let model = input.model.ok_or_else(|| {
                CodegenError::Message("@idAsEnum code generation requires CFD data".to_string())
            })?;
            let ids = model
                .records_assignable_to(input.schema, &record_type.name)
                .map(|(_, record)| record.id().to_string())
                .collect::<std::collections::BTreeSet<_>>();
            if let Some(entries) = active_values.get_mut(en.name.as_str()) {
                entries.retain(|id, _| ids.contains(id));
                for id in &ids {
                    if !entries.contains_key(id) {
                        return Err(CodegenError::Message(format!(
                            "missing stable value for `{en_name}::{id}`",
                            en_name = en.name
                        )));
                    }
                }
            }
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
