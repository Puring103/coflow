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

/// Generates C# files and includes data-driven enum variants for fields marked
/// with `@IdAsEnum`.
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
    use std::collections::BTreeMap;

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

    fn json_database_templates() -> CsharpDatabaseTemplates {
        CsharpDatabaseTemplates {
            database_template: CsharpTemplate {
                name: "database_json.cs.tera",
                contents: include_str!(
                    "../../coflow-codegen-csharp-json/templates/database_json.cs.tera"
                ),
            },
            partials: &[
                CsharpTemplate {
                    name: "database_json_loaders.cs.tera",
                    contents: include_str!(
                        "../../coflow-codegen-csharp-json/templates/database_json_loaders.cs.tera"
                    ),
                },
                CsharpTemplate {
                    name: "database_json_readers.cs.tera",
                    contents: include_str!(
                        "../../coflow-codegen-csharp-json/templates/database_json_readers.cs.tera"
                    ),
                },
            ],
        }
    }

    fn messagepack_database_templates() -> CsharpDatabaseTemplates {
        CsharpDatabaseTemplates {
            database_template: CsharpTemplate {
                name: "database_messagepack.cs.tera",
                contents: include_str!(
                    "../../coflow-codegen-csharp-messagepack/templates/database_messagepack.cs.tera"
                ),
            },
            partials: &[
                CsharpTemplate {
                    name: "database_messagepack_loaders.cs.tera",
                    contents: include_str!(
                        "../../coflow-codegen-csharp-messagepack/templates/database_messagepack_loaders.cs.tera"
                    ),
                },
                CsharpTemplate {
                    name: "database_messagepack_readers.cs.tera",
                    contents: include_str!(
                        "../../coflow-codegen-csharp-messagepack/templates/database_messagepack_readers.cs.tera"
                    ),
                },
            ],
        }
    }

    fn generate_json(
        schema: &CftContainer,
        options: &CsharpCodegenOptions,
    ) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
        generate_csharp(
            schema,
            options,
            CsharpDataFormat::Json,
            &json_database_templates(),
        )
    }

    fn generate_messagepack(
        schema: &CftContainer,
        options: &CsharpCodegenOptions,
    ) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
        generate_csharp(
            schema,
            options,
            CsharpDataFormat::MessagePack,
            &messagepack_database_templates(),
        )
    }

    fn generate_json_with_key_as_enum_variants(
        schema: &CftContainer,
        options: &CsharpCodegenOptions,
        variants: BTreeMap<String, Vec<CsharpKeyAsEnumVariant>>,
    ) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
        generate_csharp_with_key_as_enum_variants(
            schema,
            options,
            CsharpDataFormat::Json,
            &json_database_templates(),
            variants,
        )
    }

    #[test]
    fn data_format_serializes_messagepack_without_separator() -> Result<(), String> {
        let value = serde::Serialize::serialize(&CsharpDataFormat::MessagePack, StringSerializer)
            .map_err(|err| err.to_string())?;
        assert_eq!(value, "messagepack");
        Ok(())
    }

    #[test]
    fn codegen_messagepack_uses_msgpack_loader_template() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type Item {
                    @id id: string;
                    value: int;
                }
            ",
        )?;

        let files = generate_messagepack(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(database, "using MessagePack;")?;
        require_contains(database, "Path.Combine(dataDir, \"Item.msgpack\")")?;
        require_not_contains(database, "Newtonsoft.Json")?;
        Ok(())
    }

    #[test]
    fn codegen_key_as_enum_generates_enum_and_strongly_typed_id_and_ref() -> Result<(), String> {
        let schema = compile_schema(
            r#"
                type GeneConfig {
                    @IdAsEnum("GeneId")
                    @id
                    id: string;
                }

                type BioRemainsConfig {
                    @id
                    id: string;
                    @ref(GeneConfig)
                    gene_id: string?;
                    @ref(GeneConfig)
                    fallback_gene_id: string = "Gene_Mating";
                }
            "#,
        )?;
        let mut variants = BTreeMap::new();
        variants.insert(
            "GeneId".to_string(),
            vec![
                CsharpKeyAsEnumVariant {
                    name: "Gene_Spore".to_string(),
                    value: 0,
                },
                CsharpKeyAsEnumVariant {
                    name: "Gene_Mating".to_string(),
                    value: 1,
                },
            ],
        );

        let files = generate_json_with_key_as_enum_variants(
            &schema,
            &CsharpCodegenOptions::new("Game.Config"),
            variants,
        )
        .map_err(|err| err.to_string())?;

        let gene_id = generated_file(&files, "GeneId.cs")?;
        require_contains(gene_id, "public enum GeneId")?;
        require_contains(gene_id, "Gene_Spore = 0")?;
        require_contains(gene_id, "Gene_Mating = 1")?;

        let gene = generated_file(&files, "GeneConfig.cs")?;
        require_contains(gene, "public GeneId Id { get; set; }")?;
        require_not_contains(gene, "public string Id")?;
        require_not_contains(gene, " = \"\";")?;

        let remains = generated_file(&files, "BioRemainsConfig.cs")?;
        require_contains(remains, "public GeneId? GeneId { get; set; }")?;
        require_contains(
            remains,
            "public GeneId FallbackGeneId { get; set; } = (GeneId)Enum.Parse(typeof(GeneId), \"Gene_Mating\");",
        )?;
        require_contains(remains, "public GeneConfig? Gene { get; internal set; }")?;
        require_contains(
            remains,
            "public GeneConfig FallbackGene { get; internal set; } = null!;",
        )?;

        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(database, "GeneId = ReadRequiredNullable")?;
        require_contains(
            database,
            "FallbackGeneId = ReadWithDefault(obj, \"fallback_gene_id\", path, (GeneId)Enum.Parse(typeof(GeneId), \"Gene_Mating\")",
        )?;
        require_contains(database, "ReadStringEnum<GeneId>")?;
        require_contains(
            database,
            "Dictionary<GeneId, GeneConfig> _geneConfigRefIndex",
        )?;
        require_contains(
            database,
            "ResolveRef(geneConfigRefIndex, value.GeneId.Value",
        )?;
        Ok(())
    }

    #[test]
    fn codegen_emits_unity_compatible_csharp_syntax() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type Target { @id id: string; }
                type Item {
                    @id id: string;
                    @ref(Target)
                    target_id: string;
                }
            ",
        )?;

        let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let item = generated_file(&files, "Item.cs")?;
        let target = generated_file(&files, "Target.cs")?;
        let database = generated_file(&files, "GameConfig.cs")?;
        let load_exception = generated_file(&files, "CftLoadException.cs")?;

        for output in [item, target, database, load_exception] {
            let normalized = output.replace("\r\n", "\n");
            require_contains(&normalized, "namespace Game.Config\n{")?;
            require_not_contains(output, "namespace Game.Config;")?;
            require_not_contains(output, "get; init;")?;
            require_not_contains(output, "record struct")?;
        }

        Ok(())
    }

    #[test]
    fn codegen_key_as_enum_generates_strongly_typed_string_field() -> Result<(), String> {
        let schema = compile_schema(
            r#"
                type AttributeConfig {
                    @id
                    @IdAsEnum("CreatureAttribute")
                    id: string;
                }

                sealed type ModifyValueOperation {
                    @GenAsEnum("CreatureAttribute")
                    attribute: string;
                    value: float;
                }
            "#,
        )?;
        let mut variants = BTreeMap::new();
        variants.insert(
            "CreatureAttribute".to_string(),
            vec![
                CsharpKeyAsEnumVariant {
                    name: "Body_Hp".to_string(),
                    value: 0,
                },
                CsharpKeyAsEnumVariant {
                    name: "Energy_Limit".to_string(),
                    value: 1,
                },
            ],
        );

        let files = generate_json_with_key_as_enum_variants(
            &schema,
            &CsharpCodegenOptions::new("Game.Config"),
            variants,
        )
        .map_err(|err| err.to_string())?;

        assert_eq!(
            files
                .iter()
                .filter(|file| file.relative_path.ends_with("CreatureAttribute.cs"))
                .count(),
            1
        );
        let modifier = generated_file(&files, "ModifyValueOperation.cs")?;
        require_contains(modifier, "public CreatureAttribute Attribute { get; set; }")?;
        require_not_contains(modifier, "public string Attribute")?;

        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(database, "ReadStringEnum<CreatureAttribute>")?;
        Ok(())
    }

    #[test]
    fn codegen_messagepack_emits_explicit_readers_type_dispatch_and_ref_resolution(
    ) -> Result<(), String> {
        let schema = compile_schema(
            r"
                type Item { @id id: string; }
                abstract type Reward { id: string; }
                type ItemReward : Reward {
                    @ref(Item)
                    item_id: string;
                    count: int = 1;
                    maybe_count: int?;
                }
                type DropTable {
                    @id id: string;
                    rewards: [Reward];
                }
            ",
        )?;

        let files = generate_messagepack(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(database, "using MessagePack;")?;
        require_contains(database, "private delegate T MessagePackRowLoader<T>(")?;
        require_contains(
            database,
            "private static Item LoadItem(ref MessagePackReader reader, string path)",
        )?;
        require_contains(
            database,
            "private static int ReadMapHeader(ref MessagePackReader reader, string path)",
        )?;
        require_contains(
            database,
            "private static int ReadArrayHeader(ref MessagePackReader reader, string path)",
        )?;
        require_contains(database, "var count = ReadMapHeader(ref reader, path);")?;
        require_contains(
            database,
            "int count = ReadArrayHeader(ref reader, tableName);",
        )?;
        require_contains(database, "var count = ReadArrayHeader(ref reader, path);")?;
        require_contains(database, "var count = ReadMapHeader(ref reader, path);")?;
        require_not_contains(database, "var count = reader.ReadMapHeader();")?;
        require_not_contains(database, "var count = reader.ReadArrayHeader();")?;
        require_not_contains(database, "count = reader.ReadArrayHeader();")?;
        require_contains(database, "var key = ReadString(ref reader, path);")?;
        require_contains(database, "var typeKey = ReadString(ref reader, path);")?;
        require_contains(database, "var rawKey = ReadString(ref reader, path);")?;
        require_not_contains(database, "var key = reader.ReadString();")?;
        require_not_contains(database, "var typeKey = reader.ReadString();")?;
        require_not_contains(database, "var rawKey = reader.ReadString();")?;
        require_contains(database, "case \"item_id\":")?;
        require_contains(database, "SkipValue(ref reader, fieldPath)")?;
        require_not_contains(database, "default:\n                    reader.Skip();")?;
        require_contains(database, "if (result.ContainsKey(key))")?;
        require_contains(database, "var value = readValue(ref reader, keyPath);")?;
        require_contains(database, "result.Add(key, value);")?;
        require_not_contains(database, "TryAdd(key, readValue(")?;
        require_contains(database, "if (!reader.End)")?;
        require_contains(database, "\"single MessagePack array\"")?;
        require_contains(database, "\"trailing data\"")?;
        require_contains(database, "ReadNil(ref reader, fieldPath) ? null :")?;
        require_not_contains(database, ".TryReadNil() ? null")?;
        require_contains(
            database,
            "private static bool ReadNil(ref MessagePackReader reader, string path)",
        )?;
        require_contains(database, "catch (EndOfStreamException ex)")?;
        require_contains(database, "if (ReadNil(ref reader, path))")?;
        if !database.contains("LoadRewardPolymorphic(ref reader, path)")
            && !database.contains("LoadRewardPolymorphic(ref itemReader, itemPath)")
        {
            return Err(
                "expected generated output to contain polymorphic Reward MessagePack loading"
                    .to_string(),
            );
        }
        require_contains(database, "ResolveRef(itemRefIndex")?;
        require_not_contains(database, "Newtonsoft.Json")?;
        require_not_contains(database, "JToken")?;
        require_not_contains(database, "JObject")?;
        require_not_contains(database, "JArray")?;
        Ok(())
    }

    #[test]
    fn codegen_messagepack_requires_fields_with_unrepresentable_schema_defaults(
    ) -> Result<(), String> {
        let project = model::CsharpProject {
            namespace: "Game.Config".to_string(),
            database_class: "GameConfig".to_string(),
            enums: Vec::new(),
            types: Vec::new(),
            database: model::CsharpDatabase {
                tables: Vec::new(),
                ref_indexes: Vec::new(),
                indexes: Vec::new(),
                constructor_parameters: Vec::new(),
                load_steps: Vec::new(),
                constructor_args: Vec::new(),
                loaders: vec![model::CsharpLoader {
                    type_name: "Item".to_string(),
                    fields: vec![
                        model::CsharpLoadField {
                            property: "Items".to_string(),
                            source_name: "items".to_string(),
                            local_name: "items".to_string(),
                            type_name: "List<long>".to_string(),
                            read_expr: String::new(),
                            messagepack_read_expr: "ReadArray(ref reader, fieldPath, static (ref MessagePackReader itemReader, string itemPath) => ReadInt(ref itemReader, itemPath))".to_string(),
                            default_expr: None,
                            is_required: true,
                        },
                        model::CsharpLoadField {
                            property: "Count".to_string(),
                            source_name: "count".to_string(),
                            local_name: "count".to_string(),
                            type_name: "long".to_string(),
                            read_expr: String::new(),
                            messagepack_read_expr: "ReadInt(ref reader, fieldPath)".to_string(),
                            default_expr: Some("1".to_string()),
                            is_required: false,
                        },
                    ],
                }],
                polymorphic_loaders: Vec::new(),
                resolve: None,
            },
        };

        let files = render::render_project(&project, &messagepack_database_templates())
            .map_err(|err| err.to_string())?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(database, "List<long> items = default!;")?;
        require_contains(database, "if (!hasItems)")?;
        require_contains(database, "missing required field `items`")?;
        require_contains(database, "long count = 1;")?;
        require_not_contains(database, "if (!hasCount)")?;
        Ok(())
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

        let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let stat_block = generated_file(&files, "StatBlock.cs")?;
        require_contains(stat_block, "public partial struct StatBlock")?;
        require_not_contains(stat_block, "= 1.0f;")?;
        require_not_contains(stat_block, "= 5;")?;
        let database = generated_file(&files, "GameConfig.cs")?;
        let item = generated_file(&files, "Item.cs")?;
        require_contains(database, "Speed = ReadWithDefault")?;
        require_contains(database, "Crit = ReadWithDefault")?;
        require_contains(item, "public StatBlock Stats { get; set; }")?;
        require_not_contains(item, "public StatBlock Stats { get; set; } = null!;")?;
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

        let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
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

        let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
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
                enum Element {
                    Physical = 0,
                    Fire = 1,
                }

                type Item {
                    @id id: string;
                    name: string = "unknown";
                    maybe: int?;
                    element: Element? = null;
                    tags: [string] = [];
                }
            "#,
        )?;

        let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let database = generated_file(&files, "GameConfig.cs")?;
        let item = generated_file(&files, "Item.cs")?;
        require_contains(item, "public IReadOnlyList<string> Tags { get; set; }")?;
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
            "Element = ReadNullableWithDefault(obj, \"element\", path, (Element?)null",
        )?;
        require_contains(
            database,
            "Tags = ReadWithDefault(obj, \"tags\", path, new List<string>()",
        )?;
        Ok(())
    }

    #[test]
    fn codegen_renames_keyword_field_loader_locals() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type Item {
                    @id id: string;
                    params: int;
                }
            ",
        )?;

        let files = generate_messagepack(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(database, "long paramsValue = default!;")?;
        require_contains(database, "paramsValue = ReadInt(ref reader, fieldPath);")?;
        require_contains(database, "Params = paramsValue,")?;
        require_not_contains(database, "long params =")?;
        Ok(())
    }

    #[test]
    fn codegen_messagepack_renames_reserved_field_loader_locals() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type Item {
                    @id id: string;
                    has_id: string;
                    count: int;
                    count_value: int;
                    params: int;
                    params_value: int;
                    key: string;
                    key_value: string;
                    reader: string;
                    path: string;
                    field_path: string;
                }
            ",
        )?;

        let files = generate_messagepack(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(database, "var hasId = false;")?;
        require_contains(database, "string hasId2 = default!;")?;
        require_contains(database, "long countValue = default!;")?;
        require_contains(database, "long countValue2 = default!;")?;
        require_contains(database, "long paramsValue = default!;")?;
        require_contains(database, "long paramsValue2 = default!;")?;
        require_contains(database, "string keyValue = default!;")?;
        require_contains(database, "string keyValue2 = default!;")?;
        require_contains(database, "string readerValue = default!;")?;
        require_contains(database, "string pathValue = default!;")?;
        require_contains(database, "string fieldPathValue = default!;")?;
        require_contains(database, "countValue = ReadInt(ref reader, fieldPath);")?;
        require_contains(database, "countValue2 = ReadInt(ref reader, fieldPath);")?;
        require_contains(database, "paramsValue = ReadInt(ref reader, fieldPath);")?;
        require_contains(database, "paramsValue2 = ReadInt(ref reader, fieldPath);")?;
        require_contains(database, "hasId2 = ReadString(ref reader, fieldPath);")?;
        require_contains(database, "keyValue = ReadString(ref reader, fieldPath);")?;
        require_contains(database, "keyValue2 = ReadString(ref reader, fieldPath);")?;
        require_contains(database, "readerValue = ReadString(ref reader, fieldPath);")?;
        require_contains(database, "pathValue = ReadString(ref reader, fieldPath);")?;
        require_contains(
            database,
            "fieldPathValue = ReadString(ref reader, fieldPath);",
        )?;
        require_contains(database, "Count = countValue,")?;
        require_contains(database, "CountValue = countValue2,")?;
        require_contains(database, "Params = paramsValue,")?;
        require_contains(database, "ParamsValue = paramsValue2,")?;
        require_contains(database, "HasId = hasId2,")?;
        require_contains(database, "Key = keyValue,")?;
        require_contains(database, "KeyValue = keyValue2,")?;
        require_contains(database, "Reader = readerValue,")?;
        require_contains(database, "Path = pathValue,")?;
        require_contains(database, "FieldPath = fieldPathValue,")?;
        require_not_contains(database, "long count =")?;
        require_not_contains(database, "long params =")?;
        require_not_contains(database, "string key =")?;
        require_not_contains(database, "string reader =")?;
        require_not_contains(database, "string path =")?;
        require_not_contains(database, "string fieldPath =")?;
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

        let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let child = generated_file(&files, "Child.cs")?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(child, "public long BaseValue { get; set; }")?;
        require_contains(child, "public long ChildValue { get; set; }")?;
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
    fn codegen_rejects_generated_member_name_collisions() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type Item {
                    @id id: string;
                    foo_bar: int;
                    fooBar: int;
                }
            ",
        )?;

        let Err(err) = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config")) else {
            return Err("generated member collision should fail".to_string());
        };
        require_contains(
            &err.to_string(),
            "generated C# member name `FooBar` collides",
        )?;
        Ok(())
    }

    #[test]
    fn codegen_preflight_collects_multiple_naming_diagnostics() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type FooBar { value: int; }
                type Foo_Bar {
                    foo_bar: int;
                    fooBar: int;
                }
            ",
        )?;

        let diagnostics = preflight_csharp_codegen(
            &schema,
            &CsharpCodegenOptions::new("Game.1Bad"),
            &BTreeMap::new(),
        );

        let messages = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_str())
            .collect::<Vec<_>>();
        assert!(
            messages
                .iter()
                .any(|message| message.contains("invalid C# namespace `Game.1Bad`")),
            "messages: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|message| { message.contains("generated C# file name `FooBar.cs` collides") }),
            "messages: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|message| { message.contains("generated C# member name `FooBar` collides") }),
            "messages: {messages:?}"
        );
        assert!(
            diagnostics.len() >= 3,
            "expected at least three diagnostics, got {messages:?}"
        );
        Ok(())
    }

    #[test]
    fn codegen_rejects_generated_file_name_collisions_and_reserved_names() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type GameConfig { value: int; }
            ",
        )?;
        let Err(err) = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config")) else {
            return Err("reserved generated file should fail".to_string());
        };
        require_contains(
            &err.to_string(),
            "generated C# file name `GameConfig.cs` is reserved",
        )?;

        let schema = compile_schema(
            r"
                type CftLoadException { value: int; }
            ",
        )?;
        let Err(err) = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config")) else {
            return Err("reserved exception file should fail".to_string());
        };
        require_contains(
            &err.to_string(),
            "generated C# file name `CftLoadException.cs` is reserved",
        )?;

        let schema = compile_schema(
            r"
                type GameConfig { value: int; }
            ",
        )?;
        let Err(err) = generate_json(
            &schema,
            &CsharpCodegenOptions::new("Game.Config").with_database_class("RuntimeConfig"),
        ) else {
            return Err(
                "GameConfig should remain reserved even with custom database class".to_string(),
            );
        };
        require_contains(
            &err.to_string(),
            "generated C# file name `GameConfig.cs` is reserved",
        )?;

        let schema = compile_schema(
            r"
                type FooBar { value: int; }
                type Foo_Bar { value: int; }
            ",
        )?;
        let Err(err) = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config")) else {
            return Err("generated file collision should fail".to_string());
        };
        require_contains(
            &err.to_string(),
            "generated C# file name `FooBar.cs` collides",
        )?;

        let schema = compile_schema(
            r"
                type Item { value: int; }
                type ITEM { value: int; }
            ",
        )?;
        let Err(err) = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config")) else {
            return Err("case-insensitive generated file collision should fail".to_string());
        };
        require_contains(
            &err.to_string(),
            "generated C# file name `Item.cs` collides",
        )?;

        let schema = compile_schema(
            r"
                type runtime_config { value: int; }
            ",
        )?;
        let Err(err) = generate_json(
            &schema,
            &CsharpCodegenOptions::new("Game.Config").with_database_class("RuntimeConfig"),
        ) else {
            return Err("case-insensitive reserved generated file should fail".to_string());
        };
        require_contains(
            &err.to_string(),
            "generated C# file name `RuntimeConfig.cs` is reserved",
        )?;
        Ok(())
    }

    #[test]
    fn codegen_uses_converted_type_and_enum_names_in_outputs() -> Result<(), String> {
        let schema = compile_schema(
            r"
                enum item_rarity { common, rare }
                type item_data {
                    @id item_id: string;
                    rarity: item_rarity;
                }
                type loot_table {
                    @id table_id: string;
                    @ref(item_data)
                    item_id: string;
                }
                abstract type reward_base { @id reward_id: string; }
                type item_reward : reward_base {
                    @ref(item_data)
                    item_id: string;
                }
                type reward_holder {
                    @id holder_id: string;
                    reward: reward_base;
                }
            ",
        )?;

        let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let item = generated_file(&files, "ItemData.cs")?;
        let rarity = generated_file(&files, "ItemRarity.cs")?;
        let loot_table = generated_file(&files, "LootTable.cs")?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(rarity, "public enum ItemRarity")?;
        require_contains(item, "public partial class ItemData")?;
        require_contains(item, "public ItemRarity Rarity { get; set; }")?;
        require_contains(loot_table, "public ItemData Item { get; internal set; }")?;
        require_contains(database, "public IReadOnlyList<ItemData> ItemDatas")?;
        require_contains(
            database,
            "private readonly Dictionary<string, ItemData> _itemDataIndex;",
        )?;
        require_contains(
            database,
            "LoadTable(Path.Combine(dataDir, \"item_data.json\"), \"item_data\", LoadItemData)",
        )?;
        require_contains(database, "var itemDataRefIndex = itemDataIndex;")?;
        require_not_contains(database, "var itemDataRefIndex = item_dataIndex;")?;
        require_contains(database, "private static ItemData LoadItemData(")?;
        require_contains(database, "\"item_reward\" => LoadItemReward(token, path)")?;
        require_contains(
            database,
            "private static RewardBase LoadRewardBasePolymorphic(",
        )?;
        require_not_contains(database, "Loaditem_data")?;
        Ok(())
    }

    #[test]
    fn codegen_rejects_generated_enum_variant_name_collisions() -> Result<(), String> {
        let schema = compile_schema(
            r"
                enum Rarity {
                    common_item,
                    commonItem,
                }
            ",
        )?;

        let Err(err) = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config")) else {
            return Err("generated enum variant collision should fail".to_string());
        };
        require_contains(
            &err.to_string(),
            "generated C# enum variant name `CommonItem` collides",
        )?;
        Ok(())
    }

    #[test]
    fn codegen_rejects_invalid_converted_generated_names() -> Result<(), String> {
        let schema = compile_schema("type __ { value: int; }")?;
        let Err(err) = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config")) else {
            return Err("empty converted type name should fail".to_string());
        };
        require_contains(&err.to_string(), "invalid C# type name ``")?;

        let schema = compile_schema("enum __ { Value }")?;
        let Err(err) = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config")) else {
            return Err("empty converted enum name should fail".to_string());
        };
        require_contains(&err.to_string(), "invalid C# enum name ``")?;

        let schema = compile_schema("enum Rarity { __ }")?;
        let Err(err) = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config")) else {
            return Err("empty converted enum variant name should fail".to_string());
        };
        require_contains(&err.to_string(), "invalid C# enum variant name ``")?;
        Ok(())
    }

    #[test]
    fn codegen_rejects_configured_database_file_name_collisions() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type RuntimeConfig { value: int; }
            ",
        )?;

        let Err(err) = generate_json(
            &schema,
            &CsharpCodegenOptions::new("Game.Config").with_database_class("RuntimeConfig"),
        ) else {
            return Err("database file collision should fail".to_string());
        };
        require_contains(
            &err.to_string(),
            "generated C# file name `RuntimeConfig.cs` is reserved",
        )?;

        let schema = compile_schema(
            r"
                type Item { value: int; }
            ",
        )?;
        let Err(err) = generate_json(
            &schema,
            &CsharpCodegenOptions::new("Game.Config").with_database_class("CftLoadException"),
        ) else {
            return Err("database exception file collision should fail".to_string());
        };
        require_contains(
            &err.to_string(),
            "generated C# database file `CftLoadException.cs` collides",
        )?;

        let schema = compile_schema(
            r"
                type Item { value: int; }
            ",
        )?;
        let Err(err) = generate_json(
            &schema,
            &CsharpCodegenOptions::new("Game.Config").with_database_class("cftloadexception"),
        ) else {
            return Err(
                "case-insensitive database exception file collision should fail".to_string(),
            );
        };
        require_contains(
            &err.to_string(),
            "generated C# database file `CftLoadException.cs` collides",
        )?;
        Ok(())
    }

    #[test]
    fn codegen_maps_float_to_double_everywhere() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type Item {
                    @id id: string;
                    scalar: float = 1.0;
                    amounts: [float] = [];
                }
            ",
        )?;

        let json_files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let item = generated_file(&json_files, "Item.cs")?;
        let database = generated_file(&json_files, "GameConfig.cs")?;
        require_contains(item, "public double Scalar { get; set; } = 1.0;")?;
        require_contains(item, "public IReadOnlyList<double> Amounts { get; set; }")?;
        require_contains(
            database,
            "private static double ReadFloat(JToken token, string path)",
        )?;
        require_contains(database, "return token.Value<double>();")?;
        require_not_contains(database, "float")?;
        require_not_contains(database, "1.0f")?;

        let messagepack_files =
            generate_messagepack(&schema, &CsharpCodegenOptions::new("Game.Config"))
                .map_err(|err| err.to_string())?;
        let database = generated_file(&messagepack_files, "GameConfig.cs")?;
        require_contains(
            database,
            "private static double ReadFloat(ref MessagePackReader reader, string path)",
        )?;
        require_contains(database, "MessagePackType.Integer => (double)ReadInt")?;
        require_contains(database, "MessagePackType.Float => reader.ReadDouble()")?;
        require_not_contains(database, "(float)")?;
        Ok(())
    }

    #[test]
    fn codegen_struct_refs_return_updated_values_and_write_back() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type Target { @id id: string; }
                @struct
                sealed type Child {
                    @ref(Target)
                    target_id: string;
                }
                @struct
                sealed type Parent {
                    child: Child;
                    children: [Child] = [];
                    by_name: {string: Child} = {};
                }
                type Holder {
                    @id id: string;
                    parent: Parent;
                }
            ",
        )?;

        let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let child = generated_file(&files, "Child.cs")?;
        let parent = generated_file(&files, "Parent.cs")?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(child, "public Target Target { get; internal set; }")?;
        require_contains(parent, "public Child Child { get; internal set; }")?;
        require_contains(
            parent,
            "public IReadOnlyList<Child> Children { get; internal set; }",
        )?;
        require_contains(
            parent,
            "public IReadOnlyDictionary<string, Child> ByName { get; internal set; }",
        )?;
        require_contains(database, "private static Child ResolveChildRefs(")?;
        require_contains(database, "return value;")?;
        require_contains(database, "value.Child = ResolveChildRefs(")?;
        require_contains(database, "list1[i1] = ResolveChildRefs(")?;
        require_contains(database, "dictionary1[key1] = ResolveChildRefs(")?;
        require_contains(database, "value.Parent = ResolveParentRefs(")?;
        Ok(())
    }

    #[test]
    fn codegen_top_level_struct_tables_write_back_resolved_refs() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type Target { @id id: string; }
                @struct
                sealed type Record {
                    @id id: string;
                    @ref(Target)
                    target_id: string;
                }
            ",
        )?;

        let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(database, "records[i] = ResolveRecordRefs(")?;
        require_contains(database, "var record = records[i];")?;
        require_contains(database, "$\"Record[{record.Id}]\"")?;
        require_not_contains(database, "foreach (var record in records)")?;
        Ok(())
    }

    #[test]
    fn codegen_does_not_write_back_nested_structs_without_refs() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type Target { @id id: string; }
                type RefRecord {
                    @id id: string;
                    @ref(Target)
                    target_id: string;
                }
                @struct
                sealed type ValueBlock {
                    amount: int;
                }
                type Holder {
                    @id id: string;
                    block: ValueBlock;
                }
            ",
        )?;

        let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let holder = generated_file(&files, "Holder.cs")?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(holder, "public ValueBlock Block { get; set; }")?;
        require_not_contains(database, "value.Block = ResolveValueBlockRefs(")?;
        Ok(())
    }

    #[test]
    fn codegen_nested_struct_ref_containers_use_distinct_resolver_locals() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type Target { @id id: string; }
                @struct
                sealed type Child {
                    @ref(Target)
                    target_id: string;
                }
                @struct
                sealed type Parent {
                    nested_children: [[Child]] = [];
                    nested_by_name: {string: {string: Child}} = {};
                }
                type Holder {
                    @id id: string;
                    parent: Parent;
                }
            ",
        )?;

        let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(
            database,
            "var list1 = (List<List<Child>>)value.NestedChildren;",
        )?;
        require_contains(database, "for (var i1 = 0; i1 < list1.Count; i1++)")?;
        require_contains(database, "var list2 = (List<Child>)list1[i1];")?;
        require_contains(database, "for (var i2 = 0; i2 < list2.Count; i2++)")?;
        require_contains(database, "list2[i2] = ResolveChildRefs(")?;
        require_contains(
            database,
            "var dictionary1 = (Dictionary<string, Dictionary<string, Child>>)value.NestedByName;",
        )?;
        require_contains(
            database,
            "foreach (var key1 in new List<string>(dictionary1.Keys))",
        )?;
        require_contains(
            database,
            "var dictionary2 = (Dictionary<string, Child>)dictionary1[key1];",
        )?;
        require_contains(
            database,
            "foreach (var key2 in new List<string>(dictionary2.Keys))",
        )?;
        require_contains(database, "dictionary2[key2] = ResolveChildRefs(")?;
        require_not_contains(database, "var list = ")?;
        require_not_contains(database, "var dictionary = ")?;
        require_not_contains(database, "foreach (var key in ")?;
        Ok(())
    }

    #[test]
    fn codegen_nullable_struct_ref_collections_do_not_use_nullable_value() -> Result<(), String> {
        let schema = compile_schema(
            r"
                type Target { @id id: string; }
                @struct
                sealed type Child {
                    @ref(Target)
                    target_id: string;
                }
                type Holder {
                    @id id: string;
                    maybe_child: Child?;
                    maybe_children: [Child]?;
                    maybe_by_name: {string: Child}?;
                }
            ",
        )?;

        let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let database = generated_file(&files, "GameConfig.cs")?;
        require_contains(database, "var nullableValue1 = value.MaybeChild.Value;")?;
        require_contains(database, "value.MaybeChild = nullableValue1;")?;
        require_contains(database, "var list1 = (List<Child>)value.MaybeChildren;")?;
        require_contains(
            database,
            "var dictionary1 = (Dictionary<string, Child>)value.MaybeByName;",
        )?;
        require_not_contains(database, "value.MaybeChildren.Value")?;
        require_not_contains(database, "value.MaybeByName.Value")?;
        Ok(())
    }

    #[test]
    fn codegen_rejects_invalid_csharp_names() -> Result<(), String> {
        let unicode_type = compile_schema("type 示例 { value: int; }")?;
        let Err(err) = generate_json(&unicode_type, &CsharpCodegenOptions::new("Game.Config"))
        else {
            return Err("unicode type should fail".to_string());
        };
        require_contains(&err.to_string(), "invalid C# type name `示例`")?;

        let schema = compile_schema("type Item { value: int; }")?;
        let Err(err) = generate_json(&schema, &CsharpCodegenOptions::new("Game.1Bad")) else {
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

        let Err(err) = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config")) else {
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

        let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
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

        let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
        let rarity = generated_file(&files, "Rarity.cs")?;
        require_contains(rarity, "/// <summary>Common display</summary>")?;
        require_contains(rarity, "[Obsolete]")?;
        Ok(())
    }

    #[derive(Debug)]
    struct StringSerializerError(String);

    impl fmt::Display for StringSerializerError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            self.0.fmt(f)
        }
    }

    impl std::error::Error for StringSerializerError {}

    impl serde::ser::Error for StringSerializerError {
        fn custom<T: fmt::Display>(msg: T) -> Self {
            Self(msg.to_string())
        }
    }

    struct StringSerializer;

    impl serde::Serializer for StringSerializer {
        type Ok = String;
        type Error = StringSerializerError;
        type SerializeSeq = serde::ser::Impossible<String, StringSerializerError>;
        type SerializeTuple = serde::ser::Impossible<String, StringSerializerError>;
        type SerializeTupleStruct = serde::ser::Impossible<String, StringSerializerError>;
        type SerializeTupleVariant = serde::ser::Impossible<String, StringSerializerError>;
        type SerializeMap = serde::ser::Impossible<String, StringSerializerError>;
        type SerializeStruct = serde::ser::Impossible<String, StringSerializerError>;
        type SerializeStructVariant = serde::ser::Impossible<String, StringSerializerError>;

        fn serialize_str(self, value: &str) -> Result<Self::Ok, Self::Error> {
            Ok(value.to_string())
        }

        fn serialize_bool(self, _value: bool) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_i8(self, _value: i8) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_i16(self, _value: i16) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_i32(self, _value: i32) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_i64(self, _value: i64) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_u8(self, _value: u8) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_u16(self, _value: u16) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_u32(self, _value: u32) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_u64(self, _value: u64) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_f32(self, _value: f32) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_f64(self, _value: f64) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_char(self, _value: char) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_bytes(self, _value: &[u8]) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_some<T: ?Sized + serde::Serialize>(
            self,
            _value: &T,
        ) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_unit_variant(
            self,
            _name: &'static str,
            _variant_index: u32,
            variant: &'static str,
        ) -> Result<Self::Ok, Self::Error> {
            Ok(variant.to_string())
        }

        fn serialize_newtype_struct<T: ?Sized + serde::Serialize>(
            self,
            _name: &'static str,
            _value: &T,
        ) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_newtype_variant<T: ?Sized + serde::Serialize>(
            self,
            _name: &'static str,
            _variant_index: u32,
            _variant: &'static str,
            _value: &T,
        ) -> Result<Self::Ok, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_tuple_struct(
            self,
            _name: &'static str,
            _len: usize,
        ) -> Result<Self::SerializeTupleStruct, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_tuple_variant(
            self,
            _name: &'static str,
            _variant_index: u32,
            _variant: &'static str,
            _len: usize,
        ) -> Result<Self::SerializeTupleVariant, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_struct(
            self,
            _name: &'static str,
            _len: usize,
        ) -> Result<Self::SerializeStruct, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }

        fn serialize_struct_variant(
            self,
            _name: &'static str,
            _variant_index: u32,
            _variant: &'static str,
            _len: usize,
        ) -> Result<Self::SerializeStructVariant, Self::Error> {
            Err(serde::ser::Error::custom("expected string"))
        }
    }
}
