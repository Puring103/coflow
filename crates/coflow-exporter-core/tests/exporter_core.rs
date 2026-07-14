#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

use coflow_cft::{build_schema, parse_modules, CftDimensions, CftFile, CftSchema, ModuleId};
use coflow_data_model::{CfdDataModel, CfdInputDictKey, CfdInputValue};
use coflow_exporter_core::{export_model_to_sink, ExportEventSink};
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

#[derive(Debug, Default)]
struct TestSink {
    tables: BTreeMap<String, TestValue>,
    table_name: Option<String>,
    root: Option<TestValue>,
    stack: Vec<TestFrame>,
}

#[derive(Debug)]
enum TestFrame {
    Array(Vec<TestValue>),
    Map {
        entries: Vec<(String, TestValue)>,
        key: Option<String>,
    },
}

impl TestSink {
    fn push(&mut self, value: TestValue) {
        match self.stack.last_mut() {
            Some(TestFrame::Array(values)) => values.push(value),
            Some(TestFrame::Map { entries, key }) => {
                entries.push((key.take().expect("map value key"), value));
            }
            None => self.root = Some(value),
        }
    }
}

impl ExportEventSink for TestSink {
    type Error = Infallible;

    fn begin_table(&mut self, name: &str, _records: usize) -> Result<(), Self::Error> {
        self.table_name = Some(name.to_string());
        self.begin_array(0)
    }

    fn end_table(&mut self) -> Result<(), Self::Error> {
        self.end_array()?;
        self.tables.insert(
            self.table_name.take().expect("table name"),
            self.root.take().expect("table root"),
        );
        Ok(())
    }

    fn begin_array(&mut self, _len: usize) -> Result<(), Self::Error> {
        self.stack.push(TestFrame::Array(Vec::new()));
        Ok(())
    }

    fn end_array(&mut self) -> Result<(), Self::Error> {
        let TestFrame::Array(values) = self.stack.pop().expect("array frame") else {
            panic!("expected array frame");
        };
        self.push(TestValue::Array(values));
        Ok(())
    }

    fn begin_map(&mut self, _len: usize) -> Result<(), Self::Error> {
        self.stack.push(TestFrame::Map {
            entries: Vec::new(),
            key: None,
        });
        Ok(())
    }

    fn map_key(&mut self, map_key: &str) -> Result<(), Self::Error> {
        let Some(TestFrame::Map { key, .. }) = self.stack.last_mut() else {
            panic!("expected map frame");
        };
        *key = Some(map_key.to_string());
        Ok(())
    }

    fn end_map(&mut self) -> Result<(), Self::Error> {
        let TestFrame::Map { entries, key } = self.stack.pop().expect("map frame") else {
            panic!("expected map frame");
        };
        assert!(key.is_none(), "map key should have a value");
        self.push(TestValue::Map(entries));
        Ok(())
    }

    fn null(&mut self) -> Result<(), Self::Error> {
        self.push(TestValue::Null);
        Ok(())
    }

    fn bool(&mut self, value: bool) -> Result<(), Self::Error> {
        self.push(TestValue::Bool(value));
        Ok(())
    }

    fn int(&mut self, value: i64) -> Result<(), Self::Error> {
        self.push(TestValue::Int(value));
        Ok(())
    }

    fn float(&mut self, value: f64) -> Result<(), Self::Error> {
        self.push(TestValue::Float(value));
        Ok(())
    }

    fn string(&mut self, value: &str) -> Result<(), Self::Error> {
        self.push(TestValue::String(value.to_string()));
        Ok(())
    }
}

fn compile_schema(source: &str) -> Result<CftSchema, String> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &CftDimensions::default())
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
) -> Result<BTreeMap<String, TestValue>, String> {
    let mut sink = TestSink::default();
    export_model_to_sink(schema, model, &mut sink)
        .map_err(|err| format!("export core: {err:?}"))?;
    Ok(sink.tables)
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
                item: &Item;
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
struct FailingSink;

impl ExportEventSink for FailingSink {
    type Error = &'static str;

    fn begin_table(&mut self, _name: &str, _records: usize) -> Result<(), Self::Error> {
        Ok(())
    }

    fn end_table(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn begin_array(&mut self, _len: usize) -> Result<(), Self::Error> {
        Ok(())
    }

    fn end_array(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn begin_map(&mut self, _len: usize) -> Result<(), Self::Error> {
        Ok(())
    }

    fn map_key(&mut self, _key: &str) -> Result<(), Self::Error> {
        Ok(())
    }

    fn end_map(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn null(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn bool(&mut self, _value: bool) -> Result<(), Self::Error> {
        Ok(())
    }

    fn int(&mut self, _value: i64) -> Result<(), Self::Error> {
        Ok(())
    }

    fn float(&mut self, _value: f64) -> Result<(), Self::Error> {
        Ok(())
    }

    fn string(&mut self, value: &str) -> Result<(), Self::Error> {
        if value == "fail here" {
            Err("encoder string failed")
        } else {
            Ok(())
        }
    }
}

#[test]
fn reports_sink_errors_with_record_and_full_field_path() -> TestResult {
    let schema = compile_schema(
        r#"
            type Nested { labels: [string]; }
            type Item { nested: Nested; }
        "#,
    )?;

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item_1",
        "Item",
        [(
            "nested",
            CfdInputValue::object_with_declared_type([(
                "labels",
                CfdInputValue::Array(vec![
                    CfdInputValue::from("ok"),
                    CfdInputValue::from("fail here"),
                ]),
            )]),
        )],
    );
    let model = build_model(builder)?;
    let mut sink = FailingSink;
    let err = export_model_to_sink(&schema, &model, &mut sink)
        .expect_err("sink error should become export error");

    assert_eq!(
        err.to_string(),
        "Item[\"item_1\"].nested.labels[1]: encoder string failed"
    );
    Ok(())
}

#[derive(Debug, Default)]
struct CountingSink {
    depth: usize,
    max_depth: usize,
    integers: usize,
}

impl CountingSink {
    fn enter(&mut self) {
        self.depth += 1;
        self.max_depth = self.max_depth.max(self.depth);
    }

    const fn exit(&mut self) {
        self.depth -= 1;
    }
}

impl ExportEventSink for CountingSink {
    type Error = Infallible;

    fn begin_table(&mut self, _name: &str, _records: usize) -> Result<(), Self::Error> {
        self.enter();
        Ok(())
    }

    fn end_table(&mut self) -> Result<(), Self::Error> {
        self.exit();
        Ok(())
    }

    fn begin_array(&mut self, _len: usize) -> Result<(), Self::Error> {
        self.enter();
        Ok(())
    }

    fn end_array(&mut self) -> Result<(), Self::Error> {
        self.exit();
        Ok(())
    }

    fn begin_map(&mut self, _len: usize) -> Result<(), Self::Error> {
        self.enter();
        Ok(())
    }

    fn map_key(&mut self, _key: &str) -> Result<(), Self::Error> {
        Ok(())
    }

    fn end_map(&mut self) -> Result<(), Self::Error> {
        self.exit();
        Ok(())
    }

    fn null(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn bool(&mut self, _value: bool) -> Result<(), Self::Error> {
        Ok(())
    }

    fn int(&mut self, _value: i64) -> Result<(), Self::Error> {
        self.integers += 1;
        Ok(())
    }

    fn float(&mut self, _value: f64) -> Result<(), Self::Error> {
        Ok(())
    }

    fn string(&mut self, _value: &str) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[test]
fn streams_large_arrays_through_a_constant_state_sink() -> TestResult {
    const ITEM_COUNT: usize = 50_000;
    const ITEM_COUNT_I64: i64 = 50_000;
    let schema = compile_schema("type Item { numbers: [int]; }")?;
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item_1",
        "Item",
        [(
            "numbers",
            CfdInputValue::Array((0..ITEM_COUNT_I64).map(CfdInputValue::from).collect()),
        )],
    );
    let model = build_model(builder)?;
    let mut sink = CountingSink::default();

    export_model_to_sink(&schema, &model, &mut sink)
        .map_err(|err| format!("stream large array: {err}"))?;

    assert_eq!(sink.integers, ITEM_COUNT);
    assert_eq!(sink.depth, 0);
    assert_eq!(sink.max_depth, 3);
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
