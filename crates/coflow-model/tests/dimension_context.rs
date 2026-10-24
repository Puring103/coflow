#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

#[path = "data_model_common/mod.rs"]
mod common;
use common::*;

fn value(schema: &CftSchema, ty: &str, source: &str) -> LoadedValueDraft {
    let cell_value::ParsedCell::Value(value) = cell_value::parse_cell(schema, ty, source).unwrap()
    else {
        panic!("expected value");
    };
    value
}

fn overlay(
    ty: &str,
    id: &str,
    field: &str,
    dimension: &str,
    variant: &str,
    value: LoadedValueDraft,
) -> DimensionValueDraft {
    DimensionValueDraft {
        source_type: TypeName::new(ty).unwrap(),
        source_id: RecordId::new(id).unwrap(),
        field: FieldName::new(field).unwrap(),
        dimension: DimensionName::new(dimension).unwrap(),
        variant: VariantName::new(variant).unwrap(),
        value,
        origin: RecordOrigin::None,
    }
}

fn rendered(value: &CfdValue) -> &str {
    match value {
        CfdValue::String(value) => value,
        CfdValue::FormattedString(value) => &value.rendered,
        _ => panic!("expected string"),
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "按语言或命令分支集中处理，保持分派与各分支语义可直接核对。"
)]
fn variant_context_selects_sources_across_references_objects_and_collections() {
    let schema = compile_schema(
        r#"
        data Bundle { target: &Item; text: string = f"{self.target.name}"; }
        table Item {
            @localized name: string;
            @dimension("platform") platform: string;
            @localized title: string = f"{self.name}/{self.platform}";
            @localized optional: Option<string> = None;
        }
        table Offer {
            target: &Item;
            @localized title: string = f"{self.target.title}";
            @localized bundle: Bundle;
            @localized texts: [string];
        }
    "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "sword",
        "Item",
        [("name", "Sword".into()), ("platform", "Base".into())],
    );
    builder.add_record(
        "offer",
        "Offer",
        [
            ("target", LoadedValueDraft::record_ref("sword")),
            ("bundle", value(&schema, "Bundle", "{ target: &sword }")),
            (
                "texts",
                value(
                    &schema,
                    "[string]",
                    "[f\"{&Item::sword.name}\" | \"literal {self.target}\"]",
                ),
            ),
        ],
    );
    builder.add_dimension_value_draft(overlay(
        "Item",
        "sword",
        "name",
        "language",
        "ja",
        "剣".into(),
    ));
    builder.add_dimension_value_draft(overlay(
        "Item",
        "sword",
        "platform",
        "platform",
        "ja",
        "Console".into(),
    ));
    builder.add_dimension_value_draft(overlay(
        "Item",
        "sword",
        "optional",
        "language",
        "ja",
        LoadedValueDraft::OptionNone,
    ));
    builder.add_dimension_variant(
        DimensionName::new("language").unwrap(),
        VariantName::new("default").unwrap(),
    );
    let model = builder.build().expect("context model");
    let item = model.lookup_assignable(&schema, "Item", "sword").unwrap();
    let offer = model.lookup_assignable(&schema, "Offer", "offer").unwrap();
    assert_eq!(
        rendered(model.record(item).unwrap().field("title").unwrap()),
        "Sword/Base"
    );
    assert_eq!(
        rendered(model.dimension_value(item, "title", "ja").unwrap()),
        "剣/Base"
    );
    assert_eq!(
        rendered(model.dimension_value(offer, "title", "ja").unwrap()),
        "剣/Base"
    );
    assert_eq!(
        rendered(model.dimension_value(item, "title", "default").unwrap()),
        "Sword/Base"
    );
    assert!(!model.has_dimension_variant(item, "title", "ja"));
    assert!(model.has_dimension_variant(item, "optional", "ja"));
    assert!(!model.has_dimension_variant(item, "optional", "default"));
    let CfdValue::Object(bundle) = model.dimension_value(offer, "bundle", "ja").unwrap() else {
        panic!("bundle")
    };
    assert_eq!(rendered(bundle.field("text").unwrap()), "剣");
    let CfdValue::Array(texts) = model.dimension_value(offer, "texts", "ja").unwrap() else {
        panic!("texts")
    };
    assert_eq!(rendered(&texts[0]), "剣");
    assert_eq!(rendered(&texts[1]), "literal {self.target}");
    assert_eq!(
        model
            .record(item)
            .unwrap()
            .dimension_field("title")
            .unwrap()
            .effective_variants
            .len(),
        2
    );
}

#[test]
fn cycles_introduced_by_a_variant_are_reported() {
    let schema = compile_schema(
        r#"
        table Item {
            @localized name: string;
            @localized title: string = f"{self.name}";
        }
    "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("sword", "Item", [("name", "Sword".into())]);
    builder.add_dimension_value_draft(overlay(
        "Item",
        "sword",
        "name",
        "language",
        "ja",
        value(&schema, "string", "f\"{self.title}\""),
    ));
    let diagnostics = builder.build().expect_err("variant dependency cycle");
    assert!(diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("cycle")));
}

#[test]
fn chained_projections_reselect_original_sources_and_preserve_other_dimensions() {
    use std::collections::BTreeMap;

    let schema = compile_schema(
        r#"
        table Item {
            @localized name: string;
            @dimension("platform") platform: string;
            title: string = f"{self.name}/{self.platform}";
        }
        table Offer { target: &Item; title: string = f"{self.target.title}"; }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "sword",
        "Item",
        [("name", "Sword".into()), ("platform", "Desktop".into())],
    );
    builder.add_record(
        "offer",
        "Offer",
        [("target", LoadedValueDraft::record_ref("sword"))],
    );
    builder.add_dimension_value_draft(overlay(
        "Item",
        "sword",
        "name",
        "language",
        "ja",
        "剣".into(),
    ));
    builder.add_dimension_value_draft(overlay(
        "Item",
        "sword",
        "platform",
        "platform",
        "mobile",
        "Mobile".into(),
    ));
    builder.add_dimension_variant(
        DimensionName::new("language").unwrap(),
        VariantName::new("fr").unwrap(),
    );
    let model = builder.build().unwrap();
    let item = model.lookup_assignable(&schema, "Item", "sword").unwrap();
    let offer = model.lookup_assignable(&schema, "Offer", "offer").unwrap();
    let select = |model: &CfdDataModel, dimension: &str, variant: &str| {
        model
            .project_dimensions(
                &schema,
                &BTreeMap::from([(dimension.into(), variant.into())]),
            )
            .unwrap()
    };
    let ja = select(&model, "language", "ja");
    let combined = select(&ja, "platform", "mobile");
    let one_shot = model
        .project_dimensions(
            &schema,
            &BTreeMap::from([
                ("language".into(), "ja".into()),
                ("platform".into(), "mobile".into()),
            ]),
        )
        .unwrap();
    assert_eq!(combined, one_shot);
    assert_eq!(
        rendered(combined.record(offer).unwrap().field("title").unwrap()),
        "剣/Mobile"
    );
    assert_eq!(
        rendered(ja.record(offer).unwrap().field("title").unwrap()),
        "剣/Desktop"
    );
    for variant in ["fr", "unknown"] {
        let fallback = select(&combined, "language", variant);
        assert_eq!(
            rendered(fallback.record(offer).unwrap().field("title").unwrap()),
            "Sword/Mobile"
        );
        assert_eq!(
            rendered(combined.dimension_value(item, "name", variant).unwrap()),
            "Sword"
        );
    }
    assert!(combined.has_dimension_variant(item, "name", "ja"));
    assert_eq!(
        rendered(model.record(offer).unwrap().field("title").unwrap()),
        "Sword/Desktop"
    );
    assert!(model
        .project_dimensions(
            &schema,
            &BTreeMap::from([("unknown_dimension".into(), "ja".into()),])
        )
        .is_err());
}

#[test]
fn variant_reference_cycles_in_unannotated_templates_fail_during_loading() {
    let schema =
        compile_schema(r#"table Entry { @dimension("mode") next: &Entry; title: string; }"#);
    let mut builder = CfdDataModel::builder(&schema);
    let base_origin = RecordOrigin::File {
        path: "base.cfd".into(),
        span: None,
    };
    let override_origin = RecordOrigin::File {
        path: "loop.cfd".into(),
        span: None,
    };
    builder.add_loaded_record(
        LoadedRecordDraft::new(
            "a",
            "Entry",
            [
                ("next", LoadedValueDraft::record_ref("b")),
                ("title", value(&schema, "string", "f\"{self.next.title}\"")),
            ],
        )
        .with_origin(base_origin.clone()),
    );
    builder.add_record(
        "b",
        "Entry",
        [
            ("next", LoadedValueDraft::record_ref("b")),
            ("title", "B".into()),
        ],
    );
    let mut next = overlay(
        "Entry",
        "a",
        "next",
        "mode",
        "loop",
        LoadedValueDraft::record_ref("a"),
    );
    next.origin = override_origin;
    builder.add_dimension_value_draft(next);
    let diagnostics = builder
        .build()
        .expect_err("variant must validate unannotated template dependencies");
    let diagnostic = diagnostic_with_code(&diagnostics, CfdErrorCode::ValueDependencyCycle);
    assert!(diagnostic
        .message
        .contains("dimension `mode`, variant `loop`"));
    let primary = diagnostic.primary.as_ref().unwrap();
    assert_eq!(primary.path, CfdPath::root().field("title"));
    assert_eq!(primary.origin.as_ref(), Some(&base_origin));
}

#[test]
fn variant_validation_shares_one_structural_budget_per_record() {
    let schema = compile_schema(
        r#"table Item { @dimension("mode") first: [int]; @dimension("mode") second: [int]; }"#,
    );
    let build = |max_nodes| {
        let mut builder = CfdDataModel::builder(&schema)
            .with_structural_limits(StructuralLimits::new(10, max_nodes, 100));
        builder.add_record(
            "item",
            "Item",
            [
                ("first", LoadedValueDraft::Array(vec![])),
                ("second", LoadedValueDraft::Array(vec![])),
            ],
        );
        for field in ["first", "second"] {
            builder.add_dimension_value_draft(overlay(
                "Item",
                "item",
                field,
                "mode",
                "full",
                LoadedValueDraft::Array(vec![1_i64.into(), 2_i64.into()]),
            ));
        }
        builder.build()
    };
    build(7).expect("one root, two arrays and four items fit seven nodes");
    let diagnostics = build(6).expect_err("each dimensional field must share the record budget");
    let diagnostic = diagnostic_with_code(&diagnostics, CfdErrorCode::DataStructureLimitExceeded);
    assert!(diagnostic
        .message
        .contains("dimension `mode`, variant `full`"));
    assert_eq!(
        diagnostic.primary.as_ref().unwrap().path,
        CfdPath::root().field("second").index(1)
    );
}

#[test]
fn model_snapshots_and_repeated_dimension_selections_share_immutable_records() {
    use std::collections::BTreeMap;
    let schema = compile_schema(r"table Item { @localized name: string; }");
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("sword", "Item", [("name", "Sword".into())]);
    builder.add_dimension_value_draft(overlay(
        "Item",
        "sword",
        "name",
        "language",
        "ja",
        "剣".into(),
    ));
    let model = builder.build().unwrap();
    let record = model.lookup_assignable(&schema, "Item", "sword").unwrap();
    let snapshot = model.clone();
    assert!(std::ptr::eq(
        model.record(record).unwrap(),
        snapshot.record(record).unwrap()
    ));
    let selection = BTreeMap::from([("language".into(), "ja".into())]);
    let selected = model.project_dimensions(&schema, &selection).unwrap();
    let repeated = selected.project_dimensions(&schema, &selection).unwrap();
    assert!(std::ptr::eq(
        selected.record(record).unwrap(),
        repeated.record(record).unwrap()
    ));
    assert_eq!(
        rendered(model.record(record).unwrap().field("name").unwrap()),
        "Sword"
    );
    assert_eq!(
        rendered(repeated.record(record).unwrap().field("name").unwrap()),
        "剣"
    );
}
