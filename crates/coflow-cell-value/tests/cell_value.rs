#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

use coflow_cell_value::{parse_cell, CellValueDiagnostics, CellValueErrorCode, ParsedCell};
use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::{CfdDataModel, CfdInputDictKey, CfdInputValue, CfdValue};

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

fn parse_ok(schema: &CftContainer, declared_type: &str, text: &str) -> Result<ParsedCell, String> {
    parse_cell(schema, declared_type, text)
        .map_err(|err| format!("expected `{text}` as `{declared_type}` to parse: {err:?}"))
}

fn parse_value(
    schema: &CftContainer,
    declared_type: &str,
    text: &str,
) -> Result<CfdInputValue, String> {
    match parse_ok(schema, declared_type, text)? {
        ParsedCell::Value(value) => Ok(value),
        ParsedCell::Omitted => Err(format!(
            "expected `{text}` as `{declared_type}` to be value"
        )),
    }
}

fn parse_err(
    schema: &CftContainer,
    declared_type: &str,
    text: &str,
) -> Result<CellValueDiagnostics, String> {
    match parse_cell(schema, declared_type, text) {
        Ok(value) => Err(format!(
            "expected `{text}` as `{declared_type}` to fail, got {value:?}"
        )),
        Err(err) => Ok(err),
    }
}

fn build_model(builder: coflow_data_model::CfdModelBuilder<'_>) -> Result<CfdDataModel, String> {
    builder
        .build()
        .map_err(|err| format!("data model should build: {err:?}"))
}

fn assert_has_code(diags: &CellValueDiagnostics, code: CellValueErrorCode) {
    assert!(
        diags.diagnostics.iter().any(|diag| diag.code == code),
        "expected {code:?}, got {:?}",
        diags
            .diagnostics
            .iter()
            .map(|diag| diag.code)
            .collect::<Vec<_>>()
    );
}

#[test]
fn parses_schema_guided_scalar_values() -> TestResult {
    let schema = compile_schema("")?;

    assert_eq!(
        parse_ok(&schema, "int", "42")?,
        ParsedCell::Value(CfdInputValue::Int(42))
    );
    assert_eq!(
        parse_ok(&schema, "float", "2.5")?,
        ParsedCell::Value(CfdInputValue::Float(2.5))
    );
    assert_eq!(
        parse_ok(&schema, "bool", "true")?,
        ParsedCell::Value(CfdInputValue::Bool(true))
    );
    assert_eq!(
        parse_ok(&schema, "string", "hello world")?,
        ParsedCell::Value(CfdInputValue::String("hello world".to_string()))
    );
    assert_eq!(
        parse_ok(&schema, "string", r#""hello, world""#)?,
        ParsedCell::Value(CfdInputValue::String("hello, world".to_string()))
    );
    Ok(())
}

#[test]
fn parses_enum_values_with_or_without_type_prefix() -> TestResult {
    let schema = compile_schema(
        r#"
            enum Rarity { Common = 0, Rare = 10, }
        "#,
    )?;

    assert_eq!(
        parse_ok(&schema, "Rarity", "Rare")?,
        ParsedCell::Value(CfdInputValue::enum_variant("Rarity", "Rare"))
    );
    assert_eq!(
        parse_ok(&schema, "Rarity", "Rarity.Rare")?,
        ParsedCell::Value(CfdInputValue::enum_variant("Rarity", "Rare"))
    );
    Ok(())
}

#[test]
fn parses_positional_object_cells_using_field_order() -> TestResult {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int;
                attack: int;
                speed: float = 1.0;
            }
            type Monster {
                stats: Stats;
            }
        "#,
    )?;

    let stats = parse_value(&schema, "Stats", "100, 50")?;
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Monster", [("stats", stats)]);
    let model = build_model(builder)?;
    let Some((_, monster)) = model.records().next() else {
        return Err("expected monster record".to_string());
    };
    let Some(CfdValue::Object(stats)) = monster.field("stats") else {
        return Err("expected stats object".to_string());
    };

    assert_eq!(stats.field("hp"), Some(&CfdValue::Int(100)));
    assert_eq!(stats.field("attack"), Some(&CfdValue::Int(50)));
    assert_eq!(stats.field("speed"), Some(&CfdValue::Float(1.0)));
    Ok(())
}

#[test]
fn parses_named_object_cells_and_omits_skipped_fields() -> TestResult {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int;
                attack: int = 50;
                speed: float = 1.0;
            }
            type Monster {
                stats: Stats;
            }
        "#,
    )?;

    let stats = parse_value(&schema, "Stats", "speed: 2.0, hp: 100")?;
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Monster", [("stats", stats)]);
    let model = build_model(builder)?;
    let Some((_, monster)) = model.records().next() else {
        return Err("expected monster record".to_string());
    };
    let Some(CfdValue::Object(stats)) = monster.field("stats") else {
        return Err("expected stats object".to_string());
    };

    assert_eq!(stats.field("hp"), Some(&CfdValue::Int(100)));
    assert_eq!(stats.field("attack"), Some(&CfdValue::Int(50)));
    assert_eq!(stats.field("speed"), Some(&CfdValue::Float(2.0)));
    Ok(())
}

#[test]
fn rejects_duplicate_named_object_fields() -> TestResult {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int;
                attack: int;
            }
        "#,
    )?;

    let err = parse_err(&schema, "Stats", "hp: 100, hp: 200, attack: 50")?;
    assert_has_code(&err, CellValueErrorCode::DuplicateField);
    Ok(())
}

#[test]
fn parses_omitted_and_nullable_cells() -> TestResult {
    let schema = compile_schema("")?;

    assert_eq!(parse_ok(&schema, "int", "")?, ParsedCell::Omitted);
    assert_eq!(parse_ok(&schema, "int", "_")?, ParsedCell::Omitted);
    assert_eq!(
        parse_ok(&schema, "int?", "null")?,
        ParsedCell::Value(CfdInputValue::Null)
    );
    Ok(())
}

#[test]
fn parses_array_cells_with_pipe_delimiters() -> TestResult {
    let schema = compile_schema("")?;

    assert_eq!(
        parse_ok(&schema, "[int]", "1 | 2 | 3")?,
        ParsedCell::Value(CfdInputValue::Array(vec![
            CfdInputValue::Int(1),
            CfdInputValue::Int(2),
            CfdInputValue::Int(3),
        ]))
    );
    Ok(())
}

#[test]
fn parses_nested_objects_inside_array_cells() -> TestResult {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int;
                attack: int;
            }
            type Monster {
                id: string;
                level: int;
                stats: Stats;
            }
        "#,
    )?;

    assert_eq!(
        parse_ok(
            &schema,
            "[Monster]",
            "{slime, 5, {100, 50}} | {goblin, 10, {200, 80}}"
        )?,
        ParsedCell::Value(CfdInputValue::Array(vec![
            CfdInputValue::object_with_declared_type([
                ("id", CfdInputValue::String("slime".to_string())),
                ("level", CfdInputValue::Int(5)),
                (
                    "stats",
                    CfdInputValue::object_with_declared_type([
                        ("hp", CfdInputValue::Int(100)),
                        ("attack", CfdInputValue::Int(50)),
                    ]),
                ),
            ]),
            CfdInputValue::object_with_declared_type([
                ("id", CfdInputValue::String("goblin".to_string())),
                ("level", CfdInputValue::Int(10)),
                (
                    "stats",
                    CfdInputValue::object_with_declared_type([
                        ("hp", CfdInputValue::Int(200)),
                        ("attack", CfdInputValue::Int(80)),
                    ]),
                ),
            ]),
        ]))
    );
    Ok(())
}

#[test]
fn rejects_object_array_elements_without_object_boundaries() -> TestResult {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int;
                attack: int;
            }
        "#,
    )?;

    let err = parse_err(&schema, "[Stats]", "100, 50 | 200, 80")?;
    assert_has_code(&err, CellValueErrorCode::MissingBoundary);
    Ok(())
}

#[test]
fn rejects_nested_composite_values_without_boundaries() -> TestResult {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int;
                attack: int;
            }
            type Zone {
                name: string;
                weights: [int];
                stats: Stats;
                attrs: {string: int};
            }
        "#,
    )?;

    let missing_array = parse_err(&schema, "Zone", "forest, 1 | 2, {100, 50}, {hp: 10}")?;
    assert_has_code(&missing_array, CellValueErrorCode::MissingBoundary);

    let missing_object = parse_err(&schema, "Zone", "forest, [1 | 2], 100, {hp: 10}")?;
    assert_has_code(&missing_object, CellValueErrorCode::MissingBoundary);

    let missing_dict = parse_err(&schema, "Zone", "forest, [1 | 2], {100, 50}, hp: 10")?;
    assert_has_code(&missing_dict, CellValueErrorCode::MissingBoundary);
    Ok(())
}

#[test]
fn rejects_comma_arrays_and_bare_special_strings() -> TestResult {
    let schema = compile_schema("")?;

    let comma_array = parse_err(&schema, "[string]", "[weapon, melee]")?;
    assert_has_code(&comma_array, CellValueErrorCode::Syntax);

    for text in ["hello, world", "fire|ice", "key: value"] {
        let err = parse_err(&schema, "string", text)?;
        assert_has_code(&err, CellValueErrorCode::StringNeedsQuotes);
    }

    assert_eq!(
        parse_ok(&schema, "string", r#""_""#)?,
        ParsedCell::Value(CfdInputValue::String("_".to_string()))
    );
    assert_eq!(
        parse_ok(&schema, "string", r#""null""#)?,
        ParsedCell::Value(CfdInputValue::String("null".to_string()))
    );
    Ok(())
}

#[test]
fn keeps_delimiters_inside_quotes_and_nested_boundaries() -> TestResult {
    let schema = compile_schema(
        r#"
            type Payload {
                name: string;
                notes: [string];
                attrs: {string: string};
            }
        "#,
    )?;

    assert_eq!(
        parse_ok(
            &schema,
            "Payload",
            r#"name: "hello, world", notes: ["a|b" | "c,d"], attrs: {"x:y": "v|1", plain: "a,b"}"#,
        )?,
        ParsedCell::Value(CfdInputValue::object_with_declared_type([
            ("name", CfdInputValue::String("hello, world".to_string())),
            (
                "notes",
                CfdInputValue::Array(vec![
                    CfdInputValue::String("a|b".to_string()),
                    CfdInputValue::String("c,d".to_string()),
                ]),
            ),
            (
                "attrs",
                CfdInputValue::dict([
                    (
                        CfdInputDictKey::from("x:y"),
                        CfdInputValue::String("v|1".to_string()),
                    ),
                    (
                        CfdInputDictKey::from("plain"),
                        CfdInputValue::String("a,b".to_string()),
                    ),
                ]),
            ),
        ]))
    );
    Ok(())
}

#[test]
fn validates_polymorphic_type_markers_are_assignable_and_concrete() -> TestResult {
    let schema = compile_schema(
        r#"
            abstract type Reward {
                id: string;
            }
            type CurrencyReward : Reward {
                amount: int;
            }
            type Item {
                id: string;
            }
        "#,
    )?;

    let abstract_actual = parse_err(&schema, "Reward", "Reward{r1}")?;
    assert_has_code(&abstract_actual, CellValueErrorCode::AbstractObjectType);

    let unassignable = parse_err(&schema, "Reward", "Item{i1}")?;
    assert_has_code(&unassignable, CellValueErrorCode::ObjectTypeMismatch);
    Ok(())
}

#[test]
fn rejects_malformed_declared_types() -> TestResult {
    let schema = compile_schema("")?;

    for declared_type in ["", "[int", "{string int}", "{: int}", "{string:}", "[]"] {
        let err = parse_err(&schema, declared_type, "1")?;
        assert_has_code(&err, CellValueErrorCode::InvalidDeclaredType);
    }
    Ok(())
}

#[test]
fn rejects_unbalanced_boundaries_and_invalid_string_escapes() -> TestResult {
    let schema = compile_schema("")?;

    let unclosed_array = parse_err(&schema, "[int]", "[1 | 2")?;
    assert_has_code(&unclosed_array, CellValueErrorCode::Syntax);

    let mismatched = parse_err(&schema, "[int]", "[1}")?;
    assert_has_code(&mismatched, CellValueErrorCode::Syntax);

    let extra_close = parse_err(&schema, "[int]", "1 | 2]")?;
    assert_has_code(&extra_close, CellValueErrorCode::Syntax);

    let unclosed_string = parse_err(&schema, "string", r#""hello"#)?;
    assert_has_code(&unclosed_string, CellValueErrorCode::Syntax);

    let invalid_escape = parse_err(&schema, "string", r#""\x""#)?;
    assert_has_code(&invalid_escape, CellValueErrorCode::Syntax);
    Ok(())
}

#[test]
fn rejects_non_finite_float_values() -> TestResult {
    let schema = compile_schema("")?;

    for text in ["NaN", "inf", "-inf", "infinity", "-infinity"] {
        let err = parse_err(&schema, "float", text)?;
        assert_has_code(&err, CellValueErrorCode::TypeMismatch);
    }
    Ok(())
}

#[test]
fn quoted_strings_require_internal_quotes_to_be_escaped() -> TestResult {
    let schema = compile_schema("")?;

    assert_eq!(
        parse_ok(&schema, "string", r#""a\"b""#)?,
        ParsedCell::Value(CfdInputValue::String("a\"b".to_string()))
    );

    let err = parse_err(&schema, "string", r#""a"b""#)?;
    assert_has_code(&err, CellValueErrorCode::Syntax);
    Ok(())
}

#[test]
fn parses_full_nested_root_object_example() -> TestResult {
    let schema = compile_schema(
        r#"
            abstract type Reward {
                id: string;
            }
            type CurrencyReward : Reward {
                amount: int;
            }
            type ItemReward : Reward {
                item_id: string;
                count: int = 1;
            }
            type DropTable {
                rewards: [Reward];
                weights: [int];
            }
        "#,
    )?;

    assert_eq!(
        parse_ok(
            &schema,
            "DropTable",
            "rewards: [CurrencyReward{r1, 100} | ItemReward{r2, sword_01, 1}], weights: [60 | 40]",
        )?,
        ParsedCell::Value(CfdInputValue::object_with_declared_type([
            (
                "rewards",
                CfdInputValue::Array(vec![
                    CfdInputValue::object(
                        "CurrencyReward",
                        [
                            ("id", CfdInputValue::String("r1".to_string())),
                            ("amount", CfdInputValue::Int(100)),
                        ],
                    ),
                    CfdInputValue::object(
                        "ItemReward",
                        [
                            ("id", CfdInputValue::String("r2".to_string())),
                            ("item_id", CfdInputValue::String("sword_01".to_string())),
                            ("count", CfdInputValue::Int(1)),
                        ],
                    ),
                ]),
            ),
            (
                "weights",
                CfdInputValue::Array(vec![CfdInputValue::Int(60), CfdInputValue::Int(40)]),
            ),
        ]))
    );
    Ok(())
}

#[test]
fn parses_polymorphic_object_type_markers() -> TestResult {
    let schema = compile_schema(
        r#"
            abstract type Reward {
                id: string;
            }
            type CurrencyReward : Reward {
                amount: int;
            }
        "#,
    )?;

    assert_eq!(
        parse_ok(&schema, "Reward", "CurrencyReward{r1, 100}")?,
        ParsedCell::Value(CfdInputValue::object(
            "CurrencyReward",
            [
                ("id", CfdInputValue::String("r1".to_string())),
                ("amount", CfdInputValue::Int(100)),
            ],
        ))
    );
    Ok(())
}

#[test]
fn parses_unicode_polymorphic_object_type_markers() -> TestResult {
    let schema = compile_schema(
        r#"
            abstract type 奖励 {
                id: string;
            }
            type 金币奖励 : 奖励 {
                数量: int;
            }
        "#,
    )?;

    assert_eq!(
        parse_ok(&schema, "奖励", "金币奖励{r1, 100}")?,
        ParsedCell::Value(CfdInputValue::object(
            "金币奖励",
            [
                ("id", CfdInputValue::String("r1".to_string())),
                ("数量", CfdInputValue::Int(100)),
            ],
        ))
    );
    Ok(())
}

#[test]
fn ref_cells_parse_as_their_declared_id_type_for_data_model_resolution() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                @id
                id: string;
            }
            type Drop {
                @ref(Item)
                item_id: string;
            }
        "#,
    )?;

    let item_id = parse_value(&schema, "string", "item_1")?;
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [("id", CfdInputValue::String("item_1".to_string()))],
    );
    builder.add_record("Drop", [("item_id", item_id)]);
    let model = build_model(builder)?;
    let Some(item_record_id) = model.records().next().map(|(id, _)| id) else {
        return Err("expected item record".to_string());
    };
    let Some((_, drop_record)) = model.records().nth(1) else {
        return Err("expected drop record".to_string());
    };

    assert_eq!(
        drop_record.field("item_id"),
        Some(&CfdValue::Ref {
            id: "item_1".into(),
            target: item_record_id,
        })
    );
    Ok(())
}

#[test]
fn parses_dict_cells_with_schema_guided_keys() -> TestResult {
    let schema = compile_schema("")?;

    assert_eq!(
        parse_ok(&schema, "{string: int}", "alice: 10, bob: 20")?,
        ParsedCell::Value(CfdInputValue::dict([
            (CfdInputDictKey::from("alice"), CfdInputValue::Int(10)),
            (CfdInputDictKey::from("bob"), CfdInputValue::Int(20)),
        ]))
    );
    Ok(())
}
