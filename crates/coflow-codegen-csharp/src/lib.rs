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
    let project = ir::build_project(schema, options)?;
    render::render_project(&project)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::needless_raw_string_hashes,
        clippy::panic,
        clippy::panic_in_result_fn,
        clippy::too_many_lines,
        clippy::unwrap_used
    )]

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

    fn require_not_contains(contents: &str, needle: &str) -> Result<(), String> {
        if contents.contains(needle) {
            Err(format!(
                "expected generated output not to contain `{needle}`"
            ))
        } else {
            Ok(())
        }
    }

    #[test]
    fn codegen_does_not_emit_struct_property_initializers() -> Result<(), String> {
        let schema = compile_schema(
            r#"
            @struct
            sealed type StatBlock {
                speed: float = 1.0;
                crit: int = 5;
            }

            type Item {
                @id id: string;
                stats: StatBlock;
            }
        "#,
        )?;

        let files = generate_csharp_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let stat_block = generated_file(&files, "StatBlock.cs")?;
        require_contains(stat_block, "public partial struct StatBlock")?;
        require_not_contains(stat_block, "= 1.0f;")?;
        require_not_contains(stat_block, "= 5;")?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(database, "Speed = ReadWithDefault")?;
        require_contains(database, "Crit = ReadWithDefault")?;
        Ok(())
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
        require_contains(database, "ResolveRef(targetRefIndex")?;
        Ok(())
    }

    #[test]
    fn codegen_builds_polymorphic_ref_indexes_for_abstract_targets() -> Result<(), String> {
        let schema = compile_schema(
            r"
                abstract type Reward { @id id: string; }
                type ItemReward : Reward { count: int; }
                type CurrencyReward : Reward { amount: int; }
                type Drop {
                    @id id: string;
                    @ref(Reward)
                    reward_id: string;
                }
            ",
        )?;

        let files = generate_csharp_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(database, "Dictionary<string, Reward> _rewardRefIndex")?;
        require_contains(database, "var rewardRefIndex = BuildRefIndex(")?;
        require_contains(database, "new RefIndexSource<string, Reward>(itemRewards")?;
        require_contains(
            database,
            "new RefIndexSource<string, Reward>(currencyRewards",
        )?;
        require_contains(database, "ResolveRef(rewardRefIndex")?;
        Ok(())
    }

    #[test]
    fn codegen_preserves_missing_field_default_and_nullable_required_semantics(
    ) -> Result<(), String> {
        let schema = compile_schema(
            r#"
                type Item {
                    @id id: string;
                    name: string = "unknown";
                    maybe: int?;
                    tags: [string] = [];
                }
            "#,
        )?;

        let files = generate_csharp_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let database = generated_file(&files, "GameConfig.cs")?;
        let item = generated_file(&files, "Item.cs")?;
        require_contains(item, "public IReadOnlyList<string> Tags { get; init; }")?;
        require_contains(
            database,
            "Name = ReadWithDefault(obj, \"name\", path, \"unknown\"",
        )?;
        require_contains(
            database,
            "Maybe = ReadRequiredNullable(obj, \"maybe\", path",
        )?;
        require_contains(
            database,
            "Tags = ReadWithDefault(obj, \"tags\", path, new List<string>()",
        )?;
        Ok(())
    }

    #[test]
    fn codegen_struct_inheritance_emits_inherited_fields() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type Parent { base_value: int; }
                @struct
                sealed type Child : Parent { child_value: int; }
            ",
        )?;

        let files = generate_csharp_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let child = generated_file(&files, "Child.cs")?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(child, "public long BaseValue { get; init; }")?;
        require_contains(child, "public long ChildValue { get; init; }")?;
        require_contains(
            database,
            "BaseValue = ReadRequired(obj, \"base_value\", path",
        )?;
        require_contains(
            database,
            "ChildValue = ReadRequired(obj, \"child_value\", path",
        )?;
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

    #[test]
    fn codegen_emits_enum_variant_annotations() -> Result<(), String> {
        let schema = compile_schema(
            r#"
                enum Rarity {
                    @display("Common display")
                    Common,
                    @deprecated
                    Old,
                }
            "#,
        )?;

        let files = generate_csharp_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let rarity = generated_file(&files, "Rarity.cs")?;
        require_contains(rarity, "/// <summary>Common display</summary>")?;
        require_contains(rarity, "[Obsolete]")?;
        Ok(())
    }
}
