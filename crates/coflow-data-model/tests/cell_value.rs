#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, ModuleId};
use coflow_data_model::cell_value::{
    parse_cell, render_cell_value, CellValueDiagnostics, CellValueErrorCode, ParsedCell,
};
use coflow_data_model::{
    CfdDataModel, CfdDictKey, CfdEnumValue, CfdValue, LoadedDictKeyDraft, LoadedValueDraft,
};
use std::collections::BTreeSet;

type TestResult = Result<(), String>;

fn compile_schema(source: &str) -> Result<CftSchema, String> {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &CftDimensionInputs::default())
        .map_err(|err| format!("schema should compile: {err:?}"))
}

fn parse_ok(schema: &CftSchema, declared_type: &str, text: &str) -> Result<ParsedCell, String> {
    parse_cell(schema, declared_type, text)
        .map_err(|err| format!("expected `{text}` as `{declared_type}` to parse: {err:?}"))
}

fn parse_value(
    schema: &CftSchema,
    declared_type: &str,
    text: &str,
) -> Result<LoadedValueDraft, String> {
    match parse_ok(schema, declared_type, text)? {
        ParsedCell::Value(value) => Ok(value),
        ParsedCell::Omitted => Err(format!(
            "expected `{text}` as `{declared_type}` to be value"
        )),
    }
}

fn parse_err(
    schema: &CftSchema,
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

struct ErrorCodeCase {
    code: CellValueErrorCode,
    schema_source: &'static str,
    declared_type: &'static str,
    invalid_text: &'static str,
    adjacent_valid_declared_type: &'static str,
    adjacent_valid_text: &'static str,
}

fn error_code_cases() -> Vec<ErrorCodeCase> {
    vec![
        ErrorCodeCase {
            code: CellValueErrorCode::Syntax,
            schema_source: "",
            declared_type: "[int]",
            invalid_text: "[1 | 2",
            adjacent_valid_declared_type: "[int]",
            adjacent_valid_text: "[1 | 2]",
        },
        ErrorCodeCase {
            code: CellValueErrorCode::InvalidDeclaredType,
            schema_source: "",
            declared_type: "[int",
            invalid_text: "1",
            adjacent_valid_declared_type: "[int]",
            adjacent_valid_text: "1 | 2",
        },
        ErrorCodeCase {
            code: CellValueErrorCode::UnknownType,
            schema_source: "type Item { name: string; }",
            declared_type: "Missing",
            invalid_text: "{}",
            adjacent_valid_declared_type: "Item",
            adjacent_valid_text: "{}",
        },
        ErrorCodeCase {
            code: CellValueErrorCode::UnknownField,
            schema_source: "type Item { name: string; }",
            declared_type: "Item",
            invalid_text: "missing: Sword",
            adjacent_valid_declared_type: "Item",
            adjacent_valid_text: "name: Sword",
        },
        ErrorCodeCase {
            code: CellValueErrorCode::DuplicateField,
            schema_source: "type Item { name: string; }",
            declared_type: "Item",
            invalid_text: "name: Sword, name: Blade",
            adjacent_valid_declared_type: "Item",
            adjacent_valid_text: "name: Sword",
        },
        ErrorCodeCase {
            code: CellValueErrorCode::MissingBoundary,
            schema_source: "type Stats { hp: int; } type Zone { stats: Stats; }",
            declared_type: "Zone",
            invalid_text: "stats: hp: 10",
            adjacent_valid_declared_type: "Zone",
            adjacent_valid_text: "stats: {hp: 10}",
        },
        ErrorCodeCase {
            code: CellValueErrorCode::TypeMismatch,
            schema_source: "",
            declared_type: "int",
            invalid_text: "abc",
            adjacent_valid_declared_type: "int",
            adjacent_valid_text: "1",
        },
        ErrorCodeCase {
            code: CellValueErrorCode::ObjectTypeMismatch,
            schema_source:
                "abstract type Reward {} type CoinReward : Reward { amount: int; } type Item { name: string; }",
            declared_type: "Reward",
            invalid_text: "Item{name: Sword}",
            adjacent_valid_declared_type: "Reward",
            adjacent_valid_text: "CoinReward{amount: 1}",
        },
        ErrorCodeCase {
            code: CellValueErrorCode::AbstractObjectType,
            schema_source: "abstract type Reward {} type CoinReward : Reward { amount: int; }",
            declared_type: "Reward",
            invalid_text: "Reward{}",
            adjacent_valid_declared_type: "Reward",
            adjacent_valid_text: "CoinReward{amount: 1}",
        },
        ErrorCodeCase {
            code: CellValueErrorCode::InvalidEnumVariant,
            schema_source: "enum Rarity { Common, Rare, }",
            declared_type: "Rarity",
            invalid_text: "Missing",
            adjacent_valid_declared_type: "Rarity",
            adjacent_valid_text: "Rare",
        },
        ErrorCodeCase {
            code: CellValueErrorCode::MixedObjectStyle,
            schema_source: "type Stats { hp: int; attack: int; }",
            declared_type: "Stats",
            invalid_text: "hp: 1, 2",
            adjacent_valid_declared_type: "Stats",
            adjacent_valid_text: "hp: 1, attack: 2",
        },
        ErrorCodeCase {
            code: CellValueErrorCode::StringNeedsQuotes,
            schema_source: "",
            declared_type: "string",
            invalid_text: "a,b",
            adjacent_valid_declared_type: "string",
            adjacent_valid_text: r#""a,b""#,
        },
        ErrorCodeCase {
            code: CellValueErrorCode::ReferenceNeedsMarker,
            schema_source: "type Item { name: string; }",
            declared_type: "&Item",
            invalid_text: "item_1",
            adjacent_valid_declared_type: "&Item",
            adjacent_valid_text: "&item_1",
        },
    ]
}

#[test]
fn every_cell_value_error_code_has_negative_and_adjacent_valid_coverage() -> TestResult {
    let declared = declared_error_code_names();
    let covered = error_code_cases()
        .iter()
        .map(|case| format!("{:?}", case.code))
        .collect::<BTreeSet<_>>();
    let missing = declared.difference(&covered).cloned().collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "missing cell value error code coverage: {missing:?}"
        ));
    }

    for case in error_code_cases() {
        let schema = compile_schema(case.schema_source)?;
        let err = parse_err(&schema, case.declared_type, case.invalid_text)?;
        assert_has_code(&err, case.code);
        parse_ok(
            &schema,
            case.adjacent_valid_declared_type,
            case.adjacent_valid_text,
        )?;
    }
    Ok(())
}

fn declared_error_code_names() -> BTreeSet<String> {
    let source = include_str!("../src/cell_value/diagnostics.rs");
    let enum_body = source
        .split("pub enum CellValueErrorCode {")
        .nth(1)
        .and_then(|tail| tail.split('}').next())
        .expect("CellValueErrorCode enum body");

    enum_body
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with("#[") {
                None
            } else {
                Some(line.trim_end_matches(',').to_string())
            }
        })
        .collect()
}

#[test]
fn parses_schema_guided_scalar_values() -> TestResult {
    let schema = compile_schema("")?;

    assert_eq!(
        parse_ok(&schema, "int", "42")?,
        ParsedCell::Value(LoadedValueDraft::Int(42))
    );
    assert_eq!(
        parse_ok(&schema, "float", "2.5")?,
        ParsedCell::Value(LoadedValueDraft::Float(2.5))
    );
    assert_eq!(
        parse_ok(&schema, "bool", "true")?,
        ParsedCell::Value(LoadedValueDraft::Bool(true))
    );
    for accepted in ["TRUE", "True", "1", "yes", "Y"] {
        assert_eq!(
            parse_ok(&schema, "bool", accepted)?,
            ParsedCell::Value(LoadedValueDraft::Bool(true)),
            "{accepted} should parse as true",
        );
    }
    for accepted in ["FALSE", "False", "0", "no", "N"] {
        assert_eq!(
            parse_ok(&schema, "bool", accepted)?,
            ParsedCell::Value(LoadedValueDraft::Bool(false)),
            "{accepted} should parse as false",
        );
    }
    assert_eq!(
        parse_ok(&schema, "string", "hello world")?,
        ParsedCell::Value(LoadedValueDraft::String("hello world".to_string()))
    );
    // string context should NOT coerce these as booleans
    assert_eq!(
        parse_ok(&schema, "string", "1")?,
        ParsedCell::Value(LoadedValueDraft::String("1".to_string()))
    );
    assert_eq!(
        parse_ok(&schema, "string", "yes")?,
        ParsedCell::Value(LoadedValueDraft::String("yes".to_string()))
    );
    assert_eq!(
        parse_ok(&schema, "string", r#""hello, world""#)?,
        ParsedCell::Value(LoadedValueDraft::String("hello, world".to_string()))
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
        ParsedCell::Value(LoadedValueDraft::enum_variant("Rarity", "Rare"))
    );
    assert_eq!(
        parse_ok(&schema, "Rarity", "Rarity.Rare")?,
        ParsedCell::Value(LoadedValueDraft::enum_variant("Rarity", "Rare"))
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
    builder.add_record("monster_1", "Monster", [("stats", stats)]);
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
    builder.add_record("monster_1", "Monster", [("stats", stats)]);
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
fn named_and_positional_object_cells_skip_explicit_underscore_values() -> TestResult {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int = 10;
                attack: int = 50;
                speed: float = 1.0;
            }
            type Monster {
                stats: Stats;
            }
        "#,
    )?;

    let named = parse_value(&schema, "Stats", "hp: 100, attack: _, speed: 2.0")?;
    let positional = parse_value(&schema, "Stats", "100, _, 2.0")?;

    for stats in [named, positional] {
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record("monster_1", "Monster", [("stats", stats)]);
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
    }
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
fn rejects_mixed_unknown_and_excess_object_fields() -> TestResult {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int;
                attack: int;
            }
        "#,
    )?;

    let mixed = parse_err(&schema, "Stats", "hp: 100, 50")?;
    assert_has_code(&mixed, CellValueErrorCode::MixedObjectStyle);

    let unknown = parse_err(&schema, "Stats", "hp: 100, missing: 1")?;
    assert_has_code(&unknown, CellValueErrorCode::UnknownField);

    let too_many = parse_err(&schema, "Stats", "100, 50, 10")?;
    assert_has_code(&too_many, CellValueErrorCode::Syntax);
    Ok(())
}

#[test]
fn rejects_empty_object_field_values() -> TestResult {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int;
                attack: int;
                speed: int = 1;
            }
        "#,
    )?;

    let named = parse_err(&schema, "Stats", "hp: , attack: 2")?;
    assert_has_code(&named, CellValueErrorCode::Syntax);

    let positional = parse_err(&schema, "Stats", "100,,3")?;
    assert_has_code(&positional, CellValueErrorCode::Syntax);
    Ok(())
}

#[test]
fn parses_omitted_and_nullable_cells() -> TestResult {
    let schema = compile_schema("")?;

    assert_eq!(parse_ok(&schema, "int", "")?, ParsedCell::Omitted);
    assert_eq!(parse_ok(&schema, "int", "_")?, ParsedCell::Omitted);
    assert_eq!(
        parse_ok(&schema, "int?", "null")?,
        ParsedCell::Value(LoadedValueDraft::Null)
    );
    let non_nullable_null = parse_err(&schema, "int", "null")?;
    assert_has_code(&non_nullable_null, CellValueErrorCode::TypeMismatch);
    Ok(())
}

#[test]
fn parses_array_cells_with_pipe_delimiters() -> TestResult {
    let schema = compile_schema("")?;

    assert_eq!(
        parse_ok(&schema, "[int]", "1 | 2 | 3")?,
        ParsedCell::Value(LoadedValueDraft::Array(vec![
            LoadedValueDraft::Int(1),
            LoadedValueDraft::Int(2),
            LoadedValueDraft::Int(3),
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
                key: string;
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
        ParsedCell::Value(LoadedValueDraft::Array(vec![
            LoadedValueDraft::object_with_declared_type([
                ("key", LoadedValueDraft::String("slime".to_string())),
                ("level", LoadedValueDraft::Int(5)),
                (
                    "stats",
                    LoadedValueDraft::object_with_declared_type([
                        ("hp", LoadedValueDraft::Int(100)),
                        ("attack", LoadedValueDraft::Int(50)),
                    ]),
                ),
            ]),
            LoadedValueDraft::object_with_declared_type([
                ("key", LoadedValueDraft::String("goblin".to_string())),
                ("level", LoadedValueDraft::Int(10)),
                (
                    "stats",
                    LoadedValueDraft::object_with_declared_type([
                        ("hp", LoadedValueDraft::Int(200)),
                        ("attack", LoadedValueDraft::Int(80)),
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
        ParsedCell::Value(LoadedValueDraft::String("_".to_string()))
    );
    assert_eq!(
        parse_ok(&schema, "string", r#""null""#)?,
        ParsedCell::Value(LoadedValueDraft::String("null".to_string()))
    );
    Ok(())
}

#[test]
fn rejects_invalid_dict_entries_and_key_types() -> TestResult {
    let schema = compile_schema(
        r#"
            enum Rarity { Common, Rare, }
            type Stats { hp: int; }
        "#,
    )?;

    let missing_colon = parse_err(&schema, "{string: int}", "hp 10")?;
    assert_has_code(&missing_colon, CellValueErrorCode::Syntax);

    let bad_int_key = parse_err(&schema, "{int: string}", "one: value")?;
    assert_has_code(&bad_int_key, CellValueErrorCode::TypeMismatch);

    let bad_enum_key = parse_err(&schema, "{Rarity: int}", "Missing: 1")?;
    assert_has_code(&bad_enum_key, CellValueErrorCode::InvalidEnumVariant);

    let invalid_key_type = parse_err(&schema, "{[int]: string}", "[1 | 2]: value")?;
    assert_has_code(&invalid_key_type, CellValueErrorCode::TypeMismatch);

    let missing_nested_value_boundary = parse_err(&schema, "{string: Stats}", "base: hp: 100")?;
    assert_has_code(
        &missing_nested_value_boundary,
        CellValueErrorCode::MissingBoundary,
    );

    let empty_string_key = parse_err(&schema, "{string: int}", ": 1")?;
    assert_has_code(&empty_string_key, CellValueErrorCode::StringNeedsQuotes);
    Ok(())
}

#[test]
fn unknown_declared_enum_names_are_treated_as_object_types() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { key: string; }
        "#,
    )?;

    let err = parse_err(&schema, "MissingEnum", "SomeVariant")?;
    assert_has_code(&err, CellValueErrorCode::UnknownType);
    Ok(())
}

#[test]
fn parses_empty_arrays_dicts_and_objects_with_explicit_boundaries() -> TestResult {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int = 10;
            }
        "#,
    )?;

    assert_eq!(
        parse_ok(&schema, "[int]", "[]")?,
        ParsedCell::Value(LoadedValueDraft::Array(Vec::new()))
    );
    assert_eq!(
        parse_ok(&schema, "{string: int}", "{}")?,
        ParsedCell::Value(LoadedValueDraft::dict(std::iter::empty()))
    );
    assert_eq!(
        parse_ok(&schema, "Stats", "{}")?,
        ParsedCell::Value(LoadedValueDraft::object_with_declared_type(
            std::iter::empty::<(&str, LoadedValueDraft)>()
        ))
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
        ParsedCell::Value(LoadedValueDraft::object_with_declared_type([
            ("name", LoadedValueDraft::String("hello, world".to_string())),
            (
                "notes",
                LoadedValueDraft::Array(vec![
                    LoadedValueDraft::String("a|b".to_string()),
                    LoadedValueDraft::String("c,d".to_string()),
                ]),
            ),
            (
                "attrs",
                LoadedValueDraft::dict([
                    (
                        LoadedDictKeyDraft::from("x:y"),
                        LoadedValueDraft::String("v|1".to_string()),
                    ),
                    (
                        LoadedDictKeyDraft::from("plain"),
                        LoadedValueDraft::String("a,b".to_string()),
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
                key: string;
            }
            type CurrencyReward : Reward {
                amount: int;
            }
            type Item {
                key: string;
            }
        "#,
    )?;

    let abstract_actual = parse_err(&schema, "Reward", "Reward{r1}")?;
    assert_has_code(&abstract_actual, CellValueErrorCode::AbstractObjectType);

    let unknown_actual = parse_err(&schema, "Reward", "MissingReward{r1}")?;
    assert_has_code(&unknown_actual, CellValueErrorCode::UnknownType);

    let unassignable = parse_err(&schema, "Reward", "Item{i1}")?;
    assert_has_code(&unassignable, CellValueErrorCode::ObjectTypeMismatch);
    Ok(())
}

#[test]
fn rejects_malformed_declared_types() -> TestResult {
    let schema = compile_schema("")?;

    for declared_type in [
        "",
        "[int",
        "{string int}",
        "{: int}",
        "{string:}",
        "[]",
        "int extra",
    ] {
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

    let trailing_escape = parse_err(&schema, "string", r#""abc\"#)?;
    assert_has_code(&trailing_escape, CellValueErrorCode::Syntax);

    let unclosed_nested = parse_err(&schema, "[string]", r#"["a" | ["b"]"#)?;
    assert_has_code(&unclosed_nested, CellValueErrorCode::Syntax);
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
        ParsedCell::Value(LoadedValueDraft::String("a\"b".to_string()))
    );

    let err = parse_err(&schema, "string", r#""a"b""#)?;
    assert_has_code(&err, CellValueErrorCode::Syntax);
    Ok(())
}

#[test]
fn rejects_declared_type_and_nested_scan_edge_cases() -> TestResult {
    let schema = compile_schema("")?;

    for declared_type in ["{string int}", "{string: int", "[int}", "int?? extra"] {
        let err = parse_err(&schema, declared_type, "1")?;
        assert_has_code(&err, CellValueErrorCode::InvalidDeclaredType);
    }

    let nested_dict = parse_err(&schema, "[{string: int}]", "a: 1, b: 2")?;
    assert_has_code(&nested_dict, CellValueErrorCode::MissingBoundary);

    let nested_array = parse_err(&schema, "{string: [int]}", "ids: 1 | 2")?;
    assert_has_code(&nested_array, CellValueErrorCode::MissingBoundary);

    let empty_dict_value = parse_err(&schema, "{string: int}", "a:")?;
    assert_has_code(&empty_dict_value, CellValueErrorCode::TypeMismatch);
    Ok(())
}

#[test]
fn object_type_markers_require_identifier_names_and_closed_boundaries() -> TestResult {
    let schema = compile_schema(
        r#"
            abstract type Reward { key: string; }
            type CurrencyReward : Reward { amount: int; }
        "#,
    )?;

    let invalid_marker_name = parse_err(&schema, "Reward", "1Reward{r1, 100}")?;
    assert_has_code(&invalid_marker_name, CellValueErrorCode::StringNeedsQuotes);

    let unclosed_marker = parse_err(&schema, "Reward", "CurrencyReward{r1, 100")?;
    assert_has_code(&unclosed_marker, CellValueErrorCode::Syntax);

    let mismatched_marker = parse_err(&schema, "Reward", "CurrencyReward{r1, 100]")?;
    assert_has_code(&mismatched_marker, CellValueErrorCode::Syntax);

    let mismatched_before_marker = parse_err(&schema, "Reward", "CurrencyReward]{r1, 100}")?;
    assert_has_code(&mismatched_before_marker, CellValueErrorCode::Syntax);

    let invalid_marker_continuation = parse_err(&schema, "Reward", "Currency-Reward{r1, 100}")?;
    assert_has_code(
        &invalid_marker_continuation,
        CellValueErrorCode::StringNeedsQuotes,
    );

    let escaped_marker_brace = parse_err(&schema, "Reward", r#"CurrencyReward"{\"{r1, 100}"#)?;
    assert_has_code(&escaped_marker_brace, CellValueErrorCode::Syntax);

    let marker_with_trailing_text = parse_err(&schema, "Reward", "CurrencyReward{r1, 100} x")?;
    assert_has_code(
        &marker_with_trailing_text,
        CellValueErrorCode::StringNeedsQuotes,
    );
    Ok(())
}

#[test]
fn string_values_accept_standard_escapes_and_reject_bare_boundaries() -> TestResult {
    let schema = compile_schema("")?;

    assert_eq!(
        parse_ok(&schema, "string", r#""line\nnext\t\\\"""#)?,
        ParsedCell::Value(LoadedValueDraft::String("line\nnext\t\\\"".to_string()))
    );

    let null_err = parse_err(&schema, "string", "null")?;
    assert_has_code(&null_err, CellValueErrorCode::TypeMismatch);

    let lone_quote = parse_err(&schema, "string", "\"")?;
    assert_has_code(&lone_quote, CellValueErrorCode::Syntax);

    let trailing_escape_with_closing_quote = parse_err(&schema, "string", "\"abc\\\"")?;
    assert_has_code(
        &trailing_escape_with_closing_quote,
        CellValueErrorCode::Syntax,
    );

    for text in ["has[bracket]", "has}brace"] {
        let err = parse_err(&schema, "string", text)?;
        assert_has_code(&err, CellValueErrorCode::StringNeedsQuotes);
    }
    Ok(())
}

#[test]
fn parses_full_nested_root_object_example() -> TestResult {
    let schema = compile_schema(
        r#"
            abstract type Reward {
                key: string;
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
        ParsedCell::Value(LoadedValueDraft::object_with_declared_type([
            (
                "rewards",
                LoadedValueDraft::Array(vec![
                    LoadedValueDraft::object(
                        "CurrencyReward",
                        [
                            ("key", LoadedValueDraft::String("r1".to_string())),
                            ("amount", LoadedValueDraft::Int(100)),
                        ],
                    ),
                    LoadedValueDraft::object(
                        "ItemReward",
                        [
                            ("key", LoadedValueDraft::String("r2".to_string())),
                            ("item_id", LoadedValueDraft::String("sword_01".to_string())),
                            ("count", LoadedValueDraft::Int(1)),
                        ],
                    ),
                ]),
            ),
            (
                "weights",
                LoadedValueDraft::Array(vec![LoadedValueDraft::Int(60), LoadedValueDraft::Int(40)]),
            ),
        ]))
    );
    Ok(())
}

#[test]
fn renders_runtime_values_as_parseable_table_cell_text() -> TestResult {
    let schema = compile_schema(
        r#"
            enum Rarity { Common = 0, Rare = 10, }
            type Item { name: string; }
            type Stats { hp: int; attack: int; }
            type Drop {
                names: [string] = [];
                item: &Item? = null;
                stats: Stats;
                weights: {string: int} = {};
                rarity: Rarity = Rarity.Common;
            }
        "#,
    )?;

    let names = CfdValue::Array(vec![
        CfdValue::String("weapon".to_string()),
        CfdValue::String("melee, close".to_string()),
    ]);
    let rendered_names = render_cell_value(&names).map_err(|err| err.to_string())?;
    assert_eq!(rendered_names, r#"[weapon | "melee, close"]"#);
    assert_eq!(
        parse_value(&schema, "[string]", &rendered_names)?,
        LoadedValueDraft::Array(vec![
            LoadedValueDraft::String("weapon".to_string()),
            LoadedValueDraft::String("melee, close".to_string()),
        ])
    );

    let reference = CfdValue::record_ref("sword_01").map_err(|err| err.to_string())?;
    let rendered_reference = render_cell_value(&reference).map_err(|err| err.to_string())?;
    assert_eq!(rendered_reference, "&sword_01");
    assert_eq!(
        parse_value(&schema, "&Item", &rendered_reference)?,
        LoadedValueDraft::record_ref("sword_01")
    );

    let dict = CfdValue::Dict(vec![(
        CfdDictKey::String("rare:drop".to_string()),
        CfdValue::Int(10),
    )]);
    let rendered_dict = render_cell_value(&dict).map_err(|err| err.to_string())?;
    assert_eq!(rendered_dict, r#"{"rare:drop": 10}"#);
    assert_eq!(
        parse_value(&schema, "{string: int}", &rendered_dict)?,
        LoadedValueDraft::dict([(
            LoadedDictKeyDraft::String("rare:drop".to_string()),
            LoadedValueDraft::Int(10),
        )])
    );

    let enum_value = CfdValue::Enum(
        CfdEnumValue::try_new("Rarity", Some("Rare"), 10).map_err(|err| err.to_string())?,
    );
    assert_eq!(
        render_cell_value(&enum_value).map_err(|err| err.to_string())?,
        "Rare"
    );

    let stats = CfdValue::Object(Box::new(
        coflow_data_model::CfdObject::try_new(
            "Stats",
            std::collections::BTreeMap::from([
                ("attack".to_string(), CfdValue::Int(20)),
                ("hp".to_string(), CfdValue::Int(100)),
            ]),
        )
        .map_err(|err| err.to_string())?,
    ));
    let rendered_stats = render_cell_value(&stats).map_err(|err| err.to_string())?;
    assert_eq!(rendered_stats, "Stats{attack: 20, hp: 100}");
    let parsed_stats = parse_value(&schema, "Stats", &rendered_stats)?;
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("drop_1", "Drop", [("stats", parsed_stats)]);
    let model = build_model(builder)?;
    let drop = model
        .lookup_assignable(&schema, "Drop", "drop_1")
        .and_then(|id| model.record(id))
        .ok_or_else(|| "expected drop record".to_string())?;
    let Some(CfdValue::Object(parsed)) = drop.field("stats") else {
        return Err("expected parsed stats object".to_string());
    };
    assert_eq!(parsed.field("hp"), Some(&CfdValue::Int(100)));
    assert_eq!(parsed.field("attack"), Some(&CfdValue::Int(20)));
    Ok(())
}

#[test]
fn renders_polymorphic_object_values_with_type_marker() -> TestResult {
    let nested = CfdValue::Object(Box::new(
        coflow_data_model::CfdObject::try_new(
            "ItemReward",
            std::collections::BTreeMap::from([
                ("count".to_string(), CfdValue::Int(1)),
                (
                    "item".to_string(),
                    CfdValue::record_ref("sword").map_err(|err| err.to_string())?,
                ),
            ]),
        )
        .map_err(|err| err.to_string())?,
    ));
    let rendered = render_cell_value(&nested).map_err(|err| err.to_string())?;

    assert_eq!(rendered, "ItemReward{count: 1, item: &sword}".to_string());
    Ok(())
}

#[test]
fn parses_polymorphic_object_type_markers() -> TestResult {
    let schema = compile_schema(
        r#"
            abstract type Reward {
                key: string;
            }
            type CurrencyReward : Reward {
                amount: int;
            }
        "#,
    )?;

    assert_eq!(
        parse_ok(&schema, "Reward", "CurrencyReward{r1, 100}")?,
        ParsedCell::Value(LoadedValueDraft::object(
            "CurrencyReward",
            [
                ("key", LoadedValueDraft::String("r1".to_string())),
                ("amount", LoadedValueDraft::Int(100)),
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
                key: string;
            }
            type 金币奖励 : 奖励 {
                数量: int;
            }
        "#,
    )?;

    assert_eq!(
        parse_ok(&schema, "奖励", "金币奖励{r1, 100}")?,
        ParsedCell::Value(LoadedValueDraft::object(
            "金币奖励",
            [
                ("key", LoadedValueDraft::String("r1".to_string())),
                ("数量", LoadedValueDraft::Int(100)),
            ],
        ))
    );
    Ok(())
}

#[test]
fn object_cells_reject_invalid_record_reference_forms() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                name: string;
            }
            type DropTable {
                rewards: [Item];
            }
        "#,
    )?;

    assert_has_code(
        &parse_err(&schema, "&Item", "@Item.item_1")?,
        CellValueErrorCode::Syntax,
    );
    assert_has_code(
        &parse_err(&schema, "&Item", "@DropTable.drop_table.rewards[0]")?,
        CellValueErrorCode::Syntax,
    );
    Ok(())
}

#[test]
fn ref_cells_parse_direct_record_ref_shorthand_from_expected_type() -> TestResult {
    let schema = compile_schema(
        r#"
            abstract type Reward {}
            type Item {
                name: string;
            }
            type ItemReward : Reward {
                item: &Item;
            }
        "#,
    )?;

    assert_eq!(
        parse_value(&schema, "&Item", "&item_1")?,
        LoadedValueDraft::record_ref("item_1")
    );
    assert_eq!(
        parse_value(&schema, "&Item?", "&item_1")?,
        LoadedValueDraft::record_ref("item_1")
    );
    assert_eq!(
        parse_value(&schema, "[&Item]", "&item_1 | &item_2")?,
        LoadedValueDraft::Array(vec![
            LoadedValueDraft::record_ref("item_1"),
            LoadedValueDraft::record_ref("item_2"),
        ])
    );
    assert_eq!(
        parse_value(&schema, "{string: &Item}", "main: &item_1")?,
        LoadedValueDraft::dict([(
            LoadedDictKeyDraft::from("main"),
            LoadedValueDraft::record_ref("item_1"),
        )])
    );
    assert_eq!(
        parse_value(&schema, "&Reward", "&reward_1")?,
        LoadedValueDraft::record_ref("reward_1")
    );
    assert_eq!(
        parse_value(&schema, "ItemReward", "item: &item_1")?,
        LoadedValueDraft::object_with_declared_type([(
            "item",
            LoadedValueDraft::record_ref("item_1"),
        )])
    );
    assert_has_code(
        &parse_err(&schema, "Item", "&item_1")?,
        CellValueErrorCode::TypeMismatch,
    );
    Ok(())
}

#[test]
fn direct_record_refs_reject_invalid_forms_and_keys() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                name: string;
            }
        "#,
    )?;

    let path = parse_err(&schema, "&Item", "&item_1.name")?;
    assert_has_code(&path, CellValueErrorCode::Syntax);
    assert!(
        path.diagnostics
            .iter()
            .any(|diag| diag.message == "invalid record reference"),
        "expected invalid record reference diagnostic, got {path:?}",
    );

    let invalid_key = parse_err(&schema, "&Item", "&fire-ball")?;
    assert_has_code(&invalid_key, CellValueErrorCode::Syntax);
    assert!(
        invalid_key
            .diagnostics
            .iter()
            .any(|diag| diag.message.contains("invalid reference key")),
        "expected invalid key hint, got {invalid_key:?}",
    );

    assert_eq!(
        parse_ok(&schema, "string", "&item_1.name")?,
        ParsedCell::Value(LoadedValueDraft::String("&item_1.name".to_string()))
    );
    Ok(())
}

#[test]
fn string_cells_keep_at_prefixed_text_as_plain_strings() -> TestResult {
    let schema = compile_schema("")?;

    assert_eq!(
        parse_ok(&schema, "string", "@item_1")?,
        ParsedCell::Value(LoadedValueDraft::String("@item_1".to_string()))
    );
    Ok(())
}

#[test]
fn object_cells_reject_bare_reference_keys_with_marker_hint() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                name: string;
            }
        "#,
    )?;

    let err = parse_err(&schema, "Item", "item_1")?;
    assert_has_code(&err, CellValueErrorCode::ReferenceNeedsMarker);
    assert!(
        err.diagnostics
            .iter()
            .any(|diag| diag.message.contains("&item_1")),
        "expected marker hint, got {err:?}",
    );
    Ok(())
}

#[test]
fn object_cells_reject_invalid_at_references() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type DropTable { rewards: [Item]; }
        "#,
    )?;

    for text in ["@item_1", "@drop_table.rewards[0]", "@Item.fire-ball"] {
        let err = parse_err(&schema, "Item", text)?;
        assert!(
            err.diagnostics.iter().any(|diag| matches!(
                diag.code,
                CellValueErrorCode::Syntax | CellValueErrorCode::UnknownType
            )),
            "expected invalid reference `{text}` to fail as syntax or unknown type, got {err:?}",
        );
        assert!(
            err.diagnostics
                .iter()
                .any(|diag| diag.message == "invalid record reference"),
            "expected invalid record reference diagnostic for `{text}`, got {err:?}",
        );
    }
    Ok(())
}

#[test]
fn explicit_record_refs_resolve_to_cfd_refs() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                name: string;
            }
            type Drop {
                item: &Item;
            }
        "#,
    )?;

    let parsed = parse_value(&schema, "&Item", "&item_1")?;
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item_1",
        "Item",
        [("name", LoadedValueDraft::from("Sword"))],
    );
    builder.add_record("drop_1", "Drop", [("item", parsed)]);
    let model = build_model(builder)?;
    let Some(item_record_id) = model.records().next().map(|(id, _)| id) else {
        return Err("expected item record".to_string());
    };
    let Some((_, drop_record)) = model.records().nth(1) else {
        return Err("expected drop record".to_string());
    };

    assert_eq!(
        drop_record.field("item"),
        Some(&CfdValue::record_ref("item_1").unwrap())
    );
    let _ = item_record_id;
    Ok(())
}

#[test]
fn direct_record_ref_shorthand_resolves_to_cfd_refs_by_expected_type() -> TestResult {
    let schema = compile_schema(
        r#"
            abstract type Reward {}
            type ItemReward : Reward {
                count: int;
            }
            type Drop {
                reward: &Reward;
            }
        "#,
    )?;

    let parsed = parse_value(&schema, "&Reward", "&reward_1")?;
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "reward_1",
        "ItemReward",
        [("count", LoadedValueDraft::from(3_i64))],
    );
    builder.add_record("drop_1", "Drop", [("reward", parsed)]);
    let model = build_model(builder)?;
    let Some(reward_record_id) = model.records().next().map(|(id, _)| id) else {
        return Err("expected reward record".to_string());
    };
    let Some((_, drop_record)) = model.records().nth(1) else {
        return Err("expected drop record".to_string());
    };

    assert_eq!(
        drop_record.field("reward"),
        Some(&CfdValue::record_ref("reward_1").unwrap())
    );
    let _ = reward_record_id;
    Ok(())
}

#[test]
fn parses_dict_cells_with_schema_guided_keys() -> TestResult {
    let schema = compile_schema("")?;

    assert_eq!(
        parse_ok(&schema, "{string: int}", "alice: 10, bob: 20")?,
        ParsedCell::Value(LoadedValueDraft::dict([
            (LoadedDictKeyDraft::from("alice"), LoadedValueDraft::Int(10)),
            (LoadedDictKeyDraft::from("bob"), LoadedValueDraft::Int(20)),
        ]))
    );
    Ok(())
}
