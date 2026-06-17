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

use coflow_cft::CftContainer;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

pub use ir::{
    preflight_csharp_codegen, CsharpCodegenDiagnostic, CsharpCodegenOptions, CsharpDataFormat,
    CsharpKeyAsEnumVariant,
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
    generate_csharp_with_key_as_enum_variants(
        schema,
        options,
        data_format,
        database_templates,
        BTreeMap::new(),
    )
}

/// Generates C# files and includes data-driven enum variants for types marked
/// with `@keyAsEnum`.
///
/// # Errors
///
/// Returns an error when the compiled schema cannot be mapped to C# runtime
/// code or when a Tera template fails to render.
pub fn generate_csharp_with_key_as_enum_variants(
    schema: &CftContainer,
    options: &CsharpCodegenOptions,
    data_format: CsharpDataFormat,
    database_templates: &CsharpDatabaseTemplates,
    key_as_enum_variants: BTreeMap<String, Vec<CsharpKeyAsEnumVariant>>,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let project = ir::build_project(schema, options, data_format, key_as_enum_variants)?;
    render::render_project(&project, database_templates)
}

#[cfg(test)]
mod tests;
