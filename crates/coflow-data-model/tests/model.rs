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
            key: "reward_1".to_string(),
            target: reward_id,
        })
    );
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

        let err = match builder.build() {
            Ok(_) => panic!("`{key}` should fail as a record key"),
            Err(err) => err,
        };
        assert_has_code(&err, CfdErrorCode::InvalidRecordKey);
    }
}
