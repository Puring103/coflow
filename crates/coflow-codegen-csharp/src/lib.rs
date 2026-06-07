//! C# code generator for Coflow's exported JSON runtime data.
//!
//! The generated C# code does not depend on CFT at runtime. CFT is consumed only
//! by this Rust crate during generation.

mod emit;
mod ir;
mod model;
mod names;
mod render;
mod schema_view;

use coflow_cft::CftContainer;
use std::fmt;
use std::path::PathBuf;

pub use ir::CsharpCodegenOptions;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFile {
    pub relative_path: PathBuf,
    pub contents: String,
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

/// Generates C# type definitions and a Newtonsoft.Json based folder loader.
///
/// The emitted loader expects the current `coflow export json` layout: one
/// `<TypeName>.json` file per table, each containing a JSON array.
/// It targets the general-purpose `Newtonsoft.Json` package API rather than a
/// Unity-specific serialization API.
///
/// # Errors
///
/// Returns an error when the compiled schema cannot be mapped to C# runtime
/// code or when a Tera template fails to render.
pub fn generate_csharp_json(
    schema: &CftContainer,
    options: &CsharpCodegenOptions,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let project = ir::build_project(schema, options)?;
    render::render_project(&project)
}
