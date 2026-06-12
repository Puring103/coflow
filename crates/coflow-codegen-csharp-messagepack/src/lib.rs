//! C# code generator for Coflow `MessagePack` runtime data.
//!
//! This crate owns MessagePack-specific C# loader templates and delegates
//! shared C# type/model emission to `coflow-codegen-csharp`.

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

use coflow_cft::CftContainer;
use coflow_codegen_csharp::{
    generate_csharp_with_database_templates, generate_csharp_with_key_as_enum_variants,
    CsharpDataFormat, CsharpDatabaseTemplates, CsharpTemplate,
};
pub use coflow_codegen_csharp::{CsharpCodegenError, CsharpCodegenOptions, GeneratedFile};
use std::collections::BTreeMap;

const DATABASE_TEMPLATES: CsharpDatabaseTemplates = CsharpDatabaseTemplates {
    database_template: CsharpTemplate {
        name: "database_messagepack.cs.tera",
        contents: include_str!("../templates/database_messagepack.cs.tera"),
    },
    partials: &[
        CsharpTemplate {
            name: "database_messagepack_loaders.cs.tera",
            contents: include_str!("../templates/database_messagepack_loaders.cs.tera"),
        },
        CsharpTemplate {
            name: "database_messagepack_readers.cs.tera",
            contents: include_str!("../templates/database_messagepack_readers.cs.tera"),
        },
    ],
};

/// Generates C# type definitions and a `MessagePack` based folder loader.
///
/// The emitted loader is a trusted artifact loader for `MessagePack` produced by
/// `coflow export messagepack`. It uses explicit `MessagePackReader` code so
/// the generated C# remains suitable for Unity/IL2CPP/AOT scenarios.
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
        &DATABASE_TEMPLATES,
    )
}

/// Generates C# `MessagePack` loader files and includes data-driven
/// `@KeyAsEnum` variants.
///
/// # Errors
///
/// Returns an error when the compiled schema cannot be mapped to C# runtime
/// code or when a Tera template fails to render.
pub fn generate_csharp_messagepack_with_key_as_enum_variants(
    schema: &CftContainer,
    options: &CsharpCodegenOptions,
    key_as_enum_variants: BTreeMap<String, Vec<String>>,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    generate_csharp_with_key_as_enum_variants(
        schema,
        options,
        CsharpDataFormat::MessagePack,
        &DATABASE_TEMPLATES,
        key_as_enum_variants,
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use coflow_cft::ModuleId;

    #[test]
    fn generates_messagepack_loader_from_messagepack_crate() -> Result<(), String> {
        let mut schema = CftContainer::new();
        schema
            .add_module(
                ModuleId::from("main"),
                "type Item { @id id: string; value: int; }",
            )
            .map_err(|err| format!("{err:?}"))?;
        schema.compile().map_err(|err| format!("{err:?}"))?;

        let files = generate_csharp_messagepack(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let database = files
            .iter()
            .find(|file| file.relative_path.as_os_str() == "GameConfig.cs")
            .ok_or_else(|| "generated GameConfig.cs".to_string())?
            .contents
            .as_str();

        if !database.contains("using MessagePack;") {
            return Err("expected MessagePack loader".to_string());
        }
        if database.contains("using Newtonsoft.Json;") {
            return Err("did not expect Newtonsoft.Json loader".to_string());
        }
        Ok(())
    }
}
