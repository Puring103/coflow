//! C# code generator for Coflow's exported JSON runtime data.
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

#[cfg(test)]
mod tests {
    use super::*;
    use coflow_cft::{CftContainer, ModuleId};

    fn compile_schema(source: &str) -> Result<CftContainer, String> {
        let mut container = CftContainer::new();
        container
            .add_module(ModuleId::from("main"), source)
            .map_err(|err| format!("add schema module: {err:?}"))?;
        container
            .compile()
            .map_err(|err| format!("compile schema: {err:?}"))?;
        Ok(container)
    }

    fn generated_file<'a>(files: &'a [GeneratedFile], name: &str) -> Result<&'a str, String> {
        files
            .iter()
            .find(|file| file.relative_path.as_os_str() == name)
            .map(|file| file.contents.as_str())
            .ok_or_else(|| format!("generated file `{name}`"))
    }

    fn require_contains(text: &str, needle: &str) -> Result<(), String> {
        if text.contains(needle) {
            Ok(())
        } else {
            Err(format!("expected generated output to contain `{needle}`"))
        }
    }

    #[test]
    fn codegen_handles_recursive_type_graph_when_collecting_refs() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type Target { @id id: string; }
                type Node {
                    child: Node? = null;
                    @ref(Target)
                    target_id: string;
                }
                type Table {
                    @id id: string;
                    root: Node;
                }
            ",
        )?;

        let files = generate_csharp_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(database, "ResolveNodeRefs")?;
        require_contains(database, "ResolveRef(_targetIndex")?;
        Ok(())
    }

    #[test]
    fn codegen_rejects_invalid_csharp_names() -> Result<(), String> {
        let unicode_type = compile_schema("type 示例 { value: int; }")?;
        let Err(err) =
            generate_csharp_json(&unicode_type, &CsharpCodegenOptions::new("Game.Config"))
        else {
            return Err("unicode type should fail".to_string());
        };
        require_contains(&err.to_string(), "invalid C# type name `示例`")?;

        let schema = compile_schema("type Item { value: int; }")?;
        let Err(err) = generate_csharp_json(&schema, &CsharpCodegenOptions::new("Game.1Bad"))
        else {
            return Err("namespace should fail".to_string());
        };
        require_contains(&err.to_string(), "invalid C# namespace `Game.1Bad`")?;
        Ok(())
    }

    #[test]
    fn codegen_rejects_invalid_generated_csharp_names() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type Target { @id id: string; }
                type Class {
                    @id id: string;
                    @ref(Target)
                    target_id: string;
                }
            ",
        )?;

        let Err(err) = generate_csharp_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        else {
            return Err("keyword-derived local variable should fail".to_string());
        };
        require_contains(
            &err.to_string(),
            "invalid C# table item variable name `class`",
        )?;
        Ok(())
    }

    #[test]
    fn codegen_escapes_xml_doc_summaries() -> Result<(), String> {
        let schema = compile_schema(
            r#"
                @display("A < B & C")
                type Item {
                    @display("Line 1\nLine 2")
                    value: int;
                }
            "#,
        )?;

        let files = generate_csharp_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let item = generated_file(&files, "Item.cs")?;
        require_contains(item, "/// <summary>A &lt; B &amp; C</summary>")?;
        require_contains(item, "/// <summary>Line 1 Line 2</summary>")?;
        Ok(())
    }
}
