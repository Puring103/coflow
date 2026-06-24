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
mod model;
mod names;
mod render;
mod schema_view;

use coflow_api::{
    ArtifactFile, ArtifactSet, CodeGenerator, CodegenContext, CodegenDescriptor, Diagnostic,
    DiagnosticSet, OutputSpec,
};
use coflow_cft::CftContainer;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

pub use ir::{
    preflight_csharp_codegen, CsharpCodegenDiagnostic, CsharpCodegenOptions, CsharpDataFormat,
    CsharpIdAsEnumVariant,
};

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
    message: String,
}

impl CsharpCodegenError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CsharpCodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
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
    schema: &CftContainer,
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
    schema: &CftContainer,
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
    schema: &CftContainer,
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
    schema: &CftContainer,
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
    schema: &CftContainer,
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
    schema: &CftContainer,
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
    schema: &CftContainer,
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

pub const CSHARP_CODEGEN_DESCRIPTOR: CodegenDescriptor = CodegenDescriptor {
    id: "csharp",
    display_name: "C#",
    language: "csharp",
    file_extensions: &["cs"],
    supported_data_formats: &["json", "messagepack"],
    needs_model_for_build: true,
};

impl CodeGenerator for CsharpCodeGenerator {
    fn descriptor(&self) -> &'static CodegenDescriptor {
        &CSHARP_CODEGEN_DESCRIPTOR
    }

    fn preflight(&self, ctx: CodegenContext<'_>, output: &OutputSpec) -> DiagnosticSet {
        let namespace = output
            .options
            .get("namespace")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Game.Config");
        let int_32 = output
            .options
            .get("int_32")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let float_32 = output
            .options
            .get("float_32")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let options = CsharpCodegenOptions::new(namespace)
            .with_int_32(int_32)
            .with_float_32(float_32);
        DiagnosticSet {
            diagnostics: preflight_csharp_codegen(ctx.schema, &options, &BTreeMap::new())
                .into_iter()
                .map(|diagnostic| {
                    Diagnostic::error(diagnostic.code, diagnostic.stage, diagnostic.message)
                })
                .collect(),
        }
    }

    fn generate(
        &self,
        ctx: CodegenContext<'_>,
        output: &OutputSpec,
    ) -> Result<ArtifactSet, DiagnosticSet> {
        let namespace = output
            .options
            .get("namespace")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Game.Config");
        let int_32 = output
            .options
            .get("int_32")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let float_32 = output
            .options
            .get("float_32")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let options = CsharpCodegenOptions::new(namespace)
            .with_int_32(int_32)
            .with_float_32(float_32);
        let id_as_enum_variants = id_as_enum_variants_from_options(&output.options)?;
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
                &options,
                id_as_enum_variants,
                non_empty_tables.as_ref(),
            ),
            "messagepack" => generate_csharp_messagepack_with_id_as_enum_variants(
                ctx.schema,
                &options,
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
        .map_err(|err| {
            DiagnosticSet::one(Diagnostic::error(
                "CSHARP-CODEGEN",
                "CODEGEN",
                err.to_string(),
            ))
        })?;
        Ok(ArtifactSet::new(
            generated
                .into_iter()
                .map(|file| ArtifactFile::text(file.relative_path, file.contents))
                .collect(),
        ))
    }
}

fn id_as_enum_variants_from_options(
    options: &serde_json::Value,
) -> Result<BTreeMap<String, Vec<CsharpIdAsEnumVariant>>, DiagnosticSet> {
    let Some(value) = options.get("id_as_enum_variants") else {
        return Ok(BTreeMap::new());
    };
    serde_json::from_value(value.clone()).map_err(|err| {
        DiagnosticSet::one(Diagnostic::error(
            "CSHARP-OPTIONS",
            "CODEGEN",
            format!("invalid id_as_enum_variants option: {err}"),
        ))
    })
}

#[cfg(test)]
mod tests;
