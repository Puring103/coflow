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
use coflow_exporter_core::{export_model_with_encoder, ExportEncoder};
use std::collections::BTreeMap;
use std::convert::Infallible;

type TestResult = Result<(), String>;

#[derive(Debug, Clone, PartialEq)]
enum TestValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<TestValue>),
    Map(Vec<(String, TestValue)>),
}

#[derive(Debug, Clone, Copy)]
struct TestEncoder;

impl ExportEncoder for TestEncoder {
    type Error = Infallible;
    type Value = TestValue;

    fn null(&mut self) -> Result<Self::Value, Self::Error> {
        Ok(TestValue::Null)
    }

    fn bool(&mut self, value: bool) -> Result<Self::Value, Self::Error> {
        Ok(TestValue::Bool(value))
    }

    fn int(&mut self, value: i64) -> Result<Self::Value, Self::Error> {
        Ok(TestValue::Int(value))
    }

    fn float(&mut self, value: f64) -> Result<Self::Value, Self::Error> {
        Ok(TestValue::Float(value))
    }

    fn string(&mut self, value: &str) -> Result<Self::Value, Self::Error> {
        Ok(TestValue::String(value.to_string()))
    }

    fn array(&mut self, values: Vec<Self::Value>) -> Result<Self::Value, Self::Error> {
        Ok(TestValue::Array(values))
    }

    fn map(&mut self, entries: Vec<(String, Self::Value)>) -> Result<Self::Value, Self::Error> {
        Ok(TestValue::Map(entries))
    }
}

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
) -> Result<BTreeMap<String, TestValue>, String> {
    let mut encoder = TestEncoder;
    export_model_with_encoder(schema, model, &mut encoder)
        .map_err(|err| format!("export core: {err:?}"))
}

#[test]
fn exports_every_concrete_id_table_and_empty_arrays_for_missing_id_types() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Monster { @id id: string; }
            type InlineOnly { value: string; }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("item_1"))]);
    builder.add_record("InlineOnly", [("value", CfdInputValue::from("embedded"))]);
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(
        tables["Item"],
        TestValue::Array(vec![TestValue::Map(vec![(
            "id".to_string(),
            TestValue::String("item_1".to_string())
        )])])
    );
    assert_eq!(tables["Monster"], TestValue::Array(Vec::new()));
    assert!(!tables.contains_key("InlineOnly"));
    Ok(())
}

#[test]
fn exports_fields_in_inherited_schema_order() -> TestResult {
    let schema = compile_schema(
        r#"
            type Base {
                @id
                id: string;
                parent_field: int;
            }
            type Child : Base {
                child_field: string;
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Child",
        [
            ("id", CfdInputValue::from("child_1")),
            ("parent_field", CfdInputValue::from(7_i64)),
            ("child_field", CfdInputValue::from("leaf")),
        ],
    );
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(
        tables["Child"],
        TestValue::Array(vec![TestValue::Map(vec![
            ("id".to_string(), TestValue::String("child_1".to_string())),
            ("parent_field".to_string(), TestValue::Int(7)),
            (
                "child_field".to_string(),
                TestValue::String("leaf".to_string())
            ),
        ])])
    );
    Ok(())
}

#[test]
fn exports_polymorphic_objects_with_type_tag_as_first_entry() -> TestResult {
    let schema = compile_schema(
        r#"
            abstract type Reward { id: string; }
            type CurrencyReward : Reward { amount: int; }
            type DropTable {
                @id
                id: string;
                reward: Reward;
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "DropTable",
        [
            ("id", CfdInputValue::from("drop_1")),
            (
                "reward",
                CfdInputValue::object(
                    "CurrencyReward",
                    [
                        ("id", CfdInputValue::from("reward_gold")),
                        ("amount", CfdInputValue::from(50_i64)),
                    ],
                ),
            ),
        ],
    );
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(
        tables["DropTable"],
        TestValue::Array(vec![TestValue::Map(vec![
            ("id".to_string(), TestValue::String("drop_1".to_string())),
            (
                "reward".to_string(),
                TestValue::Map(vec![
                    (
                        "$type".to_string(),
                        TestValue::String("CurrencyReward".to_string())
                    ),
                    (
                        "id".to_string(),
                        TestValue::String("reward_gold".to_string())
                    ),
                    ("amount".to_string(), TestValue::Int(50)),
                ])
            ),
        ])])
    );
    Ok(())
}

#[test]
fn exports_refs_enums_and_dict_keys_as_exporter_scalars() -> TestResult {
    let schema = compile_schema(
        r#"
            enum Rarity { Common = 0, Rare = 10, }
            type Item { @id id: string; }
            type Holder {
                @id
                id: int;
                @ref(Item)
                item_id: string;
                rarity: Rarity;
                by_int: {int: string};
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("item_1"))]);
    builder.add_record(
        "Holder",
        [
            ("id", CfdInputValue::from(42_i64)),
            ("item_id", CfdInputValue::from("item_1")),
            ("rarity", CfdInputValue::enum_variant("Rarity", "Rare")),
            (
                "by_int",
                CfdInputValue::dict([(CfdInputDictKey::from(7_i64), CfdInputValue::from("seven"))]),
            ),
        ],
    );
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(
        tables["Holder"],
        TestValue::Array(vec![TestValue::Map(vec![
            ("id".to_string(), TestValue::Int(42)),
            (
                "item_id".to_string(),
                TestValue::String("item_1".to_string())
            ),
            ("rarity".to_string(), TestValue::Int(10)),
            (
                "by_int".to_string(),
                TestValue::Map(vec![(
                    "7".to_string(),
                    TestValue::String("seven".to_string())
                )])
            ),
        ])])
    );
    Ok(())
}

#[test]
fn exports_nullable_fields_as_null_or_inner_value() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                @id
                id: string;
                maybe_name: string?;
                maybe_count: int?;
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("null_item")),
            ("maybe_name", CfdInputValue::Null),
            ("maybe_count", CfdInputValue::from(3_i64)),
        ],
    );
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(
        tables["Item"],
        TestValue::Array(vec![TestValue::Map(vec![
            ("id".to_string(), TestValue::String("null_item".to_string())),
            ("maybe_name".to_string(), TestValue::Null),
            ("maybe_count".to_string(), TestValue::Int(3)),
        ])])
    );
    Ok(())
}

#[test]
fn exports_array_fields_as_arrays() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                @id
                id: string;
                tags: [string];
                scores: [int];
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("array_item")),
            (
                "tags",
                CfdInputValue::Array(vec![
                    CfdInputValue::from("alpha"),
                    CfdInputValue::from("beta"),
                ]),
            ),
            (
                "scores",
                CfdInputValue::Array(vec![CfdInputValue::from(7_i64), CfdInputValue::from(9_i64)]),
            ),
        ],
    );
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(
        tables["Item"],
        TestValue::Array(vec![TestValue::Map(vec![
            (
                "id".to_string(),
                TestValue::String("array_item".to_string())
            ),
            (
                "tags".to_string(),
                TestValue::Array(vec![
                    TestValue::String("alpha".to_string()),
                    TestValue::String("beta".to_string()),
                ])
            ),
            (
                "scores".to_string(),
                TestValue::Array(vec![TestValue::Int(7), TestValue::Int(9)])
            ),
        ])])
    );
    Ok(())
}

#[derive(Debug)]
struct FailingEncoder;

impl ExportEncoder for FailingEncoder {
    type Error = &'static str;
    type Value = ();

    fn null(&mut self) -> Result<Self::Value, Self::Error> {
        Err("encoder null failed")
    }

    fn bool(&mut self, _value: bool) -> Result<Self::Value, Self::Error> {
        Err("encoder bool failed")
    }

    fn int(&mut self, _value: i64) -> Result<Self::Value, Self::Error> {
        Err("encoder int failed")
    }

    fn float(&mut self, _value: f64) -> Result<Self::Value, Self::Error> {
        Err("encoder float failed")
    }

    fn string(&mut self, _value: &str) -> Result<Self::Value, Self::Error> {
        Err("encoder string failed")
    }

    fn array(&mut self, _values: Vec<Self::Value>) -> Result<Self::Value, Self::Error> {
        Err("encoder array failed")
    }

    fn map(&mut self, _entries: Vec<(String, Self::Value)>) -> Result<Self::Value, Self::Error> {
        Err("encoder map failed")
    }
}

#[test]
fn converts_encoder_errors_to_export_error_display_string() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("item_1"))]);
    let model = build_model(builder)?;
    let mut encoder = FailingEncoder;
    let err = export_model_with_encoder(&schema, &model, &mut encoder)
        .expect_err("encoder error should become export error");

    assert_eq!(err.to_string(), "encoder string failed");
    Ok(())
}

#[test]
fn does_not_emit_type_tag_for_non_polymorphic_declared_object() -> TestResult {
    let schema = compile_schema(
        r#"
            type Stats { hp: int; }
            type Holder {
                @id
                id: string;
                stats: Stats;
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Holder",
        [
            ("id", CfdInputValue::from("holder_1")),
            (
                "stats",
                CfdInputValue::object_with_declared_type([("hp", CfdInputValue::from(10_i64))]),
            ),
        ],
    );
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(
        tables["Holder"],
        TestValue::Array(vec![TestValue::Map(vec![
            ("id".to_string(), TestValue::String("holder_1".to_string())),
            (
                "stats".to_string(),
                TestValue::Map(vec![("hp".to_string(), TestValue::Int(10))])
            ),
        ])])
    );
    Ok(())
}
