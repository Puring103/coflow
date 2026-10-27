#![allow(clippy::expect_used, clippy::panic)]
use super::*;
use coflow_codegen::CodegenTarget;
use coflow_language::cft::{build_schema, parse_modules, CftFile, ModuleId};

fn schema(source: &str) -> CftSchema {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main.cft"), source)]);
    build_schema(&modules).expect("schema")
}

#[test]
fn rejects_xid_names_that_csharp_cannot_represent() {
    assert!(generate_csharp(&schema("table ℘ { value: int = 1; }")).is_err());
    assert!(generate_csharp(&schema("table 𐐀 { value: int = 1; }")).is_err());
    assert!(generate_csharp(&schema("table 物品 { 名称: string; }")).is_ok());
}

#[test]
fn escapes_unicode_line_separators_in_generated_literals() {
    let text = all(&generate_csharp(&schema(
        "table Item { value: string = \"a\u{2028}b\u{2029}c\"; }",
    ))
    .expect("generate"));
    assert!(text.contains("a\\u2028b\\u2029c"));
    assert!(!text.contains('\u{2028}'));
    assert!(!text.contains('\u{2029}'));
}
fn all(files: &[GeneratedFile]) -> String {
    files
        .iter()
        .map(|file| file.contents.as_str())
        .collect::<Vec<_>>()
        .join("\n")
        .replace("global::Coflow.Loader.", "")
        .replace("global::System.Collections.Generic.", "")
        .replace("global::System.", "")
}

#[test]
fn emits_typed_business_objects_and_shared_loader_descriptors() {
    let schema = schema(
        r"
        @struct sealed data Stats { hp: int; }
        table Item { name: string; stats: Stats; next: Option<&Item> = None; }
    ",
    );
    let text = all(&generate_csharp(&schema).expect("generate"));
    assert!(text.contains("public partial struct Stats"));
    assert!(text.contains("public partial class Item"));
    assert!(text.contains("public int hp { get; internal set; }"));
    assert!(text.contains("public Option<Item> next { get; internal set; }"));
    assert!(text.contains("new CfdType<Stats>"));
    assert!(text.contains("reader => new Stats"));
    assert!(text.contains("id => new Item(id)"));
    assert!(text.contains("CfdValues.Reference<Item>(\"Item\")"));
    assert!(text.contains("CfdLoader.Load(sources, Types"));
    for forbidden in [
        "Coflow.Runtime",
        "Arena",
        "CoflowFunction",
        "typeof(",
        "Reflection",
        "CfdParser",
        "Check(",
    ] {
        assert!(!text.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn inheritance_preserves_shared_record_identity() {
    let text = all(&generate_csharp(&schema(
        "abstract table Base { name: string; } table Child: Base { count: int; }",
    ))
    .expect("generate"));
    assert!(text.contains("public abstract partial class Base"));
    assert!(text.contains("public partial class Child : Base"));
    assert!(text.contains("internal Child(string id) : base(id)"));
    assert!(text.contains("value.name = reader.Get<string>(\"name\")"));
    assert!(text.contains("parent: \"Base\""));
    assert!(text.contains("abstractType: true"));
}

#[test]
fn emitted_metadata_is_data_only_even_when_schema_has_checks() {
    let text = all(&generate_csharp(&schema(
        r#"table Item { value: int; check Valid { value > 0: "positive"; } } check Validate { 1 > 0; }"#,
    ))
    .expect("generate"));
    assert!(!text.contains("positive"));
    assert!(!text.contains("Validate"));
}

#[test]
fn descriptor_defaults_preserve_literal_strings_and_explicit_templates() {
    let text = all(&generate_csharp(&schema(
        r#"
        table Item {
            name: string = "{literal}";
            label: string = f"{{text}} {self.name}";
            nums: [int] = [1, 2];
        }
    "#,
    ))
    .expect("generate"));
    assert!(text.contains("CfdDefaults.String(\"{literal}\")"));
    assert!(text.contains("CfdValues.String"));
    assert!(text.contains("CfdDefaults.Template(\"f\\\"{{text}} {self.name}\\\"\")"));
    assert!(text.contains("CfdDefaults.Array(CfdDefaults.Int(1L), CfdDefaults.Int(2L))"));
}

#[test]
fn dimension_fields_load_all_variants() {
    let modules = parse_modules([CftFile::from_source(
        ModuleId::from("main.cft"),
        r#"table Item { @dimension("language") name: string; }"#,
    )]);
    let schema = build_schema(&modules).expect("schema");
    let text = all(&generate_csharp(&schema).expect("generate"));
    assert!(text.contains("string name"));
    assert!(text.contains("CfdValues.Dimension(\"language\", CfdValues.String)"));
    assert!(
        !text.contains("\"ja\""),
        "variant selection must not be baked into generated code"
    );
}

#[test]
fn enums_flags_constants_and_namespaces_use_loader_contracts() {
    let schema = schema(
        r"@flag enum Flags { A = 1, B = 2 } const SIZE: int = 3; table Item { flags: Flags = Flags::A | Flags::B; size: int = SIZE; }",
    );
    let text =
        all(&emit::generate(&schema, "Game.Config", &IdAsEnumValues::new()).expect("generate"));
    assert!(text.contains("namespace Game.Config"));
    assert!(text.contains("[Flags]"));
    assert!(text.contains("bits => (Flags)bits"));
    assert!(text.contains("[\"SIZE\"] = CfdDefaults.Int(3L)"));
}

#[test]
fn invalid_identifiers_and_generated_name_collisions_are_reported() {
    for source in [
        "table CoflowData {}",
        "table id {}",
        "table Item { Item: int; }",
        "enum Kind { Kind }",
        "enum Kind { value__ }",
    ] {
        assert!(generate_csharp(&schema(source)).is_err(), "{source}");
    }
    assert!(emit::generate(&schema("table Item {}"), "bad-name", &IdAsEnumValues::new()).is_err());
}

#[test]
fn generator_advertises_loader_package() {
    let target = CodegenTarget::new("csharp", "generated", serde_json::json!({}));
    let schema = schema("table Item { value: int; }");
    let values = IdAsEnumValues::new();
    let generator = CsharpCfdCodeGenerator;
    assert_eq!(generator.descriptor().runtime_package, "Coflow.Loader");
    let output = generator
        .generate(CodegenInput {
            schema: &schema,
            model: None,
            target: &target,
            id_as_enum_values: &values,
        })
        .expect("generate");
    assert!(output
        .files()
        .iter()
        .any(|file| file.relative_path == *"CoflowData.cs"));
}

#[test]
fn id_enum_keeps_active_keys_and_stable_numbers() {
    let schema = schema("enum ItemId {} @idAsEnum(ItemId) table Item { name: string; }");
    let mut builder = coflow_model::CfdDataModel::builder(&schema);
    builder.add_record(
        "live",
        "Item",
        [("name", coflow_model::LoadedValueDraft::from("item"))],
    );
    let model = builder.build().expect("model");
    let values = IdAsEnumValues::from([(
        "ItemId".to_string(),
        std::collections::BTreeMap::from([("live".to_string(), 7), ("deleted".to_string(), 8)]),
    )]);
    let target = CodegenTarget::new("csharp", "generated", serde_json::json!({}));
    let output = CsharpCfdCodeGenerator
        .generate(CodegenInput {
            schema: &schema,
            model: Some(&model),
            target: &target,
            id_as_enum_values: &values,
        })
        .expect("generate");
    let en = output
        .files()
        .iter()
        .find(|file| file.relative_path == *"ItemId.cs")
        .expect("enum source");
    assert!(en.contents.contains("live = 7"));
    assert!(!en.contents.contains("deleted"));
}

#[test]
fn language_names_are_escaped_only_in_generated_csharp_identifiers() {
    let text = all(&emit::generate(&schema(
        r"enum event { public } table class { namespace: string; state: event = event::public; }",
    ), "namespace.class", &IdAsEnumValues::new()).expect("generate"));
    assert!(text.contains("namespace @namespace.@class"));
    assert!(text.contains("public partial class @class : ICfdRecord"));
    assert!(text.contains("public string @namespace"));
    assert!(text.contains("public enum @event"));
    assert!(text.contains("@public = 0"));
    assert!(text.contains("value.@namespace = reader.Get<string>(\"namespace\")"));
    assert!(text.contains("CfdType<@class>(\"class\""));
    assert!(text.contains("[\"public\"] = @event.@public"));
}

#[test]
fn numeric_widths_and_builtin_flags_are_generated_consistently() {
    let text = all(&generate_csharp(&schema("@flag enum Bits { A = 1, B = 8 } @int64 enum Wide { Big = 4294967296 } table T { i: int; l: long; f: float; d: double; }")).expect("generate"));
    for expected in [
        "enum Bits : int",
        "None = 0,",
        "Everything = 9,",
        "enum Wide : long",
        "Big = 4294967296L",
        "public int i",
        "public long l",
        "public float f",
        "public double d",
        "CfdValues.Int",
        "CfdValues.Long",
        "CfdValues.Float",
        "CfdValues.Double",
    ] {
        assert!(text.contains(expected), "{expected}");
    }
}
