#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

mod common;
use common::*;

#[test]
fn data_model_applies_defaults_and_builds_record_key_indexes_without_running_check() {
    let schema = compile_schema(
        r#"
            const DEFAULT_NAME = "unknown";
            enum Rarity { Common = 0, Rare = 10, }
            type Item {
                name: string = DEFAULT_NAME;
                rarity: Rarity = Rarity.Common;
                tags: [string] = [];
                attrs: {string: int} = {};
                check { id != ""; }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item_1",
        "Item",
        std::iter::empty::<(&str, CfdInputValue)>(),
    );
    let model = builder.build().expect("data model should build");

    let item_id = record_id_at(&model, 0);
    let table = model.table("Item").expect("item table");
    assert_eq!(table.records, vec![item_id]);
    assert_eq!(table.primary_index.get("item_1"), Some(&item_id));
    assert_eq!(model.lookup_assignable("Item", "item_1"), Some(item_id));

    let record = model.record(item_id).expect("record");
    assert_eq!(record.key(), "item_1");
    assert_eq!(
        record.field("name"),
        Some(&CfdValue::String("unknown".to_string()))
    );
    assert_eq!(
        record.field("rarity"),
        Some(&CfdValue::Enum(CfdEnumValue {
            enum_name: "Rarity".to_string(),
            variant: Some("Common".to_string()),
            value: 0,
        }))
    );
    assert_eq!(record.field("tags"), Some(&CfdValue::Array(Vec::new())));
    assert_eq!(record.field("attrs"), Some(&CfdValue::Dict(Vec::new())));
}

#[test]
fn data_model_reports_direct_schema_default_cycle() {
    let schema = compile_schema("type Node { child: Node = {}; }");
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("root", "Node", std::iter::empty::<(&str, CfdInputValue)>());

    let err = builder.build().expect_err("default cycle must be rejected");
    let diagnostic = diagnostic_with_code(&err, CfdErrorCode::ValueDependencyCycle);
    assert_eq!(
        diagnostic.message,
        "schema default dependency cycle: Node.child -> Node"
    );
    assert_eq!(
        primary_path_segments(diagnostic),
        [CfdPathSegment::Field("child".to_string())]
    );
}

#[test]
fn data_model_reports_indirect_schema_default_cycle() {
    let schema = compile_schema(
        r#"
            type A { b: B = {}; }
            type B { c: C = {}; }
            type C { a: A = {}; }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("root", "A", std::iter::empty::<(&str, CfdInputValue)>());

    let err = builder.build().expect_err("default cycle must be rejected");
    let diagnostic = diagnostic_with_code(&err, CfdErrorCode::ValueDependencyCycle);
    assert_eq!(
        diagnostic.message,
        "schema default dependency cycle: A.b -> B.c -> C.a -> A"
    );
}

#[test]
fn data_model_reuses_shared_schema_default_subgraphs() {
    let schema = compile_schema(
        r#"
            type Leaf { value: int = 7; }
            type Branch { leaf: Leaf = {}; }
            type Root { left: Branch = {}; right: Branch = {}; }
        "#,
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("root", "Root", std::iter::empty::<(&str, CfdInputValue)>());

    let model = builder.build().expect("shared default graph builds");
    let root = model
        .records()
        .next()
        .map(|(_, record)| record)
        .expect("root record");
    for field in ["left", "right"] {
        let Some(CfdValue::Object(branch)) = root.field(field) else {
            panic!("{field} branch");
        };
        let Some(CfdValue::Object(leaf)) = branch.field("leaf") else {
            panic!("leaf object");
        };
        assert_eq!(leaf.field("value"), Some(&CfdValue::Int(7)));
    }
}

#[test]
fn data_model_reports_direct_spread_dependency_cycle() {
    let schema = compile_schema("type Stats { hp: int; }");
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::with_spreads(
        "self_ref",
        "Stats",
        [CfdInputValue::record_ref("self_ref")],
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));

    let err = builder.build().expect_err("spread cycle must be rejected");
    let diagnostic = diagnostic_with_code(&err, CfdErrorCode::ValueDependencyCycle);
    assert_eq!(
        diagnostic.message,
        "data spread dependency cycle: Stats.self_ref.hp -> Stats.self_ref.hp"
    );
    assert_eq!(
        primary_path_segments(diagnostic),
        [CfdPathSegment::Field("hp".to_string())]
    );
}

#[test]
fn data_model_reports_one_canonical_indirect_spread_cycle() {
    let schema = compile_schema("type Stats { hp: int; }");
    let mut builder = CfdDataModel::builder(&schema);
    for (key, source) in [("a", "b"), ("b", "c"), ("c", "a")] {
        builder.add_input_record(CfdInputRecord::with_spreads(
            key,
            "Stats",
            [CfdInputValue::record_ref(source)],
            std::iter::empty::<(&str, CfdInputValue)>(),
        ));
    }

    let err = builder.build().expect_err("spread cycle must be rejected");
    let cycles = err
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == CfdErrorCode::ValueDependencyCycle)
        .collect::<Vec<_>>();
    assert_eq!(cycles.len(), 1, "the same cycle should be reported once");
    assert_eq!(
        cycles[0].message,
        "data spread dependency cycle: Stats.a.hp -> Stats.b.hp -> Stats.c.hp -> Stats.a.hp"
    );
    assert_eq!(cycles[0].related.len(), 2);
}

#[test]
fn data_model_resolves_shared_spread_source_for_multiple_consumers() {
    let schema = compile_schema("type Stats { hp: int; }");
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("base", "Stats", [("hp", CfdInputValue::from(7_i64))]);
    for key in ["left", "right"] {
        builder.add_input_record(CfdInputRecord::with_spreads(
            key,
            "Stats",
            [CfdInputValue::record_ref("base")],
            std::iter::empty::<(&str, CfdInputValue)>(),
        ));
    }

    let model = builder.build().expect("shared spread source resolves");
    for key in ["left", "right"] {
        let record = model
            .lookup_assignable("Stats", key)
            .and_then(|id| model.record(id));
        assert_eq!(
            record.and_then(|record| record.field("hp")),
            Some(&CfdValue::Int(7))
        );
    }
}

#[test]
fn dimension_field_lookup_reads_record_owned_overlay() {
    let schema = compile_schema_with_dimensions(
        r#"
            type Item {
                @dimension("platform")
                name: string;
            }
        "#,
        CftDimensionInputs::try_new([("platform", vec!["pc".to_string()])])
            .expect("valid dimension fixture"),
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("potion", "Item", [("name", CfdInputValue::from("Potion"))]);
    builder.add_input_dimension_value(CfdInputDimensionValue {
        source_type: coflow_cft::TypeName::new("Item").unwrap(),
        source_key: coflow_cft::RecordKey::new("potion").unwrap(),
        field: coflow_cft::FieldName::new("name").unwrap(),
        dimension: coflow_cft::DimensionName::new("platform").unwrap(),
        variant: coflow_cft::VariantName::new("pc").unwrap(),
        value: CfdInputValue::from("PC Potion"),
        origin: RecordOrigin::None,
    });
    let model = builder.build().expect("data model should build");
    let item_id = model.lookup_assignable("Item", "potion").expect("item");

    let resolved = model
        .dimension_field_value(&schema, item_id, "name", "platform", "pc")
        .expect("variant lookup should resolve");

    assert!(matches!(
        resolved,
        DimensionValueLookup::Value {
            value: CfdValue::String(value),
            origin: RecordOrigin::None,
        } if value == "PC Potion"
    ));
    assert_eq!(model.record_count(), 1);
}

#[test]
fn dimension_refs_are_precomputed_with_typed_coordinates() {
    let schema = compile_schema_with_dimensions(
        r#"
            type Item { name: string; }
            type Offer {
                @dimension("platform")
                item: &Item;
            }
        "#,
        CftDimensionInputs::try_new([("platform", vec!["pc".to_string()])])
            .expect("valid dimension fixture"),
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("potion", "Item", [("name", CfdInputValue::from("Potion"))]);
    builder.add_record(
        "starter",
        "Offer",
        [("item", CfdInputValue::record_ref("potion"))],
    );
    builder.add_input_dimension_value(CfdInputDimensionValue {
        source_type: coflow_cft::TypeName::new("Offer").unwrap(),
        source_key: coflow_cft::RecordKey::new("starter").unwrap(),
        field: coflow_cft::FieldName::new("item").unwrap(),
        dimension: coflow_cft::DimensionName::new("platform").unwrap(),
        variant: coflow_cft::VariantName::new("pc").unwrap(),
        value: CfdInputValue::record_ref("potion"),
        origin: RecordOrigin::None,
    });

    let model = builder.build().expect("data model should build");
    let offer = model.lookup_assignable("Offer", "starter").expect("offer");
    let item = model.lookup_assignable("Item", "potion").expect("item");
    let edges = model.direct_ref_edges_from_host(offer).collect::<Vec<_>>();

    assert_eq!(edges.len(), 2);
    assert!(edges.iter().any(|edge| edge.site.dimension.is_none()));
    let overlay = edges
        .iter()
        .find(|edge| edge.site.dimension.is_some())
        .expect("overlay ref edge");
    let coordinate = overlay.site.dimension.as_ref().unwrap();
    assert_eq!(coordinate.field.as_str(), "item");
    assert_eq!(coordinate.dimension.as_str(), "platform");
    assert_eq!(coordinate.variant.as_str(), "pc");
    assert_eq!(overlay.target, item);
    assert_eq!(model.direct_ref_edges_to_target(item).count(), 2);
}

#[test]
fn dimension_field_lookup_uses_singleton_owner_record() {
    let schema = compile_schema_with_dimensions(
        r#"
            @singleton
            type UiText {
                @localized
                welcome: string;
            }
        "#,
        CftDimensionInputs::try_new([("language", vec!["zh".to_string()])])
            .expect("valid dimension fixture"),
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "UiText",
        "UiText",
        [("welcome", CfdInputValue::from("Welcome"))],
    );
    builder.add_input_dimension_value(CfdInputDimensionValue {
        source_type: coflow_cft::TypeName::new("UiText").unwrap(),
        source_key: coflow_cft::RecordKey::new("UiText").unwrap(),
        field: coflow_cft::FieldName::new("welcome").unwrap(),
        dimension: coflow_cft::DimensionName::new("language").unwrap(),
        variant: coflow_cft::VariantName::new("zh").unwrap(),
        value: CfdInputValue::from("欢迎"),
        origin: RecordOrigin::None,
    });
    let model = builder.build().expect("data model should build");
    let singleton_id = model
        .lookup_assignable("UiText", "UiText")
        .expect("singleton");

    let resolved = model
        .dimension_field_value(&schema, singleton_id, "welcome", "language", "zh")
        .expect("singleton variant lookup should resolve by field name");

    assert!(matches!(
        resolved,
        DimensionValueLookup::Value {
            value: CfdValue::String(value),
            ..
        } if value == "欢迎"
    ));
}

#[test]
fn object_typed_record_refs_resolve_by_expected_type() {
    let schema = compile_schema(
        r#"
            abstract type Reward {}
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
            type Drop {
                reward: &Reward;
                item_reward: &ItemReward;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "reward_1",
        "ItemReward",
        [("count", CfdInputValue::from(1_i64))],
    );
    builder.add_record(
        "drop_1",
        "Drop",
        [
            ("reward", CfdInputValue::record_ref("reward_1")),
            ("item_reward", CfdInputValue::record_ref("reward_1")),
        ],
    );
    let model = builder.build().expect("data model should build");
    let reward_id = record_id_at(&model, 0);
    let drop_id = record_id_at(&model, 1);

    assert_eq!(
        model.lookup_assignable("Reward", "reward_1"),
        Some(reward_id)
    );
    assert_eq!(
        model
            .polymorphic_index("Reward")
            .expect("reward index")
            .records
            .get("reward_1"),
        Some(&reward_id)
    );
    assert_eq!(
        model
            .record(drop_id)
            .and_then(|record| record.field("reward")),
        Some(&CfdValue::Ref("reward_1".to_string()))
    );
    let _ = reward_id;
}

#[test]
fn parent_record_keys_do_not_satisfy_child_typed_refs() {
    let schema = compile_schema(
        r#"
            type Base { name: string; }
            type Child : Base { power: int; }
            type Holder { child: &Child; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("base_1", "Base", [("name", CfdInputValue::from("base"))]);
    builder.add_record(
        "holder_1",
        "Holder",
        [("child", CfdInputValue::record_ref("base_1"))],
    );

    let err = builder
        .build()
        .expect_err("parent-typed ref should not satisfy child field");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
}

#[test]
fn inline_objects_use_declared_type_when_not_polymorphic() {
    let schema = compile_schema(
        r#"
            type Stats { hp: int; speed: float = 1.0; }
            type Monster { stats: Stats; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "monster_1",
        "Monster",
        [(
            "stats",
            CfdInputValue::object_with_declared_type([("hp", CfdInputValue::from(100_i64))]),
        )],
    );
    let model = builder.build().expect("data model should build");
    let monster_id = record_id_at(&model, 0);
    let Some(CfdValue::Object(stats)) = model
        .record(monster_id)
        .and_then(|record| record.field("stats"))
    else {
        panic!("expected stats object");
    };
    assert_eq!(stats.actual_type(), "Stats");
    assert_eq!(stats.field("speed"), Some(&CfdValue::Float(1.0)));
}

#[test]
fn ref_type_rejects_inline_objects() {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type Holder {
                item: &Item;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "holder",
        "Holder",
        [(
            "item",
            CfdInputValue::object_with_declared_type([("name", CfdInputValue::from("Sword"))]),
        )],
    );

    let err = builder
        .build()
        .expect_err("ref type should reject inline object");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
    assert!(
        err.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("&Item")),
        "expected ref type diagnostic, got {err:?}"
    );
}

#[test]
fn plain_object_fields_reject_record_refs() {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type Holder {
                item: Item;
                items: [Item];
                by_name: {string: Item};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("sword", "Item", [("name", CfdInputValue::from("Sword"))]);
    builder.add_record(
        "holder",
        "Holder",
        [
            ("item", CfdInputValue::record_ref("sword")),
            (
                "items",
                CfdInputValue::Array(vec![CfdInputValue::record_ref("sword")]),
            ),
            (
                "by_name",
                CfdInputValue::dict([(
                    CfdInputDictKey::from("main"),
                    CfdInputValue::record_ref("sword"),
                )]),
            ),
        ],
    );

    let err = builder
        .build()
        .expect_err("plain object fields should reject record refs");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
}

#[test]
fn object_spread_merges_record_refs_before_local_overrides() {
    let schema = compile_schema(
        r#"
            type Stats { hp: int; attack: int; }
            type Monster { name: string; stats: Stats; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "base",
        "Stats",
        [
            ("hp", CfdInputValue::from(100_i64)),
            ("attack", CfdInputValue::from(20_i64)),
        ],
    );
    builder.add_record(
        "elite",
        "Monster",
        [
            ("name", CfdInputValue::from("Elite")),
            (
                "stats",
                CfdInputValue::object_spread(
                    [CfdInputValue::record_ref("base")],
                    [("hp", CfdInputValue::from(180_i64))],
                ),
            ),
        ],
    );

    let model = builder.build().expect("spread should build");
    let elite_id = record_id_at(&model, 1);
    let Some(CfdValue::Object(stats)) = model
        .record(elite_id)
        .and_then(|record| record.field("stats"))
    else {
        panic!("expected stats object");
    };
    assert_eq!(stats.field("hp"), Some(&CfdValue::Int(180)));
    assert_eq!(stats.field("attack"), Some(&CfdValue::Int(20)));
}

#[test]
fn semantic_edges_report_data_model_diagnostics() {
    let schema = compile_schema(
        r#"
            enum Rarity { Common, Rare, }
            type Item {
                rarity: Rarity;
                maybe: int?;
                attrs: {string: int};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item_1",
        "Item",
        [
            ("unknown", CfdInputValue::from(1_i64)),
            ("rarity", CfdInputValue::enum_variant("Rarity", "Missing")),
            (
                "attrs",
                CfdInputValue::dict([
                    (CfdInputDictKey::from("x"), CfdInputValue::from(1_i64)),
                    (CfdInputDictKey::from("x"), CfdInputValue::from(2_i64)),
                ]),
            ),
        ],
    );

    let err = builder.build().expect_err("data errors");
    assert_has_code(&err, CfdErrorCode::UnknownField);
    assert_has_code(&err, CfdErrorCode::MissingRequiredField);
    assert_has_code(&err, CfdErrorCode::InvalidEnumVariant);
    assert_has_code(&err, CfdErrorCode::DuplicateDictKey);
}

#[test]
fn duplicate_keys_report_record_level_id_paths() {
    let schema = compile_schema("type Item { value: int; }");

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("same", "Item", [("value", CfdInputValue::from(1_i64))]);
    builder.add_record("same", "Item", [("value", CfdInputValue::from(2_i64))]);

    let err = builder.build().expect_err("duplicate concrete key");
    let diag = diagnostic_with_code(&err, CfdErrorCode::DuplicateId);
    assert_eq!(
        diag.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("id"))
    );
    assert_eq!(diag.related.len(), 1);
}

#[test]
fn empty_record_keys_are_rejected() {
    let schema = compile_schema("type Item { value: int; }");

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("", "Item", [("value", CfdInputValue::from(1_i64))]);

    let err = builder.build().expect_err("empty key should fail");
    let diag = diagnostic_with_code(&err, CfdErrorCode::MissingIdField);
    assert_eq!(
        diag.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("id"))
    );
}

#[test]
fn non_identifier_record_keys_are_rejected() {
    let schema = compile_schema("type Item { value: int; }");

    for key in ["123", "fire-ball", "fire.ball", "type"] {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(key, "Item", [("value", CfdInputValue::from(1_i64))]);

        let Err(err) = builder.build() else {
            panic!("`{key}` should fail as a record key")
        };
        assert_has_code(&err, CfdErrorCode::InvalidRecordKey);
    }
}

#[test]
fn record_value_lookup_traverses_object_array_and_dict_paths() {
    let schema = compile_schema(
        r#"
            type Nested { value: int; }
            type Item {
                nested: Nested;
                items: [Nested];
                entries: {string: Nested};
            }
        "#,
    );
    let nested =
        |value| CfdInputValue::object_with_declared_type([("value", CfdInputValue::from(value))]);
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item",
        "Item",
        [
            ("nested", nested(1_i64)),
            ("items", CfdInputValue::Array(vec![nested(2_i64)])),
            (
                "entries",
                CfdInputValue::dict([(CfdInputDictKey::from("slot"), nested(3_i64))]),
            ),
        ],
    );
    let model = builder.build().expect("model builds");
    let record = model
        .record(record_id_at(&model, 0))
        .expect("record exists");

    assert_eq!(
        record.value_at_path(&CfdPath::root().field("nested").field("value")),
        Some(&CfdValue::Int(1))
    );
    assert_eq!(
        record.value_at_path(&CfdPath::root().field("items").index(0).field("value")),
        Some(&CfdValue::Int(2))
    );
    assert_eq!(
        record.value_at_path(
            &CfdPath::root()
                .field("entries")
                .dict_key_value(&CfdDictKey::String("slot".to_string()))
                .field("value")
        ),
        Some(&CfdValue::Int(3))
    );
    assert_eq!(
        record.value_at_path(&CfdPath::root().field("items").index(1)),
        None
    );
}
