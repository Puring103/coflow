use coflow_cell_value::{parse_cell, CellValueDiagnostics, CellValueErrorCode, ParsedCell};
use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::{CfdDataModel, CfdInputDictKey, CfdInputValue, CfdValue};

fn compile_schema(source: &str) -> CftContainer {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("main"), source)
        .expect("schema should parse");
    container.compile().expect("schema should compile");
    container
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
fn parses_schema_guided_scalar_values() {
    let schema = compile_schema("");

    assert_eq!(
        parse_cell(&schema, "int", "42").expect("int"),
        ParsedCell::Value(CfdInputValue::Int(42))
    );
    assert_eq!(
        parse_cell(&schema, "float", "3.14").expect("float"),
        ParsedCell::Value(CfdInputValue::Float(3.14))
    );
    assert_eq!(
        parse_cell(&schema, "bool", "true").expect("bool"),
        ParsedCell::Value(CfdInputValue::Bool(true))
    );
    assert_eq!(
        parse_cell(&schema, "string", "hello world").expect("string"),
        ParsedCell::Value(CfdInputValue::String("hello world".to_string()))
    );
    assert_eq!(
        parse_cell(&schema, "string", r#""hello, world""#).expect("quoted string"),
        ParsedCell::Value(CfdInputValue::String("hello, world".to_string()))
    );
}

#[test]
fn parses_enum_values_with_or_without_type_prefix() {
    let schema = compile_schema(
        r#"
            enum Rarity { Common = 0, Rare = 10, }
        "#,
    );

    assert_eq!(
        parse_cell(&schema, "Rarity", "Rare").expect("bare enum"),
        ParsedCell::Value(CfdInputValue::enum_variant("Rarity", "Rare"))
    );
    assert_eq!(
        parse_cell(&schema, "Rarity", "Rarity.Rare").expect("qualified enum"),
        ParsedCell::Value(CfdInputValue::enum_variant("Rarity", "Rare"))
    );
}

#[test]
fn parses_positional_object_cells_using_field_order() {
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
    );

    let ParsedCell::Value(stats) = parse_cell(&schema, "Stats", "100, 50").expect("stats") else {
        panic!("expected value");
    };
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Monster", [("stats", stats)]);
    let model = builder.build().expect("model");
    let (_, monster) = model.records().next().expect("monster");
    let Some(CfdValue::Object(stats)) = monster.field("stats") else {
        panic!("expected stats object");
    };

    assert_eq!(stats.field("hp"), Some(&CfdValue::Int(100)));
    assert_eq!(stats.field("attack"), Some(&CfdValue::Int(50)));
    assert_eq!(stats.field("speed"), Some(&CfdValue::Float(1.0)));
}

#[test]
fn parses_named_object_cells_and_omits_skipped_fields() {
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
    );

    let ParsedCell::Value(stats) =
        parse_cell(&schema, "Stats", "speed: 2.0, hp: 100").expect("stats")
    else {
        panic!("expected value");
    };
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Monster", [("stats", stats)]);
    let model = builder.build().expect("model");
    let (_, monster) = model.records().next().expect("monster");
    let Some(CfdValue::Object(stats)) = monster.field("stats") else {
        panic!("expected stats object");
    };

    assert_eq!(stats.field("hp"), Some(&CfdValue::Int(100)));
    assert_eq!(stats.field("attack"), Some(&CfdValue::Int(50)));
    assert_eq!(stats.field("speed"), Some(&CfdValue::Float(2.0)));
}

#[test]
fn rejects_duplicate_named_object_fields() {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int;
                attack: int;
            }
        "#,
    );

    let err = parse_cell(&schema, "Stats", "hp: 100, hp: 200, attack: 50")
        .expect_err("duplicate named field");
    assert_has_code(&err, CellValueErrorCode::DuplicateField);
}

#[test]
fn parses_omitted_and_nullable_cells() {
    let schema = compile_schema("");

    assert_eq!(
        parse_cell(&schema, "int", "").expect("empty"),
        ParsedCell::Omitted
    );
    assert_eq!(
        parse_cell(&schema, "int", "_").expect("skip"),
        ParsedCell::Omitted
    );
    assert_eq!(
        parse_cell(&schema, "int?", "null").expect("null"),
        ParsedCell::Value(CfdInputValue::Null)
    );
}

#[test]
fn parses_array_cells_with_pipe_delimiters() {
    let schema = compile_schema("");

    assert_eq!(
        parse_cell(&schema, "[int]", "1 | 2 | 3").expect("array"),
        ParsedCell::Value(CfdInputValue::Array(vec![
            CfdInputValue::Int(1),
            CfdInputValue::Int(2),
            CfdInputValue::Int(3),
        ]))
    );
}

#[test]
fn parses_nested_objects_inside_array_cells() {
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
    );

    assert_eq!(
        parse_cell(
            &schema,
            "[Monster]",
            "{slime, 5, {100, 50}} | {goblin, 10, {200, 80}}"
        )
        .expect("monsters"),
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
}

#[test]
fn rejects_object_array_elements_without_object_boundaries() {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int;
                attack: int;
            }
        "#,
    );

    let err = parse_cell(&schema, "[Stats]", "100, 50 | 200, 80")
        .expect_err("object array elements need braces");
    assert_has_code(&err, CellValueErrorCode::MissingBoundary);
}

#[test]
fn rejects_nested_composite_values_without_boundaries() {
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
    );

    let missing_array = parse_cell(&schema, "Zone", "forest, 1 | 2, {100, 50}, {hp: 10}")
        .expect_err("nested array needs brackets");
    assert_has_code(&missing_array, CellValueErrorCode::MissingBoundary);

    let missing_object = parse_cell(&schema, "Zone", "forest, [1 | 2], 100, {hp: 10}")
        .expect_err("nested object needs braces");
    assert_has_code(&missing_object, CellValueErrorCode::MissingBoundary);

    let missing_dict = parse_cell(&schema, "Zone", "forest, [1 | 2], {100, 50}, hp: 10")
        .expect_err("nested dict needs braces");
    assert_has_code(&missing_dict, CellValueErrorCode::MissingBoundary);
}

#[test]
fn rejects_comma_arrays_and_bare_special_strings() {
    let schema = compile_schema("");

    let comma_array = parse_cell(&schema, "[string]", "[weapon, melee]").expect_err("comma array");
    assert_has_code(&comma_array, CellValueErrorCode::Syntax);

    for text in ["hello, world", "fire|ice", "key: value"] {
        let err = parse_cell(&schema, "string", text).expect_err("special string needs quotes");
        assert_has_code(&err, CellValueErrorCode::StringNeedsQuotes);
    }

    assert_eq!(
        parse_cell(&schema, "string", r#""_""#).expect("quoted underscore"),
        ParsedCell::Value(CfdInputValue::String("_".to_string()))
    );
    assert_eq!(
        parse_cell(&schema, "string", r#""null""#).expect("quoted null"),
        ParsedCell::Value(CfdInputValue::String("null".to_string()))
    );
}

#[test]
fn keeps_delimiters_inside_quotes_and_nested_boundaries() {
    let schema = compile_schema(
        r#"
            type Payload {
                name: string;
                notes: [string];
                attrs: {string: string};
            }
        "#,
    );

    assert_eq!(
        parse_cell(
            &schema,
            "Payload",
            r#"name: "hello, world", notes: ["a|b" | "c,d"], attrs: {"x:y": "v|1", plain: "a,b"}"#,
        )
        .expect("payload"),
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
}

#[test]
fn validates_polymorphic_type_markers_are_assignable_and_concrete() {
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
    );

    let abstract_actual =
        parse_cell(&schema, "Reward", "Reward{r1}").expect_err("abstract actual type");
    assert_has_code(&abstract_actual, CellValueErrorCode::AbstractObjectType);

    let unassignable =
        parse_cell(&schema, "Reward", "Item{i1}").expect_err("unassignable actual type");
    assert_has_code(&unassignable, CellValueErrorCode::ObjectTypeMismatch);
}

#[test]
fn rejects_malformed_declared_types() {
    let schema = compile_schema("");

    for declared_type in ["", "[int", "{string int}", "{: int}", "{string:}", "[]"] {
        let err = parse_cell(&schema, declared_type, "1").expect_err("invalid declared type");
        assert_has_code(&err, CellValueErrorCode::InvalidDeclaredType);
    }
}

#[test]
fn rejects_unbalanced_boundaries_and_invalid_string_escapes() {
    let schema = compile_schema("");

    let unclosed_array = parse_cell(&schema, "[int]", "[1 | 2").expect_err("unclosed array");
    assert_has_code(&unclosed_array, CellValueErrorCode::Syntax);

    let mismatched = parse_cell(&schema, "[int]", "[1}").expect_err("mismatched boundary");
    assert_has_code(&mismatched, CellValueErrorCode::Syntax);

    let extra_close = parse_cell(&schema, "[int]", "1 | 2]").expect_err("extra close");
    assert_has_code(&extra_close, CellValueErrorCode::Syntax);

    let unclosed_string = parse_cell(&schema, "string", r#""hello"#).expect_err("string");
    assert_has_code(&unclosed_string, CellValueErrorCode::Syntax);

    let invalid_escape = parse_cell(&schema, "string", r#""\x""#).expect_err("escape");
    assert_has_code(&invalid_escape, CellValueErrorCode::Syntax);
}

#[test]
fn parses_full_nested_root_object_example() {
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
    );

    assert_eq!(
        parse_cell(
            &schema,
            "DropTable",
            "rewards: [CurrencyReward{r1, 100} | ItemReward{r2, sword_01, 1}], weights: [60 | 40]",
        )
        .expect("drop table"),
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
}

#[test]
fn parses_polymorphic_object_type_markers() {
    let schema = compile_schema(
        r#"
            abstract type Reward {
                id: string;
            }
            type CurrencyReward : Reward {
                amount: int;
            }
        "#,
    );

    assert_eq!(
        parse_cell(&schema, "Reward", "CurrencyReward{r1, 100}").expect("reward"),
        ParsedCell::Value(CfdInputValue::object(
            "CurrencyReward",
            [
                ("id", CfdInputValue::String("r1".to_string())),
                ("amount", CfdInputValue::Int(100)),
            ],
        ))
    );
}

#[test]
fn parses_unicode_polymorphic_object_type_markers() {
    let schema = compile_schema(
        r#"
            abstract type 奖励 {
                id: string;
            }
            type 金币奖励 : 奖励 {
                数量: int;
            }
        "#,
    );

    assert_eq!(
        parse_cell(&schema, "奖励", "金币奖励{r1, 100}").expect("unicode reward"),
        ParsedCell::Value(CfdInputValue::object(
            "金币奖励",
            [
                ("id", CfdInputValue::String("r1".to_string())),
                ("数量", CfdInputValue::Int(100)),
            ],
        ))
    );
}

#[test]
fn ref_cells_parse_as_their_declared_id_type_for_data_model_resolution() {
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
    );

    let ParsedCell::Value(item_id) = parse_cell(&schema, "string", "item_1").expect("ref id")
    else {
        panic!("expected value");
    };
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [("id", CfdInputValue::String("item_1".to_string()))],
    );
    builder.add_record("Drop", [("item_id", item_id)]);
    let model = builder.build().expect("model");
    let item_record_id = model.records().next().map(|(id, _)| id).expect("item");
    let (_, drop_record) = model.records().nth(1).expect("drop");

    assert_eq!(
        drop_record.field("item_id"),
        Some(&CfdValue::Ref {
            id: "item_1".into(),
            target: item_record_id,
        })
    );
}

#[test]
fn parses_dict_cells_with_schema_guided_keys() {
    let schema = compile_schema("");

    assert_eq!(
        parse_cell(&schema, "{string: int}", "alice: 10, bob: 20").expect("dict"),
        ParsedCell::Value(CfdInputValue::dict([
            (CfdInputDictKey::from("alice"), CfdInputValue::Int(10)),
            (CfdInputDictKey::from("bob"), CfdInputValue::Int(20)),
        ]))
    );
}
