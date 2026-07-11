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
use std::fmt;

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

fn require_missing_file(files: &[GeneratedFile], name: &str) -> Result<(), String> {
    if files
        .iter()
        .any(|file| file.relative_path.as_os_str() == name)
    {
        Err(format!("unexpected generated file `{name}`"))
    } else {
        Ok(())
    }
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

fn generated_output(files: &[GeneratedFile]) -> String {
    files
        .iter()
        .map(|file| file.contents.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn json_database_templates() -> CsharpDatabaseTemplates {
    CsharpDatabaseTemplates {
        database_template: CsharpTemplate {
            name: "database_json.cs.tera",
            contents: include_str!("../templates/json/database_json.cs.tera"),
        },
        partials: &[],
    }
}

fn messagepack_database_templates() -> CsharpDatabaseTemplates {
    CsharpDatabaseTemplates {
        database_template: CsharpTemplate {
            name: "database_messagepack.cs.tera",
            contents: include_str!("../templates/messagepack/database_messagepack.cs.tera"),
        },
        partials: &[],
    }
}

fn generate_json(
    schema: &CftContainer,
    options: &CsharpCodegenOptions,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let compiled_schema = schema.compiled_schema();
    generate_csharp(
        &compiled_schema,
        options,
        CsharpDataFormat::Json,
        &json_database_templates(),
    )
}

fn generate_messagepack(
    schema: &CftContainer,
    options: &CsharpCodegenOptions,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let compiled_schema = schema.compiled_schema();
    generate_csharp(
        &compiled_schema,
        options,
        CsharpDataFormat::MessagePack,
        &messagepack_database_templates(),
    )
}

fn generate_json_with_id_as_enum_variants(
    schema: &CftContainer,
    options: &CsharpCodegenOptions,
    variants: BTreeMap<String, Vec<CsharpIdAsEnumVariant>>,
) -> Result<Vec<GeneratedFile>, CsharpCodegenError> {
    let compiled_schema = schema.compiled_schema();
    generate_csharp_with_id_as_enum_variants(
        &compiled_schema,
        options,
        CsharpDataFormat::Json,
        &json_database_templates(),
        variants,
        None,
    )
}

#[test]
fn codegen_wraps_localized_fields_and_emits_runtime_helper() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            type Item {
                @localized
                display_name: string;
                count: int;
            }
        "#,
    )?;
    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;

    let item = generated_file(&files, "Item.cs")?;
    require_contains(item, "public Localized<string> DisplayName { get; }")?;
    require_contains(item, "public long Count { get; }")?;
    require_contains(
        item,
        "new Localized<string>(string.Concat(\"Item/display_name/\", id.ToString())",
    )?;

    let helper = generated_file(&files, "Localized.cs")?;
    require_contains(helper, "public readonly struct Localized<T>")?;
    require_contains(helper, "public static class Localization")?;
    require_contains(helper, "public interface LocalizationProvider")?;
    Ok(())
}

#[test]
fn codegen_does_not_emit_localized_helper_without_localized_fields() -> Result<(), String> {
    let schema = compile_schema("type Item { name: string; }")?;
    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    require_missing_file(&files, "Localized.cs")?;
    Ok(())
}

#[test]
fn codegen_emits_singleton_property_on_database_class_and_skips_table() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            @singleton
            type GameConfig {
                max_level: int;
            }

            type Item { name: string; }
        "#,
    )?;
    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;

    let database = generated_file(&files, "CoflowTables.cs")?;
    require_contains(database, "public GameConfig GameConfig { get; }")?;
    require_contains(
        database,
        "GameConfig.LoadTable(Path.Combine(dataDir, \"GameConfig.json\"), LoadContext.Empty)",
    )?;
    require_contains(database, "must have exactly 1 record")?;
    require_not_contains(database, "TbGameConfig")?;
    // Item is still a regular table.
    require_contains(database, "public Table<string, Item> TbItem { get; }")?;

    // The singleton type's loader must actually define the `LoadTable`
    // method the database template calls. Without this, the generated C#
    // doesn't compile — a regression pre-spec-17 already shipped silently
    // because no test downloaded the artifacts and ran `dotnet build`.
    let singleton = generated_file(&files, "GameConfig.cs")?;
    require_contains(
        singleton,
        "internal static List<GameConfig> LoadTable(string path, CoflowTables.LoadContext context)",
    )?;
    // The singleton has no per-row `id` field; `LoadTable` should wrap
    // `LoadInline`, which silently skips the wire-side `"id"` key that
    // the JSON exporter writes for each row.
    require_contains(singleton, "result.Add(LoadInline(row, context));")?;
    Ok(())
}

#[test]
fn codegen_emits_singleton_loadtable_for_messagepack() -> Result<(), String> {
    // Same regression check for the msgpack code path: a singleton type
    // must expose `LoadTable` so the shared database template links.
    let schema = compile_schema(
        r#"
            @singleton
            type GameConfig {
                max_level: int;
            }
        "#,
    )?;
    let files = generate_messagepack(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    let singleton = generated_file(&files, "GameConfig.cs")?;
    require_contains(
        singleton,
        "internal static List<GameConfig> LoadTable(string path, CoflowTables.LoadContext context)",
    )?;
    // The msgpack loader for a singleton wraps `LoadInline`, same as
    // JSON. `LoadInline` reads the type's field map; the writer emits an
    // `"id"` key the reader's `default: reader.Skip()` swallows.
    require_contains(singleton, "result.Add(LoadInline(ref reader, context));")?;
    Ok(())
}

#[test]
fn codegen_emits_singleton_only_database_without_table_commas() -> Result<(), String> {
    // Database with no regular tables: comma generation between table block
    // and singleton block must not produce a stray leading or trailing comma.
    let schema = compile_schema(
        r#"
            @singleton
            type GameConfig {
                max_level: int;
            }
        "#,
    )?;
    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    let database = generated_file(&files, "CoflowTables.cs")?;
    require_contains(database, "public GameConfig GameConfig { get; }")?;
    require_not_contains(database, "TbGameConfig")?;
    // No leading "," before the first parameter and no double commas.
    require_not_contains(database, "(\n            ,")?;
    require_not_contains(database, ",,")?;
    Ok(())
}

#[test]
fn codegen_emits_multiple_singletons_with_correct_separators() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            @singleton
            type GameConfig {
                max_level: int;
            }

            @singleton
            type ServerConfig {
                region: string;
            }

            type Item { name: string; }
        "#,
    )?;
    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    let database = generated_file(&files, "CoflowTables.cs")?;
    require_contains(database, "public GameConfig GameConfig { get; }")?;
    require_contains(database, "public ServerConfig ServerConfig { get; }")?;
    require_contains(database, "public Table<string, Item> TbItem { get; }")?;
    require_not_contains(database, ",,")?;
    Ok(())
}

#[test]
fn data_format_serializes_messagepack_without_separator() -> Result<(), String> {
    let value = serde::Serialize::serialize(&CsharpDataFormat::MessagePack, StringSerializer)
        .map_err(|err| err.to_string())?;
    assert_eq!(value, "messagepack");
    Ok(())
}

#[test]
fn codegen_emits_coflow_tables_accessor_api_without_load_exception_or_ref_placeholders(
) -> Result<(), String> {
    let schema = compile_schema(
        r#"
            type Reward {
                amount: int;
            }

            type Item {
                display_name: string;
                reward: &Reward;
                tags: [string];
            }
        "#,
    )?;

    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;

    let database = generated_file(&files, "CoflowTables.cs")?;
    require_missing_file(&files, "GameConfig.cs")?;
    require_missing_file(&files, "CftLoadException.cs")?;
    require_contains(database, "public sealed partial class CoflowTables")?;
    require_contains(database, "public Table<string, Reward> TbReward { get; }")?;
    require_contains(database, "public Table<string, Item> TbItem { get; }")?;
    require_contains(database, "public static CoflowTables Load(string dataDir)")?;
    require_contains(
        database,
        "Reward.LoadRawTable(Path.Combine(dataDir, \"Reward.json\"))",
    )?;
    require_contains(
        database,
        "Item.LoadRawTable(Path.Combine(dataDir, \"Item.json\"))",
    )?;
    require_contains(
        database,
        "var context = new LoadContext(itemIndex, rewardIndex);",
    )?;
    require_contains(
        database,
        "Reward.HydrateAll(rewards, rewardRawRows, context);",
    )?;
    require_contains(database, "Item.HydrateAll(items, itemRawRows, context);")?;
    require_contains(
        database,
        "public sealed class Table<TKey, TRecord> : IReadOnlyList<TRecord>",
    )?;
    require_contains(database, "public TRecord? Find(TKey id)")?;
    require_contains(database, "public bool TryGet(TKey id, out TRecord value)")?;
    require_contains(database, "public TRecord Get(TKey id)")?;
    require_not_contains(database, "FindItem")?;

    let item = generated_file(&files, "Item.cs")?;
    require_contains(item, "public sealed partial class Item : IEquatable<Item>")?;
    require_contains(item, "public string Id { get; }")?;
    require_contains(item, "public string DisplayName { get; }")?;
    require_contains(item, "public Reward Reward { get => _reward; }")?;
    require_contains(item, "public IReadOnlyList<string> Tags { get; }")?;
    require_contains(item, "public Item(")?;
    require_contains(
        item,
        "internal static (List<Item> Rows, Dictionary<Item, JToken> RawRows) LoadRawTable(",
    )?;
    require_contains(
        item,
        "internal static void HydrateAll(List<Item> rows, Dictionary<Item, JToken> rawRows,",
    )?;
    require_contains(item, "internal static Dictionary<string, Item> BuildIndex(")?;
    require_contains(item, "context.GetReward(CoflowJson.ReadString(token))")?;
    require_contains(item, "public override string ToString()")?;
    require_contains(item, "public bool Equals(Item? other)")?;
    require_contains(item, "public override int GetHashCode()")?;
    require_not_contains(item, "set;")?;
    require_not_contains(item, "RewardKey")?;

    let output = generated_output(&files);
    for forbidden in [
        "__CoflowIsRef",
        "__CoflowRefKey",
        "ResolveRefs",
        "ResolveAll",
        "CftLoadException",
        "System.Reflection",
        "Activator",
        "PropertyInfo",
        "FieldInfo",
        "Type.GetType",
        "GetProperties(",
        "GetFields(",
    ] {
        require_not_contains(&output, forbidden)?;
    }

    Ok(())
}

#[test]
fn codegen_uses_schema_ref_type_to_choose_reference_or_inline_loader() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            type Reward {
                amount: int;
            }

            type Item {
                inline_reward: Reward;
                ref_reward: &Reward;
            }
        "#,
    )?;

    let json_files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    let json_item = generated_file(&json_files, "Item.cs")?;
    require_contains(
        json_item,
        "CoflowJson.ReadRequired(obj, \"inline_reward\", (token) => Reward.LoadInline(token, context))",
    )?;
    require_contains(
        json_item,
        "CoflowJson.ReadRequired(obj, \"ref_reward\", (token) => context.GetReward(CoflowJson.ReadString(token)))",
    )?;
    require_not_contains(json_item, "JTokenType.String ?")?;

    let messagepack_files =
        generate_messagepack(&schema, &CsharpCodegenOptions::new("Game.Config"))
            .map_err(|err| err.to_string())?;
    let messagepack_item = generated_file(&messagepack_files, "Item.cs")?;
    require_contains(messagepack_item, "Reward.LoadInline(ref reader, context)")?;
    require_contains(
        messagepack_item,
        "context.GetReward(CoflowMessagePack.ReadString(ref reader))",
    )?;
    require_not_contains(messagepack_item, "CoflowMessagePack.NextIsString")?;
    Ok(())
}

#[test]
fn codegen_uses_pascal_case_public_names_and_raw_source_names_for_loading() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            enum rarity_level {
                common_item,
            }

            type item_config {
                display_name: string;
                rarity: rarity_level = rarity_level.common_item;
            }
        "#,
    )?;

    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;

    let rarity = generated_file(&files, "RarityLevel.cs")?;
    require_contains(rarity, "public enum RarityLevel")?;
    require_contains(rarity, "CommonItem = 0")?;

    let item = generated_file(&files, "ItemConfig.cs")?;
    require_contains(item, "public sealed partial class ItemConfig")?;
    require_contains(item, "public string DisplayName { get; }")?;
    require_contains(item, "public RarityLevel Rarity { get; }")?;
    require_contains(item, "CoflowJson.ReadRequired(obj, \"display_name\"")?;
    require_contains(item, "RarityLevel.CommonItem")?;

    let database = generated_file(&files, "CoflowTables.cs")?;
    require_contains(
        database,
        "public Table<string, ItemConfig> TbItemConfig { get; }",
    )?;
    require_contains(database, "Path.Combine(dataDir, \"item_config.json\")")?;
    Ok(())
}

#[test]
fn codegen_rejects_pascal_case_name_collisions() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            enum Rarity {
                common_item,
                commonItem,
            }
        "#,
    )?;

    let Err(err) = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config")) else {
        return Err("PascalCase enum variant collision should fail".to_string());
    };
    require_contains(
        &err.to_string(),
        "generated C# enum variant name `CommonItem` collides",
    )?;
    Ok(())
}

#[test]
fn codegen_id_as_enum_generates_strongly_typed_ids_and_refs() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            @idAsEnum(GeneId)
            type GeneConfig {}

            enum GeneId {}

            type BioRemainsConfig {
                gene: &GeneConfig;
                maybe_gene: &GeneConfig?;
            }
        "#,
    )?;
    let mut variants = BTreeMap::new();
    variants.insert(
        "GeneId".to_string(),
        vec![
            CsharpIdAsEnumVariant {
                name: "gene_spore".to_string(),
                value: 0,
            },
            CsharpIdAsEnumVariant {
                name: "gene_mating".to_string(),
                value: 1,
            },
        ],
    );

    let files = generate_json_with_id_as_enum_variants(
        &schema,
        &CsharpCodegenOptions::new("Game.Config"),
        variants,
    )
    .map_err(|err| err.to_string())?;

    let gene_id = generated_file(&files, "GeneId.cs")?;
    require_contains(gene_id, "public enum GeneId")?;
    require_contains(gene_id, "gene_spore = 0")?;
    require_contains(gene_id, "gene_mating = 1")?;

    let gene = generated_file(&files, "GeneConfig.cs")?;
    require_contains(gene, "public GeneId Id { get; }")?;
    require_not_contains(gene, "public string Id")?;

    let remains = generated_file(&files, "BioRemainsConfig.cs")?;
    require_contains(remains, "public GeneConfig Gene { get => _gene; }")?;
    require_contains(
        remains,
        "public GeneConfig? MaybeGene { get => _maybeGene; }",
    )?;
    require_contains(
        remains,
        "context.GetGeneConfig(CoflowJson.ReadStringEnum<GeneId>(token))",
    )?;
    Ok(())
}

#[test]
fn codegen_applies_nullable_and_missing_collection_rules() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            type Target {}

            type Holder {
                maybe_target: &Target?;
                tags: [string];
                attrs: {string: int};
            }
        "#,
    )?;

    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    let holder = generated_file(&files, "Holder.cs")?;

    require_contains(
        holder,
        "public Target? MaybeTarget { get => _maybeTarget; }",
    )?;
    require_contains(holder, "public IReadOnlyList<string> Tags { get; }")?;
    require_contains(
        holder,
        "public IReadOnlyDictionary<string, long> Attrs { get; }",
    )?;
    require_contains(holder, "CoflowJson.ReadNullable(obj, \"maybe_target\"")?;
    require_contains(holder, "new List<string>()")?;
    require_contains(holder, "new Dictionary<string, long>()")?;
    Ok(())
}

#[test]
fn codegen_concrete_inheritance_passes_base_fields_and_emits_equality() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            type Item {
                display_name: string;
            }

            type Equipment : Item {
                power: int;
            }
        "#,
    )?;

    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    let item = generated_file(&files, "Item.cs")?;
    let equipment = generated_file(&files, "Equipment.cs")?;

    require_contains(item, "public partial class Item : IEquatable<Item>")?;
    require_contains(
        equipment,
        "public sealed partial class Equipment : Item, IEquatable<Equipment>",
    )?;
    require_contains(equipment, ") : base(id, displayName)")?;
    require_contains(equipment, "public bool Equals(Equipment? other)")?;
    require_contains(equipment, "public override int GetHashCode()")?;
    Ok(())
}

#[test]
fn codegen_renames_context_field_local_to_avoid_loader_parameter_collision() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            type Text {
                context: string;
            }
        "#,
    )?;

    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    let text = generated_file(&files, "Text.cs")?;

    require_contains(text, "public string Context { get; }")?;
    require_contains(text, "var contextValue = CoflowJson.ReadRequired")?;
    require_contains(text, "Context = contextValue;")?;
    require_not_contains(text, "var context = CoflowJson.ReadRequired")?;
    Ok(())
}

#[test]
fn codegen_generates_polymorphic_loader_for_concrete_base_types() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            type Item {
                name: string;
            }

            type Equipment : Item {
                power: int;
            }

            type Holder {
                item: Item;
            }
        "#,
    )?;

    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    let item = generated_file(&files, "Item.cs")?;
    let holder = generated_file(&files, "Holder.cs")?;

    require_contains(item, "internal static Item LoadPolymorphic(")?;
    require_contains(item, "\"Item\" => Item.LoadInline(token, context),")?;
    require_contains(
        item,
        "\"Equipment\" => Equipment.LoadInline(token, context),",
    )?;
    require_contains(holder, "Item.LoadPolymorphic(token, context)")?;
    Ok(())
}

#[test]
fn codegen_allows_old_reserved_type_names_but_rejects_database_class_collision(
) -> Result<(), String> {
    let schema = compile_schema(
        r#"
            type GameConfig {
                value: int;
            }

            type CftLoadException {
                value: int;
            }
        "#,
    )?;

    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    generated_file(&files, "GameConfig.cs")?;
    generated_file(&files, "CftLoadException.cs")?;
    generated_file(&files, "CoflowTables.cs")?;

    let schema = compile_schema("type CoflowTables { value: int; }")?;
    let Err(err) = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config")) else {
        return Err("default database class collision should fail".to_string());
    };
    require_contains(
        &err.to_string(),
        "generated C# file name `CoflowTables.cs` is reserved",
    )?;

    let files = generate_json(
        &schema,
        &CsharpCodegenOptions::new("Game.Config").with_database_class("RuntimeConfig"),
    )
    .map_err(|err| err.to_string())?;
    generated_file(&files, "CoflowTables.cs")?;
    generated_file(&files, "RuntimeConfig.cs")?;
    Ok(())
}

#[test]
fn codegen_json_loads_tables_without_reference_dependency_order() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            type Reward {}

            type DropTable {
                reward: &Reward;
            }

            type Item {
                drop_table: &DropTable;
            }
        "#,
    )?;

    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    let database = generated_file(&files, "CoflowTables.cs")?;
    let drop_table_raw = database
        .find("var (dropTables, dropTableRawRows) = DropTable.LoadRawTable")
        .ok_or_else(|| "missing DropTable raw load".to_string())?;
    let item_raw = database
        .find("var (items, itemRawRows) = Item.LoadRawTable")
        .ok_or_else(|| "missing Item raw load".to_string())?;
    let reward_raw = database
        .find("var (rewards, rewardRawRows) = Reward.LoadRawTable")
        .ok_or_else(|| "missing Reward raw load".to_string())?;
    let first_index = database
        .find("BuildIndex")
        .ok_or_else(|| "missing index build".to_string())?;
    let context = database
        .find("var context = new LoadContext")
        .ok_or_else(|| "missing full load context".to_string())?;
    let first_hydrate = database
        .find("HydrateAll")
        .ok_or_else(|| "missing hydrate pass".to_string())?;

    assert!(
        drop_table_raw < first_index && item_raw < first_index && reward_raw < first_index,
        "all raw table loads should happen before any index build"
    );
    assert!(
        first_index < context && context < first_hydrate,
        "indexes should be built before the full context and hydrate pass"
    );
    require_contains(
        database,
        "var context = new LoadContext(dropTableIndex, itemIndex, rewardIndex);",
    )?;
    require_contains(
        database,
        "DropTable.HydrateAll(dropTables, dropTableRawRows, context);",
    )?;
    require_contains(database, "Item.HydrateAll(items, itemRawRows, context);")?;
    require_contains(
        database,
        "Reward.HydrateAll(rewards, rewardRawRows, context);",
    )?;
    Ok(())
}

#[test]
fn codegen_json_allows_cyclic_table_references() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            type Item {
                next: &Item;
            }
        "#,
    )?;

    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    let database = generated_file(&files, "CoflowTables.cs")?;
    require_contains(
        database,
        "Item.LoadRawTable(Path.Combine(dataDir, \"Item.json\"))",
    )?;
    require_contains(database, "var context = new LoadContext(itemIndex);")?;
    require_contains(database, "Item.HydrateAll(items, itemRawRows, context);")?;

    let item = generated_file(&files, "Item.cs")?;
    require_contains(item, "public Item Next { get => _next; }")?;
    require_not_contains(item, "_coflowRaw")?;
    require_contains(
        item,
        "internal static (List<Item> Rows, Dictionary<Item, JToken> RawRows) LoadRawTable(",
    )?;
    require_contains(
        item,
        "internal static void HydrateAll(List<Item> rows, Dictionary<Item, JToken> rawRows,",
    )?;
    require_contains(item, "context.GetItem(CoflowJson.ReadString(token))")?;
    Ok(())
}

#[test]
fn codegen_messagepack_emits_coflow_tables_and_messagepack_loaders() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            type Reward {
                amount: int;
            }

            type Item {
                reward: &Reward;
            }
        "#,
    )?;

    let files = generate_messagepack(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    let database = generated_file(&files, "CoflowTables.cs")?;
    require_contains(database, "using MessagePack;")?;
    require_contains(database, "Path.Combine(dataDir, \"Reward.msgpack\")")?;
    require_contains(database, "Path.Combine(dataDir, \"Item.msgpack\")")?;
    require_contains(database, "public Table<string, Item> TbItem { get; }")?;
    require_not_contains(database, "Newtonsoft.Json")?;

    let item = generated_file(&files, "Item.cs")?;
    require_contains(item, "using MessagePack;")?;
    require_contains(item, "internal static List<Item> LoadTable(")?;
    require_contains(
        item,
        "context.GetReward(CoflowMessagePack.ReadString(ref reader))",
    )?;
    require_not_contains(item, "CoflowMessagePack.NextIsString")?;
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
