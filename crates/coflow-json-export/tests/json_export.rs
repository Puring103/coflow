#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::{CfdDataModel, CfdInputDictKey, CfdInputValue};
use coflow_json_export::export_json_model;
use serde_json::json;
use serde_json::Value;
use std::collections::BTreeMap;

type TestResult = Result<(), String>;

fn compile_schema(source: &str) -> Result<CftContainer, String> {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("main"), source)
        .map_err(|err| format!("schema should parse: {err:?}"))?;
    container
        .compile()
        .map_err(|err| format!("schema should compile: {err:?}"))?;
    Ok(container)
}

fn build_model(builder: coflow_data_model::CfdModelBuilder<'_>) -> Result<CfdDataModel, String> {
    builder
        .build()
        .map_err(|err| format!("data model should build: {err:?}"))
}

fn export_tables(
    schema: &CftContainer,
    model: &CfdDataModel,
) -> Result<BTreeMap<String, Value>, String> {
    export_json_model(schema, model).map_err(|err| format!("export json: {err:?}"))
}

#[test]
fn exports_tables_with_schema_order_defaults_and_scalar_values() -> TestResult {
    let schema = compile_schema(
        r#"
            enum Rarity { Common = 0, Rare = 10, }
            type Item {
                @id
                id: string;
                name: string = "unknown";
                rarity: Rarity = Rarity.Common;
                tags: [string] = [];
                attrs: {string: int} = {};
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("iron_sword")),
            ("rarity", CfdInputValue::enum_variant("Rarity", "Rare")),
            (
                "tags",
                CfdInputValue::Array(vec![
                    CfdInputValue::from("weapon"),
                    CfdInputValue::from("melee"),
                ]),
            ),
            (
                "attrs",
                CfdInputValue::dict([
                    (CfdInputDictKey::from("attack"), CfdInputValue::from(12_i64)),
                    (CfdInputDictKey::from("level"), CfdInputValue::from(3_i64)),
                ]),
            ),
        ],
    );
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(
        tables["Item"],
        json!([
            {
                "id": "iron_sword",
                "name": "unknown",
                "rarity": 10,
                "tags": ["weapon", "melee"],
                "attrs": {
                    "attack": 12,
                    "level": 3
                }
            }
        ])
    );
    Ok(())
}

#[test]
fn exports_empty_tables_for_concrete_id_types() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Monster { @id id: string; }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("item_1"))]);
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(tables["Item"], json!([{ "id": "item_1" }]));
    assert_eq!(tables["Monster"], json!([]));
    Ok(())
}

#[test]
fn skips_concrete_non_id_types_even_with_records() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Inline { value: string; }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("item_1"))]);
    builder.add_record("Inline", [("value", CfdInputValue::from("embedded"))]);
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert!(tables.contains_key("Item"));
    assert!(!tables.contains_key("Inline"));
    Ok(())
}

#[test]
fn exports_refs_as_ids_and_polymorphic_objects_with_type_tags() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; name: string; }
            abstract type Reward { id: string; }
            type ItemReward : Reward {
                @ref(Item)
                item_id: string;
                count: int = 1;
            }
            type CurrencyReward : Reward {
                amount: int;
            }
            type DropTable {
                @id
                id: string;
                rewards: [Reward];
                weights: [int];
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("iron_sword")),
            ("name", CfdInputValue::from("Iron Sword")),
        ],
    );
    builder.add_record(
        "DropTable",
        [
            ("id", CfdInputValue::from("drop_1")),
            (
                "rewards",
                CfdInputValue::Array(vec![
                    CfdInputValue::object(
                        "ItemReward",
                        [
                            ("id", CfdInputValue::from("reward_sword")),
                            ("item_id", CfdInputValue::from("iron_sword")),
                            ("count", CfdInputValue::from(2_i64)),
                        ],
                    ),
                    CfdInputValue::object(
                        "CurrencyReward",
                        [
                            ("id", CfdInputValue::from("reward_gold")),
                            ("amount", CfdInputValue::from(50_i64)),
                        ],
                    ),
                ]),
            ),
            (
                "weights",
                CfdInputValue::Array(vec![
                    CfdInputValue::from(70_i64),
                    CfdInputValue::from(30_i64),
                ]),
            ),
        ],
    );
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(
        tables["DropTable"],
        json!([
            {
                "id": "drop_1",
                "rewards": [
                    {
                        "$type": "ItemReward",
                        "id": "reward_sword",
                        "item_id": "iron_sword",
                        "count": 2
                    },
                    {
                        "$type": "CurrencyReward",
                        "id": "reward_gold",
                        "amount": 50
                    }
                ],
                "weights": [70, 30]
            }
        ])
    );
    Ok(())
}

#[test]
fn exports_type_tag_for_concrete_parent_ranges_even_when_actual_is_parent() -> TestResult {
    let schema = compile_schema(
        r#"
            type Reward { id: string; }
            type ItemReward : Reward { count: int; }
            type Holder {
                @id
                id: string;
                reward: Reward;
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Holder",
        [
            ("id", CfdInputValue::from("holder_1")),
            (
                "reward",
                CfdInputValue::object("Reward", [("id", CfdInputValue::from("base_reward"))]),
            ),
        ],
    );
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(
        tables["Holder"],
        json!([
            {
                "id": "holder_1",
                "reward": {
                    "$type": "Reward",
                    "id": "base_reward"
                }
            }
        ])
    );
    Ok(())
}

#[test]
fn exports_dict_keys_as_json_object_keys() -> TestResult {
    let schema = compile_schema(
        r#"
            enum DamageType { Physical = 0, Fire = 1, Ice = 2, }
            type Resistances {
                @id
                id: string;
                by_enum: {DamageType: float};
                by_int: {int: string};
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Resistances",
        [
            ("id", CfdInputValue::from("resist_1")),
            (
                "by_enum",
                CfdInputValue::dict([
                    (
                        CfdInputDictKey::enum_variant("DamageType", "Fire"),
                        CfdInputValue::from(0.5_f64),
                    ),
                    (
                        CfdInputDictKey::enum_variant("DamageType", "Ice"),
                        CfdInputValue::from(0.2_f64),
                    ),
                ]),
            ),
            (
                "by_int",
                CfdInputValue::dict([
                    (CfdInputDictKey::from(1_i64), CfdInputValue::from("one")),
                    (CfdInputDictKey::from(2_i64), CfdInputValue::from("two")),
                ]),
            ),
        ],
    );
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(
        tables["Resistances"],
        json!([
            {
                "id": "resist_1",
                "by_enum": {
                    "1": 0.5,
                    "2": 0.2
                },
                "by_int": {
                    "1": "one",
                    "2": "two"
                }
            }
        ])
    );
    Ok(())
}

#[test]
fn exports_nullable_composite_values_using_schema_type_refs() -> TestResult {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int;
            }
            type Holder {
                @id
                id: string;
                maybe_stats: Stats?;
                maybe_tags: [string]?;
                maybe_attrs: {string: int}?;
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Holder",
        [
            ("id", CfdInputValue::from("h1")),
            (
                "maybe_stats",
                CfdInputValue::object_with_declared_type([("hp", CfdInputValue::from(10_i64))]),
            ),
            (
                "maybe_tags",
                CfdInputValue::Array(vec![
                    CfdInputValue::from("alpha"),
                    CfdInputValue::from("beta"),
                ]),
            ),
            (
                "maybe_attrs",
                CfdInputValue::dict([("score".into(), CfdInputValue::from(7_i64))]),
            ),
        ],
    );
    builder.add_record(
        "Holder",
        [
            ("id", CfdInputValue::from("h2")),
            ("maybe_stats", CfdInputValue::Null),
            ("maybe_tags", CfdInputValue::Null),
            ("maybe_attrs", CfdInputValue::Null),
        ],
    );
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(
        tables["Holder"],
        json!([
            {
                "id": "h1",
                "maybe_stats": { "hp": 10 },
                "maybe_tags": ["alpha", "beta"],
                "maybe_attrs": { "score": 7 }
            },
            {
                "id": "h2",
                "maybe_stats": null,
                "maybe_tags": null,
                "maybe_attrs": null
            }
        ])
    );
    Ok(())
}
