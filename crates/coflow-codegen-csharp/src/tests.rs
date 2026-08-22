#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::*;
use coflow_language::{build_schema, parse_modules, CftFile, CftSchema, ModuleId};
use std::collections::BTreeMap;

fn schema(source: &str) -> CftSchema {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &Default::default()).expect("schema compiles")
}

fn all(files: &[GeneratedFile]) -> String {
    files
        .iter()
        .map(|file| file.contents.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn emits_declarations_and_direct_cfd_entrypoint() {
    let files = generate_csharp_cfd(
        &schema("type Item { name: string; }"),
        &CsharpCodegenOptions::new("Game.Config"),
        &["data/items.cfd".to_string()],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("SourceFiles"));
    assert!(output.contains("ICfdTypeBinding"));
    assert!(output.contains("DeclaredType => \"Item\""));
    assert!(output.contains("CfdValueReader.String"));
    assert!(output.contains("ReadItemFields"));
    assert!(!output.contains("CfdMaterializer"));
    assert!(output.contains("Load(ICfdTextLoader loader, CfdLoadOptions? options = null)"));
    assert!(output.contains("CfdParser.ParseAll"));
    assert!(output.contains("Coflow.Cfd.Runtime"));
    assert!(!output.contains("Newtonsoft.Json"));
    assert!(!output.contains("MessagePack"));
    assert!(!output.contains(".json"));
    assert!(!output.contains(".msgpack"));
}

#[test]
fn preserves_display_metadata_as_xml_docs() {
    let files = generate_csharp(
        &schema(
            r#"@label("Item") @description("Description") type Item { @label("Name") name: string; }"#,
        ),
        &CsharpCodegenOptions::new("Game.Config"),
    )
    .expect("generate");
    let item = files
        .iter()
        .find(|file| file.relative_path.as_os_str() == "Item.cs")
        .expect("item file");
    assert!(item
        .contents
        .contains("<summary>Item: Description</summary>"));
    assert!(item.contents.contains("<summary>Name</summary>"));
}

#[test]
fn descriptor_declares_cfd_runtime_contract() {
    assert_eq!(CSHARP_CFD_CODEGEN_DESCRIPTOR.id, "csharp");
    assert_eq!(
        CSHARP_CFD_CODEGEN_DESCRIPTOR.runtime_package,
        "Coflow.Cfd.Runtime"
    );
    assert!(CSHARP_CFD_CODEGEN_DESCRIPTOR.needs_model);
}

#[test]
fn emits_empty_type_reader_without_invalid_argument_list() {
    let files = generate_csharp_cfd(
        &schema("type Empty { }"),
        &CsharpCodegenOptions::new("Game.Config"),
        &["data/empty.cfd".to_string()],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("CfdValueReader.ValidateFields(fields);"));
    assert!(!output.contains("ValidateFields(fields, );"));
}

#[test]
fn emits_valid_singleton_only_load_context_and_key_collision_guard() {
    let files = generate_csharp_cfd(
        &schema("@singleton type Settings { value: int; }"),
        &CsharpCodegenOptions::new("Game.Config"),
        &["data/settings.cfd".to_string()],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(!output.contains("internal LoadContext(\n        )"));
    assert!(output.contains("CFD-SINGLETON-KEY-COLLISION"));
    assert!(output.contains("var singletonKeys = new HashSet<string>"));
}

#[test]
fn generated_dimension_normalizers_attach_physical_source_paths() {
    let schema = schema("type Item { value: int; }");
    let files = generate_csharp_cfd_with_manifest(
        &schema,
        &CsharpCodegenOptions::new("Game.Config"),
        &[SourceManifestEntry {
            logical_path: "data/dimensions/language/Item_value.cfd".to_string(),
            origin: SourceOrigin::Dimension {
                dimension: "language".to_string(),
                source_type: "Item".to_string(),
                field: "value".to_string(),
            },
        }],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains(
        "NormalizeDimensionRecord(record, \"Item\", \"Item_valueVariants\", document.Path)"
    ));
    assert!(output.contains("not assignable to `{sourceType}`\", path, record.Span"));
}

#[test]
fn emits_source_enum_mappings_and_flag_masks() {
    let files = generate_csharp_cfd(
        &schema(
            "enum item_rarity { common_value, rare_value }\n@flag enum item_flags { fire = 1, ice = 2 }\ntype Item { rarity: item_rarity; flags: item_flags; }",
        ),
        &CsharpCodegenOptions::new("Game.Config"),
        &["data/items.cfd".to_string()],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("ReadEnumItemRarity"));
    assert!(output.contains("\"common_value\" or \"item_rarity.common_value\""));
    assert!(output.contains("ReadEnumItemFlags"));
    assert!(output.contains(" 3L"));
    assert!(output.contains("CfdValueReader.Object(node, context, \"Item\""));
}

#[test]
fn emits_schema_defaults_in_direct_cfd_readers() {
    let files = generate_csharp_cfd(
        &schema(
            r#"
enum item_rarity { common_value, rare_value }
type Stats { hp: int = 10; }
type Item {
    rarity: item_rarity = item_rarity.common_value;
    enabled: bool = false;
    label: string = "line\ntext";
    tags: [string] = [];
    weights: {string: int} = {};
    stats: Stats = {};
    target: &Item? = null;
}
"#,
        ),
        &CsharpCodegenOptions::new("Game.Config"),
        &["data/items.cfd".to_string()],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("CfdValueReader.FindField(fields, \"enabled\")"));
    assert!(output.contains("valueEnabled ? CfdValueReader.Boolean(valueEnabled) : false"));
    assert!(output.contains("ItemRarity.CommonValue"));
    assert!(output.contains("\"line\\ntext\""));
    assert!(output.contains("Array.Empty<string>()"));
    assert!(output.contains("new Dictionary<string, long>()"));
    assert!(output.contains("ReadStatsFields(Array.Empty<CfdFieldNode>(), string.Empty, context)"));
    assert!(output.contains("valueTarget") && output.contains(": default"));
}

#[test]
fn emits_source_names_for_polymorphic_bindings_and_assignability() {
    let files = generate_csharp_cfd(
        &schema(
            r#"
abstract type reward_base {}
sealed type fixed_reward : reward_base { amount: int; }
type concrete_base { name: string; }
sealed type concrete_child : concrete_base { amount: int; }
type Holder { reward: reward_base; target: &reward_base; concrete: concrete_base; }
"#,
        ),
        &CsharpCodegenOptions::new("Game.Config"),
        &["data/rewards.cfd".to_string()],
        BTreeMap::new(),
        None,
    )
    .expect("generate");
    let output = all(&files);
    assert!(output.contains("DeclaredType => \"fixed_reward\""));
    assert!(output.contains(
        "AssignableTypes { get; } = new string[] { \"fixed_reward\", \"reward_base\" }"
    ));
    assert!(output.contains("\"fixed_reward\" => ReadFixedReward(node, context)"));
    assert!(!output.contains("Readfixed_reward"));
    assert!(output.contains(
        "CfdValueReader.Reference<RewardBase>(CfdValueReader.Field(fields, \"target\"), context, \"reward_base\")"
    ));
    assert!(!output.contains("expected a polymorphic object or reference"));
    assert!(output.contains("\"reward\" => \"reward_base\""));
    assert!(output.contains("\"target\" => \"reward_base\""));
    assert!(output.contains("ObjectFieldType(string fieldName)"));
    assert!(output.contains("ReferenceFieldType(string fieldName)"));
    assert!(output.contains("\"concrete_child\" => ReadConcreteChild(node, context)"));
    assert!(output.contains(
        "null or \"concrete_base\" => CfdValueReader.Object(node, context, \"concrete_base\", ReadConcreteBaseFields)"
    ));
}

#[test]
fn registry_generator_returns_safe_code_artifacts() {
    let mut registry = coflow_runtime::codegen::CodegenRegistry::default();
    registry
        .register(CsharpCfdCodeGenerator)
        .expect("register C#");
    assert!(registry.get("csharp").is_some());
    assert!(registry.register(CsharpCfdCodeGenerator).is_err());
}
