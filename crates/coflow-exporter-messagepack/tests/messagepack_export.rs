#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

use coflow_api::{CftContainer, ModuleId};
use coflow_data_model::{CfdDataModel, CfdInputDictKey, CfdInputValue};
use coflow_exporter_messagepack::export_messagepack_model;
use rmpv::Value;
use std::collections::BTreeMap;
use std::io::Cursor;

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
    let bytes_by_table = export_messagepack_model(schema, model)
        .map_err(|err| format!("export msgpack: {err:?}"))?;
    bytes_by_table
        .into_iter()
        .map(|(table, bytes)| {
            let mut cursor = Cursor::new(bytes);
            let value = rmpv::decode::read_value(&mut cursor)
                .map_err(|err| format!("decode msgpack table `{table}`: {err}"))?;
            Ok((table, value))
        })
        .collect()
}

fn map(entries: Vec<(&str, Value)>) -> Value {
    Value::Map(
        entries
            .into_iter()
            .map(|(key, value)| (Value::from(key), value))
            .collect(),
    )
}

#[test]
fn exports_tables_as_messagepack_arrays_with_json_export_shape() -> TestResult {
    let schema = compile_schema(
        r#"
            enum Rarity { Common = 0, Rare = 10, }
            type Item {
                name: string = "unknown";
                rarity: Rarity = Rarity.Common;
                tags: [string] = [];
                attrs: {string: int} = {};
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "iron_sword",
        "Item",
        [
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
        Value::Array(vec![map(vec![
            ("id", Value::from("iron_sword")),
            ("name", Value::from("unknown")),
            ("rarity", Value::from(10)),
            (
                "tags",
                Value::Array(vec![Value::from("weapon"), Value::from("melee")])
            ),
            (
                "attrs",
                map(vec![("attack", Value::from(12)), ("level", Value::from(3)),])
            ),
        ])])
    );
    Ok(())
}

#[test]
fn exports_refs_raw_keys_and_polymorphic_objects_with_type_tag_first() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            abstract type Reward {}
            type ItemReward : Reward {
                item: Item;
                count: int = 1;
            }
            type CurrencyReward : Reward {
                amount: int;
            }
            type DropTable {
                rewards: [Reward];
                weights: [int];
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "iron_sword",
        "Item",
        [("name", CfdInputValue::from("Iron Sword"))],
    );
    builder.add_record(
        "drop_1",
        "DropTable",
        [
            (
                "rewards",
                CfdInputValue::Array(vec![
                    CfdInputValue::object(
                        "ItemReward",
                        [
                            ("item", CfdInputValue::record_ref("Item", "iron_sword")),
                            ("count", CfdInputValue::from(2_i64)),
                        ],
                    ),
                    CfdInputValue::object(
                        "CurrencyReward",
                        [("amount", CfdInputValue::from(50_i64))],
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
        Value::Array(vec![map(vec![
            ("id", Value::from("drop_1")),
            (
                "rewards",
                Value::Array(vec![
                    map(vec![
                        ("$type", Value::from("ItemReward")),
                        ("item", Value::from("iron_sword")),
                        ("count", Value::from(2)),
                    ]),
                    map(vec![
                        ("$type", Value::from("CurrencyReward")),
                        ("amount", Value::from(50)),
                    ]),
                ])
            ),
            (
                "weights",
                Value::Array(vec![Value::from(70), Value::from(30)])
            ),
        ])])
    );

    let Value::Array(drop_tables) = &tables["DropTable"] else {
        panic!("DropTable should decode to an array");
    };
    let Value::Map(drop_table) = &drop_tables[0] else {
        panic!("DropTable record should decode to a map");
    };
    let Value::Array(rewards) = &drop_table[1].1 else {
        panic!("rewards should decode to an array");
    };
    let Value::Map(first_reward) = &rewards[0] else {
        panic!("reward should decode to a map");
    };
    assert_eq!(first_reward[0].0, Value::from("$type"));
    assert_eq!(first_reward[0].1, Value::from("ItemReward"));
    Ok(())
}

#[test]
fn exports_nullable_values_and_arrays_as_messagepack_values() -> TestResult {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int;
            }
            type Holder {
                maybe_stats: Stats?;
                maybe_tags: [string]?;
                maybe_attrs: {string: int}?;
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "h1",
        "Holder",
        [
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
        "h2",
        "Holder",
        [
            ("maybe_stats", CfdInputValue::Null),
            ("maybe_tags", CfdInputValue::Null),
            ("maybe_attrs", CfdInputValue::Null),
        ],
    );
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(
        tables["Holder"],
        Value::Array(vec![
            map(vec![
                ("id", Value::from("h1")),
                ("maybe_stats", map(vec![("hp", Value::from(10))])),
                (
                    "maybe_tags",
                    Value::Array(vec![Value::from("alpha"), Value::from("beta")])
                ),
                ("maybe_attrs", map(vec![("score", Value::from(7))])),
            ]),
            map(vec![
                ("id", Value::from("h2")),
                ("maybe_stats", Value::Nil),
                ("maybe_tags", Value::Nil),
                ("maybe_attrs", Value::Nil),
            ]),
        ])
    );
    Ok(())
}
