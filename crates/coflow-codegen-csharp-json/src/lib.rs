//! C# code generator for Coflow JSON runtime data.
//!
//! This crate owns JSON-specific C# loader templates and delegates shared C#
//! type/model emission to `coflow-codegen-csharp`.

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
pub use coflow_codegen_csharp::{
    CsharpCodegenError, CsharpCodegenOptions, CsharpKeyAsEnumVariant, GeneratedFile,
};
use std::collections::BTreeMap;

const DATABASE_TEMPLATES: CsharpDatabaseTemplates = CsharpDatabaseTemplates {
    database_template: CsharpTemplate {
        name: "database_json.cs.tera",
        contents: include_str!("../templates/database_json.cs.tera"),
    },
    partials: &[
        CsharpTemplate {
            name: "database_json_loaders.cs.tera",
            contents: include_str!("../templates/database_json_loaders.cs.tera"),
        },
        CsharpTemplate {
            name: "database_json_readers.cs.tera",
            contents: include_str!("../templates/database_json_readers.cs.tera"),
        },
    ],
};

/// Generates C# type definitions and a Newtonsoft.Json based folder loader.
///
/// The emitted loader is a trusted artifact loader for JSON produced by
/// `coflow export json`; it is not a validator for arbitrary JSON.
/// It expects one `<TypeName>.json` file per table, each containing a JSON array.
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
    generate_csharp_with_database_templates(
        schema,
        options,
        CsharpDataFormat::Json,
        &DATABASE_TEMPLATES,
    )
}

/// Generates C# JSON loader files and includes data-driven `@IdAsEnum`
/// variants.
///
/// # Errors
///
/// Returns an error when the compiled schema cannot be mapped to C# runtime
/// code or when a Tera template fails to render.
pub fn generate_csharp_json_with_key_as_enum_variants(
    schema: &CftContainer,
    options: &CsharpCodegenOptions,
    key_as_enum_variants: BTreeMap<String, Vec<CsharpKeyAsEnumVariant>>,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    generate_csharp_with_key_as_enum_variants(
        schema,
        options,
        CsharpDataFormat::Json,
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
    fn generates_json_loader_from_json_crate() -> Result<(), String> {
        let mut schema = CftContainer::new();
        schema
            .add_module(
                ModuleId::from("main"),
                "type Item { @id id: string; value: int; }",
            )
            .map_err(|err| format!("{err:?}"))?;
        schema.compile().map_err(|err| format!("{err:?}"))?;

        let files = generate_csharp_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let database = files
            .iter()
            .find(|file| file.relative_path.as_os_str() == "GameConfig.cs")
            .ok_or_else(|| "generated GameConfig.cs".to_string())?
            .contents
            .as_str();

        if !database.contains("using Newtonsoft.Json;") {
            return Err("expected Newtonsoft.Json loader".to_string());
        }
        if database.contains("using MessagePack;") {
            return Err("did not expect MessagePack loader".to_string());
        }
        Ok(())
    }
}
