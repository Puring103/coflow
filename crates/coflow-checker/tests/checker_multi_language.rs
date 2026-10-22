#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

#[path = "checker_common/mod.rs"]
mod common;
use coflow_language::cft::{DimensionName, FieldName, RecordId, TypeName, VariantName};
use common::*;

fn add_overlay(
    builder: &mut CfdModelBuilder<'_>,
    field: &str,
    variant: &str,
    value: LoadedValueDraft,
) {
    builder.add_dimension_value_draft(DimensionValueDraft {
        source_type: TypeName::new("Item").unwrap(),
        source_id: RecordId::new("item").unwrap(),
        field: FieldName::new(field).unwrap(),
        dimension: DimensionName::new("language").unwrap(),
        variant: VariantName::new(variant).unwrap(),
        value,
        origin: RecordOrigin::File {
            path: "ja.cfd".into(),
            span: None,
        },
    });
}

#[test]
fn explicit_variant_selection_reads_overlay_and_locates_its_source() {
    let schema = compile_schema(
        r#"table Item { @localized name: string; check Valid { name.for("ja") != "" : "empty translation"; name.for("unknown") == "base"; } }"#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("item", "Item", [("name", "base".into())]);
    add_overlay(&mut builder, "name", "ja", "".into());
    let model = builder.build().expect("model");
    let output = execute_checks(
        &schema,
        &model,
        base_tasks(&schema, &model),
        CheckLimits::default(),
    );
    assert_eq!(output.results.len(), 2);
    let diagnostics = output.diagnostics().collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = diagnostics[0];
    assert_eq!(diagnostic.diagnostic.message, "empty translation");
    let label = diagnostic.diagnostic.primary.as_ref().unwrap();
    assert_eq!(label.path, CfdPath::root().field("name"));
    assert_eq!(
        label
            .origin
            .as_ref()
            .unwrap()
            .local_path()
            .unwrap()
            .to_str(),
        Some("ja.cfd")
    );
    assert!(diagnostic.contexts.iter().any(|context| matches!(context, CheckDiagnosticContext::Dimension { dimension, variant } if dimension == "language" && variant == "ja")));
}

#[test]
fn missing_or_unknown_variant_uses_base_value() {
    let schema = compile_schema(
        r#"table Item { @localized name: string; check Valid { name.for("ja") == "base"; name.for("unknown") == "base"; } }"#,
    );
    for overlay in [false, true] {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("item", "Item", [("name", "base".into())]);
        if overlay {
            add_overlay(&mut builder, "name", "fr", "autre".into());
        }
        let model = builder.build().expect("model");
        assert!(run_model_checks(&model, &schema).is_ok());
    }
}

#[test]
fn explicit_none_does_not_fall_back_to_optional_base() {
    let schema = compile_schema(
        r#"table Item { @localized name: Option<string>; check Valid { name.for("ja") == None; name.for("unknown") is Some(value) && value == "base"; } }"#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        [(
            "name",
            LoadedValueDraft::OptionSome(Box::new("base".into())),
        )],
    );
    add_overlay(&mut builder, "name", "ja", LoadedValueDraft::OptionNone);
    let model = builder.build().expect("model");
    assert!(run_model_checks(&model, &schema).is_ok());
}

#[test]
fn for_requires_a_dimension_field_and_a_string_variant() {
    for source in [
        r#"table Item { name: string; check Valid { name.for("ja") == "x"; } }"#,
        r#"table Item { @localized name: string; check Valid { name.for(1) == "x"; } }"#,
    ] {
        let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
        assert!(build_schema(&modules).is_err(), "{source}");
    }
}

#[test]
fn variants_include_empty_headers_and_effective_fallback_values() {
    let schema = compile_schema(
        r#"table Item {
        @localized name: string;
        check Valid {
            name == "base";
            name.variants().len() == 2;
            name.variants()["ja"] == "翻訳";
            name.variants()["fr"] == "base";
            name.variants().containsKey("fr");
            name.variants().containsValue("base");
            name.variants().keys().contains("ja");
            name.variants().values().contains("base");
            all variant, text in name.variants() { text != ""; }
        }
    }"#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("item", "Item", [("name", "base".into())]);
    add_overlay(&mut builder, "name", "ja", "翻訳".into());
    builder.add_dimension_variant(
        DimensionName::new("language").unwrap(),
        VariantName::new("fr").unwrap(),
    );
    let model = builder.build().expect("model");
    let output = execute_checks(
        &schema,
        &model,
        base_tasks(&schema, &model),
        CheckLimits::default(),
    );
    assert!(output.is_success(), "{output:?}");
}

#[test]
fn explicit_presence_and_variant_template_context_are_independent() {
    let schema = compile_schema(
        r#"table Item {
        @localized name: string;
        @localized title: string = f"购买 {self.name}";
        @localized note: Option<string>;
        check Variants {
            name == "剑";
            name.hasVariant("ja");
            !name.hasVariant("fr");
            !title.hasVariant("ja");
            title.for("ja") == "购买 剣";
            title.variants()["ja"] == "购买 剣";
            title.for("fr") == "购买 剑";
            note.hasVariant("ja");
            note.for("ja") == None;
            all variant, text in title.variants() { !text.isBlank(); }
        }
    }"#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        [
            ("name", "剑".into()),
            ("note", LoadedValueDraft::OptionNone),
        ],
    );
    add_overlay(&mut builder, "name", "ja", "剣".into());
    add_overlay(&mut builder, "note", "ja", LoadedValueDraft::OptionNone);
    builder.add_dimension_variant(
        DimensionName::new("language").unwrap(),
        VariantName::new("fr").unwrap(),
    );
    let model = builder.build().unwrap();
    let output = execute_checks(
        &schema,
        &model,
        base_tasks(&schema, &model),
        CheckLimits::default(),
    );
    assert!(output.is_success(), "{output:?}");
}

#[test]
fn dimension_reference_fallback_uses_the_base_reference_edge() {
    let schema = compile_schema(
        r#"table Reward { count: int; }
        table Item { @dimension("mode") reward: &Reward; check Reference { reward.for("hard").count > 0; all variant, item in reward.variants() { item.count > 0; } } }"#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("reward", "Reward", [("count", 1_i64.into())]);
    builder.add_record(
        "item",
        "Item",
        [("reward", LoadedValueDraft::record_ref("reward"))],
    );
    builder.add_dimension_variant(
        DimensionName::new("mode").unwrap(),
        VariantName::new("hard").unwrap(),
    );
    let model = builder.build().unwrap();
    let output = execute_checks(
        &schema,
        &model,
        base_tasks(&schema, &model),
        CheckLimits::default(),
    );
    assert!(output.is_success(), "{output:?}");
}
