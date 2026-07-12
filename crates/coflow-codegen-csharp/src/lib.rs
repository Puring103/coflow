//! C# code generator for Coflow's exported JSON and `MessagePack` runtime data.
//!
//! The generated C# code does not depend on CFT at runtime. CFT is consumed only
//! by this Rust crate during generation.

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

use coflow_api::{
    ArtifactFile, ArtifactSet, CodeGenerator, CodegenContext, CodegenDescriptor,
    DecodedOutputOptions, Diagnostic, DiagnosticSet, ProviderBundle, ProviderRegistrationError,
};
use coflow_cft::CompiledSchema;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

pub use ir::{CsharpCodegenOptions, CsharpDataFormat, CsharpIdAsEnumVariant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub relative_path: PathBuf,
    pub contents: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsharpTemplate {
    pub name: &'static str,
    pub contents: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CsharpDatabaseTemplates {
    pub database_template: CsharpTemplate,
    pub partials: &'static [CsharpTemplate],
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

    fn messages(&self) -> impl Iterator<Item = &str> {
        self.messages.iter().map(String::as_str)
    }
}

impl fmt::Display for CsharpCodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.messages.join("\n").fmt(f)
    }
}

impl std::error::Error for CsharpCodegenError {}

/// Generates C# files using a caller-provided database loader template set.
///
/// # Errors
///
/// Returns an error when the compiled schema cannot be mapped to C# runtime
/// code or when a Tera template fails to render.
pub fn generate_csharp(
    schema: &CompiledSchema,
    options: &CsharpCodegenOptions,
    data_format: CsharpDataFormat,
    database_templates: &CsharpDatabaseTemplates,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    generate_csharp_with_database_templates(schema, options, data_format, database_templates)
}

/// Generates C# files using a caller-provided database loader template set.
///
/// Format-specific crates use this to keep JSON and `MessagePack` templates out
/// of the shared C# codegen core.
///
/// # Errors
///
/// Returns an error when the compiled schema cannot be mapped to C# runtime
/// code or when a Tera template fails to render.
pub fn generate_csharp_with_database_templates(
    schema: &CompiledSchema,
    options: &CsharpCodegenOptions,
    data_format: CsharpDataFormat,
    database_templates: &CsharpDatabaseTemplates,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    generate_csharp_with_id_as_enum_variants(
        schema,
        options,
        data_format,
        database_templates,
        BTreeMap::new(),
        None,
    )
}

/// Generates C# files and includes data-driven enum variants for types marked
/// with `@idAsEnum`.
///
/// # Errors
///
/// Returns an error when the compiled schema cannot be mapped to C# runtime
/// code or when a Tera template fails to render.
pub fn generate_csharp_with_id_as_enum_variants(
    schema: &CompiledSchema,
    options: &CsharpCodegenOptions,
    data_format: CsharpDataFormat,
    database_templates: &CsharpDatabaseTemplates,
    id_as_enum_variants: BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    non_empty_tables: Option<&std::collections::BTreeSet<String>>,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let project = ir::build_project(
        schema,
        options,
        data_format,
        id_as_enum_variants,
        non_empty_tables,
    )?;
    render::render_project(&project, database_templates)
}

const JSON_DATABASE_TEMPLATES: CsharpDatabaseTemplates = CsharpDatabaseTemplates {
    database_template: CsharpTemplate {
        name: "database_json.cs.tera",
        contents: include_str!("../templates/json/database_json.cs.tera"),
    },
    partials: &[],
};

const MESSAGEPACK_DATABASE_TEMPLATES: CsharpDatabaseTemplates = CsharpDatabaseTemplates {
    database_template: CsharpTemplate {
        name: "database_messagepack.cs.tera",
        contents: include_str!("../templates/messagepack/database_messagepack.cs.tera"),
    },
    partials: &[],
};

/// Generates C# type definitions and a Newtonsoft.Json based folder loader.
///
/// # Errors
///
/// Returns an error when the compiled schema cannot be mapped to C# runtime
/// code or when a Tera template fails to render.
pub fn generate_csharp_json(
    schema: &CompiledSchema,
    options: &CsharpCodegenOptions,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    generate_csharp_with_database_templates(
        schema,
        options,
        CsharpDataFormat::Json,
        &JSON_DATABASE_TEMPLATES,
    )
}

/// Generates C# JSON loader files and includes data-driven `@idAsEnum`
/// variants.
///
/// # Errors
///
/// Returns an error when the compiled schema cannot be mapped to C# runtime
/// code or when a Tera template fails to render.
pub fn generate_csharp_json_with_id_as_enum_variants(
    schema: &CompiledSchema,
    options: &CsharpCodegenOptions,
    id_as_enum_variants: BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    non_empty_tables: Option<&std::collections::BTreeSet<String>>,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    generate_csharp_with_id_as_enum_variants(
        schema,
        options,
        CsharpDataFormat::Json,
        &JSON_DATABASE_TEMPLATES,
        id_as_enum_variants,
        non_empty_tables,
    )
}

/// Generates C# type definitions and a `MessagePack` based folder loader.
///
/// # Errors
///
/// Returns an error when the compiled schema cannot be mapped to C# runtime
/// code or when a Tera template fails to render.
pub fn generate_csharp_messagepack(
    schema: &CompiledSchema,
    options: &CsharpCodegenOptions,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    generate_csharp_with_database_templates(
        schema,
        options,
        CsharpDataFormat::MessagePack,
        &MESSAGEPACK_DATABASE_TEMPLATES,
    )
}

/// Generates C# `MessagePack` loader files and includes data-driven
/// `@idAsEnum` variants.
///
/// # Errors
///
/// Returns an error when the compiled schema cannot be mapped to C# runtime
/// code or when a Tera template fails to render.
pub fn generate_csharp_messagepack_with_id_as_enum_variants(
    schema: &CompiledSchema,
    options: &CsharpCodegenOptions,
    id_as_enum_variants: BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
    non_empty_tables: Option<&std::collections::BTreeSet<String>>,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    generate_csharp_with_id_as_enum_variants(
        schema,
        options,
        CsharpDataFormat::MessagePack,
        &MESSAGEPACK_DATABASE_TEMPLATES,
        id_as_enum_variants,
        non_empty_tables,
    )
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CsharpCodeGenerator;

/// Declares the C# code generator role implemented by this package.
///
/// # Errors
///
/// Returns an error if the package declares the code generator id more than once.
pub fn provider_bundle() -> Result<ProviderBundle, ProviderRegistrationError> {
    let mut bundle = ProviderBundle::default();
    bundle.add_codegen(CsharpCodeGenerator)?;
    Ok(bundle)
}

pub const CSHARP_CODEGEN_DESCRIPTOR: CodegenDescriptor = CodegenDescriptor {
    id: "csharp",
    display_name: "C#",
    language: "csharp",
    file_extensions: &["cs"],
    supported_data_formats: &["json", "messagepack"],
    needs_model_for_build: true,
};

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct CsharpOutputOptionsConfig {
    namespace: Option<String>,
    database_class: Option<String>,
    int_32: bool,
    float_32: bool,
}

#[derive(Debug)]
struct CsharpOutputOptions {
    codegen: CsharpCodegenOptions,
}

impl CodeGenerator for CsharpCodeGenerator {
    fn descriptor(&self) -> &'static CodegenDescriptor {
        &CSHARP_CODEGEN_DESCRIPTOR
    }

    fn decode_options(
        &self,
        options: &serde_json::Value,
    ) -> Result<DecodedOutputOptions, DiagnosticSet> {
        let raw = serde_json::from_value::<CsharpOutputOptionsConfig>(options.clone()).map_err(
            |err| {
                DiagnosticSet::one(Diagnostic::error(
                    "CSHARP-OPTIONS",
                    "CODEGEN",
                    format!("invalid C# output options: {err}"),
                ))
            },
        )?;
        let codegen = CsharpCodegenOptions::new(raw.namespace.as_deref().unwrap_or("Game.Config"))
            .with_database_class(raw.database_class.as_deref().unwrap_or("CoflowTables"))
            .with_int_32(raw.int_32)
            .with_float_32(raw.float_32);
        Ok(DecodedOutputOptions::new(
            "csharp",
            CsharpOutputOptions { codegen },
        ))
    }

    fn generate(
        &self,
        ctx: CodegenContext<'_>,
        options: &DecodedOutputOptions,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        let options = options.require::<CsharpOutputOptions>("csharp")?;
        let id_as_enum_variants = id_as_enum_variants_from_context(ctx.id_as_enum_variants)?;
        let non_empty_tables = ctx.model.map(|model| {
            model
                .tables()
                .filter(|(_, table)| !table.records.is_empty())
                .map(|(name, _)| name.to_string())
                .collect::<std::collections::BTreeSet<String>>()
        });
        let generated = match ctx.data_format {
            "json" => generate_csharp_json_with_id_as_enum_variants(
                ctx.schema,
                &options.codegen,
                id_as_enum_variants,
                non_empty_tables.as_ref(),
            ),
            "messagepack" => generate_csharp_messagepack_with_id_as_enum_variants(
                ctx.schema,
                &options.codegen,
                id_as_enum_variants,
                non_empty_tables.as_ref(),
            ),
            other => {
                return Err(DiagnosticSet::one(Diagnostic::error(
                    "CSHARP-FORMAT",
                    "CODEGEN",
                    format!("C# codegen does not support data format `{other}`"),
                )))
            }
        }
        .map_err(|err| DiagnosticSet {
            diagnostics: err
                .messages()
                .map(|message| Diagnostic::error("CODEGEN-CSHARP-001", "CODEGEN", message))
                .collect(),
        })?;
        ArtifactSet::new(
            generated
                .into_iter()
                .map(|file| ArtifactFile::text(file.relative_path, file.contents))
                .collect(),
        )
        .map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "CSHARP-ARTIFACT",
                "ARTIFACT",
                err.to_string(),
            ))
        })
    }
}

fn id_as_enum_variants_from_context(
    value: &serde_json::Value,
) -> Result<BTreeMap<String, Vec<CsharpIdAsEnumVariant>>, DiagnosticSet> {
    if value.is_null() {
        return Ok(BTreeMap::new());
    }
    serde_json::from_value(value.clone()).map_err(|err| {
        DiagnosticSet::one(Diagnostic::error(
            "CSHARP-OPTIONS",
            "CODEGEN",
            format!("invalid generated id_as_enum_variants: {err}"),
        ))
    })
}

#[cfg(test)]
mod tests;
