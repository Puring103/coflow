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
    Array(Vec<Self>),
    Map(Vec<(String, Self)>),
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
fn exports_every_concrete_table_with_synthesized_record_keys() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { name: string = "unknown"; }
            type Monster { level: int; }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item_1",
        "Item",
        std::iter::empty::<(&str, CfdInputValue)>(),
    );
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(
        tables["Item"],
        TestValue::Array(vec![TestValue::Map(vec![
            ("id".to_string(), TestValue::String("item_1".to_string())),
            ("name".to_string(), TestValue::String("unknown".to_string())),
        ])])
    );
    assert_eq!(tables["Monster"], TestValue::Array(Vec::new()));
    Ok(())
}

#[test]
fn exports_fields_in_inherited_schema_order() -> TestResult {
    let schema = compile_schema(
        r#"
            type Base {
                parent_field: int;
            }
            type Child : Base {
                child_field: string;
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "child_1",
        "Child",
        [
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
fn exports_polymorphic_inline_objects_with_type_tag_only() -> TestResult {
    let schema = compile_schema(
        r#"
            abstract type Reward {}
            type CurrencyReward : Reward { amount: int; }
            type DropTable {
                reward: Reward;
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "drop_1",
        "DropTable",
        [(
            "reward",
            CfdInputValue::object("CurrencyReward", [("amount", CfdInputValue::from(50_i64))]),
        )],
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
            type Item { name: string; }
            type Holder {
                item: Item;
                rarity: Rarity;
                by_int: {int: string};
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("item_1", "Item", [("name", CfdInputValue::from("Sword"))]);
    builder.add_record(
        "holder_1",
        "Holder",
        [
            ("item", CfdInputValue::record_ref("item_1")),
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
            ("id".to_string(), TestValue::String("holder_1".to_string())),
            ("item".to_string(), TestValue::String("item_1".to_string())),
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
fn exports_nullable_and_array_fields() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                maybe_name: string?;
                tags: [string];
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item_1",
        "Item",
        [
            ("maybe_name", CfdInputValue::Null),
            (
                "tags",
                CfdInputValue::Array(vec![
                    CfdInputValue::from("alpha"),
                    CfdInputValue::from("beta"),
                ]),
            ),
        ],
    );
    let model = build_model(builder)?;
    let tables = export_tables(&schema, &model)?;

    assert_eq!(
        tables["Item"],
        TestValue::Array(vec![TestValue::Map(vec![
            ("id".to_string(), TestValue::String("item_1".to_string())),
            ("maybe_name".to_string(), TestValue::Null),
            (
                "tags".to_string(),
                TestValue::Array(vec![
                    TestValue::String("alpha".to_string()),
                    TestValue::String("beta".to_string()),
                ])
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
    let schema = compile_schema("type Item { name: string; }")?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("item_1", "Item", [("name", CfdInputValue::from("Sword"))]);
    let model = build_model(builder)?;
    let mut encoder = FailingEncoder;
    let err = export_model_with_encoder(&schema, &model, &mut encoder)
        .expect_err("encoder error should become export error");

    assert_eq!(err.to_string(), "encoder string failed");
    Ok(())
}

#[test]
fn does_not_emit_type_tag_or_id_for_non_polymorphic_inline_object() -> TestResult {
    let schema = compile_schema(
        r#"
            type Stats { hp: int; }
            type Holder {
                stats: Stats;
            }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "holder_1",
        "Holder",
        [(
            "stats",
            CfdInputValue::object_with_declared_type([("hp", CfdInputValue::from(10_i64))]),
        )],
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
