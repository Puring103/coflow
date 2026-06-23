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
        "new Localized<string>(string.Concat(\"Item/\", id?.ToString() ?? string.Empty, \"/display_name\")",
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
                reward: Reward;
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
        "Reward.LoadTable(Path.Combine(dataDir, \"Reward.json\"), LoadContext.Empty)",
    )?;
    require_contains(
        database,
        "Item.LoadTable(Path.Combine(dataDir, \"Item.json\"), new LoadContext(rewardIndex))",
    )?;
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
    require_contains(item, "public Reward Reward { get; }")?;
    require_contains(item, "public IReadOnlyList<string> Tags { get; }")?;
    require_contains(item, "private Item(")?;
    require_contains(item, "internal static List<Item> LoadTable(")?;
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
fn codegen_key_as_enum_generates_strongly_typed_ids_and_refs() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            @keyAsEnum(GeneId)
            type GeneConfig {}

            enum GeneId {}

            type BioRemainsConfig {
                gene: GeneConfig;
                maybe_gene: GeneConfig?;
            }
        "#,
    )?;
    let mut variants = BTreeMap::new();
    variants.insert(
        "GeneId".to_string(),
        vec![
            CsharpKeyAsEnumVariant {
                name: "gene_spore".to_string(),
                value: 0,
            },
            CsharpKeyAsEnumVariant {
                name: "gene_mating".to_string(),
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
    require_contains(gene_id, "GeneSpore = 0")?;
    require_contains(gene_id, "GeneMating = 1")?;

    let gene = generated_file(&files, "GeneConfig.cs")?;
    require_contains(gene, "public GeneId Id { get; }")?;
    require_not_contains(gene, "public string Id")?;

    let remains = generated_file(&files, "BioRemainsConfig.cs")?;
    require_contains(remains, "public GeneConfig Gene { get; }")?;
    require_contains(remains, "public GeneConfig? MaybeGene { get; }")?;
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
                maybe_target: Target?;
                tags: [string];
                attrs: {string: int};
            }
        "#,
    )?;

    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    let holder = generated_file(&files, "Holder.cs")?;

    require_contains(holder, "public Target? MaybeTarget { get; }")?;
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
fn codegen_sorts_tables_by_reference_dependencies() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            type Reward {}

            type DropTable {
                reward: Reward;
            }

            type Item {
                drop_table: DropTable;
            }
        "#,
    )?;

    let files = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config"))
        .map_err(|err| err.to_string())?;
    let database = generated_file(&files, "CoflowTables.cs")?;
    let reward = database
        .find("Reward.LoadTable")
        .ok_or_else(|| "missing Reward load".to_string())?;
    let drop_table = database
        .find("DropTable.LoadTable")
        .ok_or_else(|| "missing DropTable load".to_string())?;
    let item = database
        .find("Item.LoadTable")
        .ok_or_else(|| "missing Item load".to_string())?;

    assert!(reward < drop_table, "Reward should load before DropTable");
    assert!(drop_table < item, "DropTable should load before Item");
    require_contains(
        database,
        "DropTable.LoadTable(Path.Combine(dataDir, \"DropTable.json\"), new LoadContext(rewardIndex))",
    )?;
    require_contains(
        database,
        "Item.LoadTable(Path.Combine(dataDir, \"Item.json\"), new LoadContext(rewardIndex, dropTableIndex))",
    )?;
    Ok(())
}

#[test]
fn codegen_rejects_cyclic_table_references_for_immediate_readonly_loading() -> Result<(), String> {
    let schema = compile_schema(
        r#"
            type Item {
                next: Item;
            }
        "#,
    )?;

    let Err(err) = generate_json(&schema, &CsharpCodegenOptions::new("Game.Config")) else {
        return Err("cyclic table reference should fail".to_string());
    };
    require_contains(
        &err.to_string(),
        "C# read-only immediate reference loading does not support cyclic table references",
    )?;
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
                reward: Reward;
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
        "CoflowMessagePack.NextIsString(ref reader) ? context.GetReward",
    )?;
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
