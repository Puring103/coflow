#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

use coflow_api::{ArtifactContent, DataExporter, ExportContext};
use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, ModuleId};
use coflow_data_model::{CfdDataModel, CfdInputDictKey, CfdInputValue};
use coflow_exporter_json::export_json_artifacts;
use serde_json::json;
use serde_json::Value;
use std::collections::BTreeMap;

type TestResult = Result<(), String>;

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
    let artifacts = export_json_artifacts(schema, model)
        .map_err(|err| format!("export json: {err:?}"))?;
    artifacts
        .files()
        .iter()
        .map(|file| {
            let table = file
                .relative_path
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or_else(|| "artifact table name should be UTF-8".to_string())?;
            let ArtifactContent::Text(text) = &file.content else {
                return Err(format!("JSON artifact `{table}` should contain text"));
            };
            let value = serde_json::from_str(text)
                .map_err(|err| format!("parse JSON artifact `{table}`: {err}"))?;
            Ok((table.to_string(), value))
        })
        .collect()
}

#[test]
fn exports_tables_with_schema_order_defaults_and_record_key_id() -> TestResult {
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
fn streaming_json_preserves_pretty_output_bytes() -> TestResult {
    let schema = compile_schema("type Item { name: string; numbers: [int]; }")?;
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item_1",
        "Item",
        [
            ("name", CfdInputValue::from("Sword")),
            (
                "numbers",
                CfdInputValue::Array(vec![CfdInputValue::from(1_i64), CfdInputValue::from(2_i64)]),
            ),
        ],
    );
    let model = build_model(builder)?;
    let artifacts = export_json_artifacts(&schema, &model)
        .map_err(|err| format!("export JSON artifacts: {err}"))?;
    let ArtifactContent::Text(actual) = &artifacts.files()[0].content else {
        return Err("JSON artifact should contain text".to_string());
    };
    let expected = serde_json::to_string_pretty(&json!([{
        "id": "item_1",
        "name": "Sword",
        "numbers": [1, 2]
    }]))
    .map_err(|err| format!("serialize expected JSON: {err}"))?;

    assert_eq!(actual, &expected);
    Ok(())
}

#[test]
fn artifact_export_omits_empty_tables() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type Monster { level: int; }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("item_1", "Item", [("name", CfdInputValue::from("Sword"))]);
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(tables["Item"], json!([{ "id": "item_1", "name": "Sword" }]));
    assert!(!tables.contains_key("Monster"));
    Ok(())
}

#[test]
fn json_exporter_skips_empty_table_files() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type Monster { level: int; }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("item_1", "Item", [("name", CfdInputValue::from("Sword"))]);
    let model = build_model(builder)?;
    let schema = &schema;
    let options = coflow_exporter_json::JsonExporter
        .decode_options(&json!({}))
        .map_err(|err| format!("decode JSON output options: {err:?}"))?;
    let artifacts = coflow_exporter_json::JsonExporter
        .export(
            ExportContext {
                schema: schema,
                model: &model,
            },
            &options,
        )
        .map_err(|err| format!("export json artifacts: {err:?}"))?;

    assert!(artifacts
        .files()
        .iter()
        .any(|file| file.relative_path.as_os_str() == "Item.json"));
    assert!(!artifacts
        .files()
        .iter()
        .any(|file| file.relative_path.as_os_str() == "Monster.json"));
    Ok(())
}

#[test]
fn json_exporter_rejects_output_options() {
    let diagnostics = coflow_exporter_json::JsonExporter
        .decode_options(&json!({"pretty": false}))
        .expect_err("JSON output options should be rejected");

    assert_eq!(diagnostics.diagnostics.len(), 1);
    assert_eq!(diagnostics.diagnostics[0].code, "JSON-OPTIONS");
}

#[test]
fn exports_refs_as_keys_and_polymorphic_objects_with_type_tags() -> TestResult {
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
                            ("item", CfdInputValue::record_ref("iron_sword")),
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
        json!([
            {
                "id": "drop_1",
                "rewards": [
                    {
                        "$type": "ItemReward",
                        "item": "iron_sword",
                        "count": 2
                    },
                    {
                        "$type": "CurrencyReward",
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
            type Reward { name: string; }
            type ItemReward : Reward { count: int; }
            type Holder {
                reward: Reward;
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "holder_1",
        "Holder",
        [(
            "reward",
            CfdInputValue::object("Reward", [("name", CfdInputValue::from("Base"))]),
        )],
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
                    "name": "Base"
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
                by_enum: {DamageType: float};
                by_int: {int: string};
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "resist_1",
        "Resistances",
        [
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
