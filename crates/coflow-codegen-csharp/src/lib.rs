//! C# code generator for Coflow declarations and direct CFD bindings.

#![cfg_attr(
    not(test),
    deny(
        clippy::dbg_macro,
        clippy::expect_used,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable,
        clippy::unwrap_used
    )
)]

mod emit;
mod ir;
mod lowering;
mod model;
mod names;
mod render;

use coflow_language::CftSchema;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use coflow_runtime::codegen::{
    CodeArtifactFile, CodeArtifactSet, CodeGenerator as CfdCodeGeneratorTrait,
    CodegenDescriptor as CfdCodegenDescriptor, CodegenError, CodegenInput as CfdCodegenInput,
    SourceManifestEntry, SourceOrigin,
};

pub use ir::{CsharpCodegenOptions, CsharpIdAsEnumVariant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub relative_path: PathBuf,
    pub contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsharpCodegenError {
    messages: Vec<String>,
}

impl CsharpCodegenError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            messages: vec![message.into()],
        }
    }

    fn from_messages(messages: impl IntoIterator<Item = String>) -> Self {
        Self {
            messages: messages.into_iter().collect(),
        }
    }
}

impl fmt::Display for CsharpCodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.messages.join("\n").fmt(f)
    }
}

impl std::error::Error for CsharpCodegenError {}

fn build_csharp_project(
    schema: &CftSchema,
    options: &CsharpCodegenOptions,
    id_as_enum_variants: BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    non_empty_tables: Option<&BTreeSet<String>>,
) -> Result<model::CsharpProject, CsharpCodegenError> {
    ir::build_project(schema, options, id_as_enum_variants, non_empty_tables)
}

/// Generates format-independent C# declarations.
///
/// # Errors
///
/// Returns an error when the schema cannot be mapped to C# runtime code or a
/// template fails to render.
pub fn generate_csharp(
    schema: &CftSchema,
    options: &CsharpCodegenOptions,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    generate_common_with_id_as_enum_variants(schema, options, BTreeMap::new(), None)
}

/// Generates C# declarations plus a direct CFD source loader. The loader
/// consumes the logical paths in `sources` through `Coflow.Cfd.Runtime`.
pub fn generate_csharp_cfd(
    schema: &CftSchema,
    options: &CsharpCodegenOptions,
    sources: &[String],
    id_as_enum_variants: BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    non_empty_tables: Option<&BTreeSet<String>>,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let sources = sources
        .iter()
        .map(|logical_path| SourceManifestEntry {
            logical_path: logical_path.clone(),
            origin: SourceOrigin::Project,
        })
        .collect::<Vec<_>>();
    generate_csharp_cfd_with_manifest(
        schema,
        options,
        &sources,
        id_as_enum_variants,
        non_empty_tables,
    )
}

fn generate_csharp_cfd_with_manifest(
    schema: &CftSchema,
    options: &CsharpCodegenOptions,
    sources: &[SourceManifestEntry],
    id_as_enum_variants: BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    non_empty_tables: Option<&BTreeSet<String>>,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let project = build_csharp_project(schema, options, id_as_enum_variants, non_empty_tables)?;
    let mut files = render::render_common_project(&project)?;
    files.push(GeneratedFile {
        relative_path: PathBuf::from(format!("{}.Cfd.cs", project.database_class)),
        contents: render::render_cfd_loader(&project, sources, schema),
    });
    Ok(files)
}

pub const CSHARP_CFD_CODEGEN_DESCRIPTOR: CfdCodegenDescriptor = CfdCodegenDescriptor {
    id: "csharp",
    language: "csharp",
    file_extensions: &["cs"],
    runtime_package: "Coflow.Cfd.Runtime",
    runtime_version: "0.9.1",
    needs_model: true,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct CsharpCfdCodeGenerator;

impl CfdCodeGeneratorTrait for CsharpCfdCodeGenerator {
    fn descriptor(&self) -> &'static CfdCodegenDescriptor {
        &CSHARP_CFD_CODEGEN_DESCRIPTOR
    }

    fn generate(&self, input: CfdCodegenInput<'_>) -> Result<CodeArtifactSet, CodegenError> {
        let raw = input.target.options.clone();
        let options = CsharpOutputOptionsConfig::deserialize(raw).map_err(|error| {
            CodegenError::Message(format!("invalid C# output options: {error}"))
        })?;
        let codegen =
            CsharpCodegenOptions::new(options.namespace.as_deref().unwrap_or("Game.Config"))
                .with_database_class(options.database_class.as_deref().unwrap_or("CoflowTables"))
                .with_int_32(options.int_32)
                .with_float_32(options.float_32);
        let files = generate_csharp_cfd_with_manifest(
            input.schema,
            &codegen,
            input.sources,
            BTreeMap::new(),
            None,
        )
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

fn generate_common_with_id_as_enum_variants(
    schema: &CftSchema,
    options: &CsharpCodegenOptions,
    id_as_enum_variants: BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    non_empty_tables: Option<&BTreeSet<String>>,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let project = build_csharp_project(schema, options, id_as_enum_variants, non_empty_tables)?;
    render::render_common_project(&project)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CsharpOutputOptionsConfig {
    namespace: Option<String>,
    database_class: Option<String>,
    int_32: bool,
    float_32: bool,
}

#[cfg(test)]
mod tests;
