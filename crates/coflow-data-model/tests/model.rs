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
    assert_eq!(model.lookup("Item", "item_1"), Some(item_id));

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
fn object_typed_record_refs_resolve_by_expected_type() {
    let schema = compile_schema(
        r#"
            abstract type Reward {}
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
            type Drop {
                reward: Reward;
                item_reward: ItemReward;
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
            (
                "reward",
                CfdInputValue::record_ref("ItemReward", "reward_1"),
            ),
            (
                "item_reward",
                CfdInputValue::record_ref("ItemReward", "reward_1"),
            ),
        ],
    );
    let model = builder.build().expect("data model should build");
    let reward_id = record_id_at(&model, 0);
    let drop_id = record_id_at(&model, 1);

    assert_eq!(model.lookup("Reward", "reward_1"), Some(reward_id));
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
        Some(&CfdValue::Ref {
            target_type: "ItemReward".to_string(),
            target_key: "reward_1".to_string(),
        })
    );
    let _ = reward_id;
}

#[test]
fn parent_record_keys_do_not_satisfy_child_typed_refs() {
    let schema = compile_schema(
        r#"
            type Base { name: string; }
            type Child : Base { power: int; }
            type Holder { child: Child; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("base_1", "Base", [("name", CfdInputValue::from("base"))]);
    builder.add_record(
        "holder_1",
        "Holder",
        [("child", CfdInputValue::record_ref("Base", "base_1"))],
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
    assert_eq!(stats.actual_type, "Stats");
    assert_eq!(stats.key(), "");
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
fn plain_object_fields_still_accept_record_refs_until_data_model_simplification() {
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
            ("item", CfdInputValue::record_ref("Item", "sword")),
            (
                "items",
                CfdInputValue::Array(vec![CfdInputValue::record_ref("Item", "sword")]),
            ),
            (
                "by_name",
                CfdInputValue::dict([(
                    CfdInputDictKey::from("main"),
                    CfdInputValue::record_ref("Item", "sword"),
                )]),
            ),
        ],
    );

    let model = builder
        .build()
        .expect("plain object fields still accept record refs before Phase 3");
    let holder_id = model.lookup("Holder", "holder").expect("holder");
    let holder = model.record(holder_id).expect("holder record");
    assert!(matches!(
        holder.field("item"),
        Some(CfdValue::Ref {
            target_type,
            target_key,
        }) if target_type == "Item" && target_key == "sword"
    ));
}

#[test]
fn singleton_fields_accept_refs_in_nested_shapes_and_reject_inline_objects() {
    let schema = compile_schema(
        r#"
            @singleton
            type GameConfig { value: int; }

            type Holder {
                config: GameConfig;
                optional: GameConfig? = null;
                configs: [GameConfig];
                by_name: {string: GameConfig};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "main",
        "GameConfig",
        [("value", CfdInputValue::from(1_i64))],
    );
    builder.add_record(
        "holder",
        "Holder",
        [
            ("config", CfdInputValue::record_ref("GameConfig", "main")),
            ("optional", CfdInputValue::Null),
            (
                "configs",
                CfdInputValue::Array(vec![CfdInputValue::record_ref("GameConfig", "main")]),
            ),
            (
                "by_name",
                CfdInputValue::dict([(
                    CfdInputDictKey::from("default"),
                    CfdInputValue::record_ref("GameConfig", "main"),
                )]),
            ),
        ],
    );

    let model = builder
        .build()
        .expect("singleton refs should be valid in fields, arrays, and dicts");
    let holder_id = model.lookup("Holder", "holder").expect("holder");
    let holder = model.record(holder_id).expect("holder record");
    assert!(matches!(
        holder.field("config"),
        Some(CfdValue::Ref {
            target_type,
            target_key,
        }) if target_type == "GameConfig" && target_key == "main"
    ));

    let mut inline_builder = CfdDataModel::builder(&schema);
    inline_builder.add_record(
        "main",
        "GameConfig",
        [("value", CfdInputValue::from(1_i64))],
    );
    inline_builder.add_record(
        "bad",
        "Holder",
        [
            (
                "config",
                CfdInputValue::object_with_declared_type([("value", CfdInputValue::from(2_i64))]),
            ),
            ("configs", CfdInputValue::Array(Vec::new())),
            ("by_name", CfdInputValue::dict(std::iter::empty())),
        ],
    );

    let err = inline_builder
        .build()
        .expect_err("singleton fields should reject inline objects");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
    assert!(
        err.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("singleton")),
        "expected singleton diagnostic, got {err:?}"
    );
}

#[test]
fn object_spread_merges_path_refs_before_local_overrides() {
    let schema = compile_schema(
        r#"
            type Stats { hp: int; attack: int; }
            type Monster { name: string; stats: Stats; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "base",
        "Monster",
        [
            ("name", CfdInputValue::from("Base")),
            (
                "stats",
                CfdInputValue::object_with_declared_type([
                    ("hp", CfdInputValue::from(100_i64)),
                    ("attack", CfdInputValue::from(20_i64)),
                ]),
            ),
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
                    [CfdInputValue::path_ref(
                        "Monster",
                        "base",
                        [CfdRefPathSegment::Field("stats".to_string())],
                    )],
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
fn object_path_spread_source_must_be_assignable_to_destination_type() {
    let schema = compile_schema(
        r#"
            type A { value: int; }
            type B { value: int; }
            type Wrapper { a: A; }
            type Holder { b: B; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "source",
        "Wrapper",
        [(
            "a",
            CfdInputValue::object_with_declared_type([("value", CfdInputValue::from(1_i64))]),
        )],
    );
    builder.add_record(
        "holder",
        "Holder",
        [(
            "b",
            CfdInputValue::object_spread(
                [CfdInputValue::path_ref(
                    "Wrapper",
                    "source",
                    [CfdRefPathSegment::Field("a".to_string())],
                )],
                std::iter::empty::<(&str, CfdInputValue)>(),
            ),
        )],
    );

    let err = builder
        .build()
        .expect_err("unrelated object type must not satisfy object spread");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
}

#[test]
fn dict_spread_merges_path_refs_before_local_overrides() {
    let schema = compile_schema(
        r#"
            enum Element { Fire, Ice, }
            type Table { weights: {Element: int}; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "base",
        "Table",
        [(
            "weights",
            CfdInputValue::dict([
                (
                    CfdInputDictKey::enum_variant("Element", "Fire"),
                    CfdInputValue::from(10_i64),
                ),
                (
                    CfdInputDictKey::enum_variant("Element", "Ice"),
                    CfdInputValue::from(5_i64),
                ),
            ]),
        )],
    );
    builder.add_record(
        "elite",
        "Table",
        [(
            "weights",
            CfdInputValue::dict_spread(
                [CfdInputValue::path_ref(
                    "Table",
                    "base",
                    [CfdRefPathSegment::Field("weights".to_string())],
                )],
                [(
                    CfdInputDictKey::enum_variant("Element", "Fire"),
                    CfdInputValue::from(20_i64),
                )],
            ),
        )],
    );

    let model = builder.build().expect("dict spread should build");
    let elite_id = record_id_at(&model, 1);
    let Some(CfdValue::Dict(weights)) = model
        .record(elite_id)
        .and_then(|record| record.field("weights"))
    else {
        panic!("expected dict");
    };
    assert_eq!(weights.len(), 2);
    assert!(weights.iter().any(|(_, value)| value == &CfdValue::Int(20)));
    assert!(weights.iter().any(|(_, value)| value == &CfdValue::Int(5)));
}

#[test]
fn path_refs_can_index_dict_spread_results_and_continue_to_object_fields() {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type Table { by_name: {string: Item}; }
            type Holder { label: string; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "base",
        "Table",
        [(
            "by_name",
            CfdInputValue::dict([(
                CfdInputDictKey::from("main"),
                CfdInputValue::object_with_declared_type([(
                    "name",
                    CfdInputValue::from("Base Sword"),
                )]),
            )]),
        )],
    );
    builder.add_record(
        "merged",
        "Table",
        [(
            "by_name",
            CfdInputValue::dict_spread(
                [CfdInputValue::path_ref(
                    "Table",
                    "base",
                    [CfdRefPathSegment::Field("by_name".to_string())],
                )],
                std::iter::empty::<(CfdInputDictKey, CfdInputValue)>(),
            ),
        )],
    );
    builder.add_record(
        "holder",
        "Holder",
        [(
            "label",
            CfdInputValue::path_ref(
                "Table",
                "merged",
                [
                    CfdRefPathSegment::Field("by_name".to_string()),
                    CfdRefPathSegment::Index(CfdInputRefIndex::String("main".to_string())),
                    CfdRefPathSegment::Field("name".to_string()),
                ],
            ),
        )],
    );

    let model = builder
        .build()
        .expect("path refs should traverse dict spread object values");
    let holder_id = record_id_at(&model, 2);
    let holder = model.record(holder_id).expect("holder record");
    assert_eq!(
        holder.field("label"),
        Some(&CfdValue::String("Base Sword".to_string()))
    );
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
