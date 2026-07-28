#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

use coflow_api::{ArtifactContent, DataExporter};
use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, ModuleId};
use coflow_data_model::{CfdDataModel, LoadedDictKeyDraft, LoadedValueDraft};
use coflow_exporter_messagepack::export_messagepack_artifacts;
use rmpv::Value;
use std::collections::BTreeMap;
use std::io::Cursor;

type TestResult = Result<(), String>;

#[test]
fn messagepack_exporter_rejects_output_options() {
    let diagnostics = coflow_exporter_messagepack::MessagePackExporter
        .decode_options(&serde_json::json!({"compact": true}))
        .expect_err("MessagePack output options should be rejected");

    assert_eq!(diagnostics.diagnostics.len(), 1);
    assert_eq!(diagnostics.diagnostics[0].code, "MESSAGEPACK-OPTIONS");
}

fn compile_schema(source: &str) -> Result<CftSchema, String> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &CftDimensionInputs::default())
        .map_err(|err| format!("schema should compile: {err:?}"))
}

fn build_model(builder: coflow_data_model::CfdModelBuilder<'_>) -> Result<CfdDataModel, String> {
    builder
        .build()
        .map_err(|err| format!("data model should build: {err:?}"))
}

fn export_tables(
    schema: &CftSchema,
    model: &CfdDataModel,
) -> Result<BTreeMap<String, Value>, String> {
    let artifacts = export_messagepack_artifacts(schema, model)
        .map_err(|err| format!("export msgpack: {err:?}"))?;
    artifacts
        .files()
        .iter()
        .map(|file| {
            let table = file
                .relative_path
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or_else(|| "artifact table name should be UTF-8".to_string())?;
            let ArtifactContent::Bytes(bytes) = &file.content else {
                return Err(format!(
                    "MessagePack artifact `{table}` should contain bytes"
                ));
            };
            let mut cursor = Cursor::new(bytes);
            let value = rmpv::decode::read_value(&mut cursor)
                .map_err(|err| format!("decode msgpack table `{table}`: {err}"))?;
            Ok((table.to_string(), value))
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
            ("rarity", LoadedValueDraft::enum_variant("Rarity", "Rare")),
            (
                "tags",
                LoadedValueDraft::Array(vec![
                    LoadedValueDraft::from("weapon"),
                    LoadedValueDraft::from("melee"),
                ]),
            ),
            (
                "attrs",
                LoadedValueDraft::dict([
                    (
                        LoadedDictKeyDraft::from("attack"),
                        LoadedValueDraft::from(12_i64),
                    ),
                    (
                        LoadedDictKeyDraft::from("level"),
                        LoadedValueDraft::from(3_i64),
                    ),
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
                item: &Item;
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
        [("name", LoadedValueDraft::from("Iron Sword"))],
    );
    builder.add_record(
        "drop_1",
        "DropTable",
        [
            (
                "rewards",
                LoadedValueDraft::Array(vec![
                    LoadedValueDraft::object(
                        "ItemReward",
                        [
                            ("item", LoadedValueDraft::record_ref("iron_sword")),
                            ("count", LoadedValueDraft::from(2_i64)),
                        ],
                    ),
                    LoadedValueDraft::object(
                        "CurrencyReward",
                        [("amount", LoadedValueDraft::from(50_i64))],
                    ),
                ]),
            ),
            (
                "weights",
                LoadedValueDraft::Array(vec![
                    LoadedValueDraft::from(70_i64),
                    LoadedValueDraft::from(30_i64),
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
                LoadedValueDraft::object_with_declared_type([(
                    "hp",
                    LoadedValueDraft::from(10_i64),
                )]),
            ),
            (
                "maybe_tags",
                LoadedValueDraft::Array(vec![
                    LoadedValueDraft::from("alpha"),
                    LoadedValueDraft::from("beta"),
                ]),
            ),
            (
                "maybe_attrs",
                LoadedValueDraft::dict([("score".into(), LoadedValueDraft::from(7_i64))]),
            ),
        ],
    );
    builder.add_record(
        "h2",
        "Holder",
        [
            ("maybe_stats", LoadedValueDraft::Null),
            ("maybe_tags", LoadedValueDraft::Null),
            ("maybe_attrs", LoadedValueDraft::Null),
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
