mod common;
use common::*;
use std::collections::BTreeMap;

#[test]
fn data_model_applies_defaults_and_builds_indexes_without_running_check() {
    let schema = compile_schema(
        r#"
            const DEFAULT_NAME = "unknown";
            enum Rarity { Common = 0, Rare = 10, }
            type Item {
                @id
                id: string;
                name: string = DEFAULT_NAME;
                @index
                rarity: Rarity = Rarity.Common;
                tags: [string] = [];
                attrs: {string: int} = {};
                check { id != ""; }
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from(""))]);
    let model = builder.build().expect("data model should build");

    let item_id = record_id_at(&model, 0);
    let table = model.table("Item").expect("item table");
    assert_eq!(table.records, vec![item_id]);
    assert!(table.primary_index.contains_key(&CfdIdValue::from("")));
    assert!(
        table.secondary_indexes["rarity"].contains_key(&CfdIndexKey::Enum(CfdEnumValue {
            enum_name: "Rarity".to_string(),
            variant: "Common".to_string(),
            value: 0,
        }))
    );

    let record = model.record(item_id).expect("record");
    assert_eq!(
        record.field("name"),
        Some(&CfdValue::String("unknown".to_string()))
    );
    assert_eq!(record.field("tags"), Some(&CfdValue::Array(Vec::new())));
    assert_eq!(
        record.field("attrs"),
        Some(&CfdValue::Dict(BTreeMap::new()))
    );
}

#[test]
fn polymorphic_refs_resolve_against_the_data_model() {
    let schema = compile_schema(
        r#"
            abstract type Reward { @id id: string; }
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
            type Drop {
                @ref(Reward)
                reward_id: string;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "ItemReward",
        [
            ("id", CfdInputValue::from("reward_1")),
            ("count", CfdInputValue::from(1_i64)),
        ],
    );
    builder.add_record(
        "Drop",
        [(
            "reward_id",
            CfdInputValue::Ref(CfdIdValue::from("reward_1")),
        )],
    );
    let model = builder.build().expect("data model should build");
    let item_reward_id = record_id_at(&model, 0);
    let drop_id = record_id_at(&model, 1);

    assert!(model
        .polymorphic_index("Reward")
        .unwrap()
        .records
        .contains_key(&CfdIdValue::from("reward_1")));
    assert_eq!(
        model
            .record(drop_id)
            .and_then(|record| record.field("reward_id")),
        Some(&CfdValue::Ref {
            id: CfdIdValue::from("reward_1"),
            target: item_reward_id,
        })
    );
}

#[test]
fn ref_field_defaults_are_resolved_as_references() {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Drop {
                @ref(Item)
                item_id: string = "default_item";
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("default_item"))]);
    builder.add_input_record(CfdInputRecord::new(
        "Drop",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    let model = builder.build().expect("data model should build");
    let item_id = record_id_at(&model, 0);
    let drop_id = record_id_at(&model, 1);

    assert_eq!(
        model
            .record(drop_id)
            .and_then(|record| record.field("item_id")),
        Some(&CfdValue::Ref {
            id: CfdIdValue::from("default_item"),
            target: item_id,
        })
    );
}

#[test]
fn duplicate_ids_are_checked_inside_polymorphic_ranges() {
    let schema = compile_schema(
        r#"
            abstract type Reward { @id id: string; }
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "ItemReward",
        [
            ("id", CfdInputValue::from("same")),
            ("count", CfdInputValue::from(1_i64)),
        ],
    );
    builder.add_record(
        "CurrencyReward",
        [
            ("id", CfdInputValue::from("same")),
            ("amount", CfdInputValue::from(10_i64)),
        ],
    );

    let err = builder.build().expect_err("duplicate polymorphic id");
    assert_has_code(&err, CfdErrorCode::DuplicatePolymorphicId);
    let diag = err
        .diagnostics
        .iter()
        .find(|diag| diag.code == CfdErrorCode::DuplicatePolymorphicId)
        .expect("diag");
    assert!(!diag.related.is_empty());
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
    assert_eq!(stats.field("speed"), Some(&CfdValue::Float(1.0)));
}

#[test]
fn semantic_edges_report_data_model_diagnostics() {
    let schema = compile_schema(
        r#"
            enum Rarity { Common, Rare, }
            type Item {
                @id
                id: string;
                rarity: Rarity;
                maybe: int?;
                attrs: {string: int};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("item_1")),
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
fn build_collects_diagnostics_across_multiple_records() {
    let schema = compile_schema(
        r#"
            type Item { id: string; value: int; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("a")),
            ("value", CfdInputValue::from("not_int")),
        ],
    );
    builder.add_record("MissingType", [("id", CfdInputValue::from("b"))]);

    let err = builder.build().expect_err("data errors");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
    assert_has_code(&err, CfdErrorCode::UnknownType);
}

#[test]
fn polymorphic_object_fields_need_actual_type_markers() {
    let schema = compile_schema(
        r#"
            abstract type Reward { id: string; }
            type CurrencyReward : Reward { amount: int; }
            type Drop { reward: Reward; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Drop",
        [(
            "reward",
            CfdInputValue::object_with_declared_type([
                ("id", CfdInputValue::from("r1")),
                ("amount", CfdInputValue::from(10_i64)),
            ]),
        )],
    );

    let err = builder.build().expect_err("missing object type");
    assert_has_code(&err, CfdErrorCode::MissingObjectType);
}

#[test]
fn ref_resolution_reports_missing_targets_and_targets_without_id() {
    let missing_schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Drop { @ref(Item) item_id: string; }
        "#,
    );
    let mut missing_builder = CfdDataModel::builder(&missing_schema);
    missing_builder.add_record(
        "Drop",
        [("item_id", CfdInputValue::Ref(CfdIdValue::from("missing")))],
    );
    let missing = missing_builder.build().expect_err("missing ref target");
    assert_has_code(&missing, CfdErrorCode::RefTargetNotFound);

    let no_id_schema = compile_schema(
        r#"
            type Item { name: string; }
            type Drop { @ref(Item) item_id: string; }
        "#,
    );
    let mut no_id_builder = CfdDataModel::builder(&no_id_schema);
    no_id_builder.add_record("Item", [("name", CfdInputValue::from("Potion"))]);
    no_id_builder.add_record(
        "Drop",
        [("item_id", CfdInputValue::Ref(CfdIdValue::from("potion")))],
    );
    let no_id = no_id_builder.build().expect_err("ref target without id");
    assert_has_code(&no_id, CfdErrorCode::RefTargetHasNoId);
}
