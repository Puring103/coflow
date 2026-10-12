#![allow(clippy::expect_used, clippy::panic)]
use super::*;
use coflow_codegen::CodegenTarget;
use coflow_language::cft::{build_schema, parse_modules, CftFile, ModuleId};

fn schema(source: &str) -> CftSchema {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main.cft"), source)]);
    build_schema(&modules).expect("schema")
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
        r#"
        @struct sealed data Stats { hp: int; }
        table Item { name: string; stats: Stats; next: Option<&Item> = None; }
    "#,
    );
    let text = all(&generate_csharp(&schema).expect("generate"));
    assert!(text.contains("public partial struct Stats"));
    assert!(text.contains("public partial class Item"));
    assert!(text.contains("public long hp { get; internal set; }"));
    assert!(text.contains("public Option<Item> next { get; internal set; }"));
    assert!(text.contains("new CfdType<Stats>"));
    assert!(text.contains("reader => new Stats"));
    assert!(text.contains("key => new Item(key)"));
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
    assert!(text.contains("internal Child(string key) : base(key)"));
    assert!(text.contains("value.name = reader.Get<string>(\"name\")"));
    assert!(text.contains("parent: \"Base\""));
    assert!(text.contains("abstractType: true"));
}

#[test]
fn emitted_metadata_is_data_only_even_when_schema_has_checks() {
    let text = all(&generate_csharp(&schema(
        r#"table Item { value: int; check { value > 0: "positive"; } } check Validate { 1 > 0; }"#,
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
            label: fstring = f"{{text}} {self.name}";
            nums: [int] = [1, 2];
        }
    "#,
    ))
    .expect("generate"));
    assert!(text.contains("CfdDefaults.String(\"{literal}\")"));
    assert!(text.contains("CfdValues.FormattedString"));
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
    assert!(text.contains("DimensionValue<string> name"));
    assert!(text.contains("CfdValues.Dimension(\"language\", CfdValues.String)"));
    assert!(
        !text.contains("\"ja\""),
        "variant selection must not be baked into generated code"
    );
}

#[test]
fn enums_flags_constants_and_namespaces_use_loader_contracts() {
    let schema = schema(
        r#"@flag enum Flags { A = 1, B = 2 } const SIZE: int = 3; table Item { flags: Flags = Flags::A | Flags::B; size: int = SIZE; }"#,
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
        "table class {}",
        "table CoflowData {}",
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
        .any(|file| file.relative_path == PathBuf::from("CoflowData.cs")));
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
        .find(|file| file.relative_path == PathBuf::from("ItemId.cs"))
        .expect("enum source");
    assert!(en.contents.contains("live = 7L"));
    assert!(!en.contents.contains("deleted"));
}
