//! Edge-case and regression tests for `coflow-data-model`.
//!
//! Organised into four sections:
//! 1. Public API regression tests (record_count, lookup, tables iterator, etc.)
//! 2. Diagnostic path correctness (especially dict keys).
//! 3. Type-system edge cases (nullables, nesting, recursion).
//! 4. Inheritance + reference resolution edge cases.

mod common;
use common::*;
use std::collections::BTreeMap;

// ───────────────────────────────────────────────────────────────────────────
// 1. Public API regression tests
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn record_count_is_zero_for_empty_model() {
    let schema = compile_schema(r#"type Item { @id id: string; }"#);
    let model = CfdDataModel::builder(&schema)
        .build()
        .expect("empty model should build");
    assert_eq!(model.record_count(), 0);
    assert!(model.is_empty());
}

#[test]
fn record_count_tracks_top_level_records() {
    let schema = compile_schema(r#"type Item { @id id: string; }"#);
    let mut builder = CfdDataModel::builder(&schema);
    for i in 0..5 {
        builder.add_record("Item", [("id", CfdInputValue::from(format!("item_{i}")))]);
    }
    let model = builder.build().expect("model should build");
    assert_eq!(model.record_count(), 5);
    assert!(!model.is_empty());
}

#[test]
fn records_of_type_iterates_in_insertion_order() {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Quest { @id qid: string; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("a"))]);
    builder.add_record("Quest", [("qid", CfdInputValue::from("q1"))]);
    builder.add_record("Item", [("id", CfdInputValue::from("b"))]);
    builder.add_record("Item", [("id", CfdInputValue::from("c"))]);
    builder.add_record("Quest", [("qid", CfdInputValue::from("q2"))]);

    let model = builder.build().expect("model should build");

    let item_ids: Vec<&CfdValue> = model
        .records_of_type("Item")
        .filter_map(|(_, rec)| rec.field("id"))
        .collect();
    assert_eq!(
        item_ids,
        vec![
            &CfdValue::String("a".to_string()),
            &CfdValue::String("b".to_string()),
            &CfdValue::String("c".to_string()),
        ]
    );

    let quest_ids: Vec<&CfdValue> = model
        .records_of_type("Quest")
        .filter_map(|(_, rec)| rec.field("qid"))
        .collect();
    assert_eq!(
        quest_ids,
        vec![
            &CfdValue::String("q1".to_string()),
            &CfdValue::String("q2".to_string()),
        ]
    );
}

#[test]
fn records_of_type_returns_empty_for_unknown_type() {
    let schema = compile_schema(r#"type Item { @id id: string; }"#);
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("a"))]);
    let model = builder.build().unwrap();

    assert_eq!(model.records_of_type("Nonexistent").count(), 0);
}

#[test]
fn tables_iterator_yields_all_typed_tables() {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Quest { @id qid: string; }
            type Currency { @id cid: string; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("a"))]);
    builder.add_record("Quest", [("qid", CfdInputValue::from("q1"))]);
    let model = builder.build().unwrap();

    let names: Vec<&str> = model.tables().map(|(name, _)| name).collect();
    // Empty Currency table is not produced (only types with records get tables).
    assert_eq!(names, vec!["Item", "Quest"]);

    let item_table = model.table("Item").unwrap();
    assert_eq!(item_table.records.len(), 1);
}

#[test]
fn lookup_finds_records_in_concrete_table() {
    let schema = compile_schema(r#"type Item { @id id: string; value: int; }"#);
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("a")),
            ("value", CfdInputValue::from(1_i64)),
        ],
    );
    let model = builder.build().unwrap();
    let id = record_id_at(&model, 0);

    assert_eq!(model.lookup("Item", &CfdIdValue::from("a")), Some(id));
    assert_eq!(model.lookup("Item", &CfdIdValue::from("missing")), None);
    assert_eq!(model.lookup("Unknown", &CfdIdValue::from("a")), None);
}

#[test]
fn lookup_finds_records_in_polymorphic_range() {
    let schema = compile_schema(
        r#"
            abstract type Reward { @id id: string; }
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "ItemReward",
        [
            ("id", CfdInputValue::from("r1")),
            ("count", CfdInputValue::from(1_i64)),
        ],
    );
    builder.add_record(
        "CurrencyReward",
        [
            ("id", CfdInputValue::from("r2")),
            ("amount", CfdInputValue::from(50_i64)),
        ],
    );
    let model = builder.build().unwrap();
    let item_reward_id = record_id_at(&model, 0);
    let currency_reward_id = record_id_at(&model, 1);

    // Both ids are reachable through the abstract Reward range.
    assert_eq!(
        model.lookup("Reward", &CfdIdValue::from("r1")),
        Some(item_reward_id)
    );
    assert_eq!(
        model.lookup("Reward", &CfdIdValue::from("r2")),
        Some(currency_reward_id)
    );

    // And through their concrete types.
    assert_eq!(
        model.lookup("ItemReward", &CfdIdValue::from("r1")),
        Some(item_reward_id)
    );
    assert_eq!(
        model.lookup("CurrencyReward", &CfdIdValue::from("r2")),
        Some(currency_reward_id)
    );

    // Looking up a CurrencyReward id under ItemReward fails (different concrete table).
    assert_eq!(model.lookup("ItemReward", &CfdIdValue::from("r2")), None);
}

#[test]
fn polymorphic_index_returns_none_for_concrete_types_without_descendants() {
    let schema = compile_schema(r#"type Item { @id id: string; }"#);
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("a"))]);
    let model = builder.build().unwrap();

    assert!(model.polymorphic_index("Item").is_none());
    assert!(model.polymorphic_index("Unknown").is_none());
}

#[test]
fn polymorphic_index_returns_some_for_abstract_root() {
    let schema = compile_schema(
        r#"
            abstract type Reward { @id id: string; }
            type ItemReward : Reward { count: int; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "ItemReward",
        [
            ("id", CfdInputValue::from("r1")),
            ("count", CfdInputValue::from(1_i64)),
        ],
    );
    let model = builder.build().unwrap();

    let index = model.polymorphic_index("Reward").unwrap();
    assert_eq!(index.root_type, "Reward");
    assert!(index.records.contains_key(&CfdIdValue::from("r1")));
}

// ───────────────────────────────────────────────────────────────────────────
// 2. Diagnostic path correctness (regression for dict-key path fix)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn dict_path_uses_actual_string_key_for_value_errors() {
    let schema = compile_schema(
        r#"
            type Item {
                @id id: string;
                attrs: {string: int};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("i1")),
            (
                "attrs",
                CfdInputValue::dict([
                    (CfdInputDictKey::from("ok"), CfdInputValue::from(1_i64)),
                    (
                        CfdInputDictKey::from("broken"),
                        CfdInputValue::from("not_int"),
                    ),
                ]),
            ),
        ],
    );

    let err = builder.build().expect_err("dict value type mismatch");
    let diag = diagnostic_with_code(&err, CfdErrorCode::TypeMismatch);
    let segments = primary_path_segments(diag);
    assert_eq!(
        segments,
        &[
            CfdPathSegment::Field("attrs".to_string()),
            CfdPathSegment::DictKey("\"broken\"".to_string()),
        ]
    );
}

#[test]
fn dict_path_uses_int_key_for_value_errors() {
    let schema = compile_schema(
        r#"
            type Item {
                @id id: string;
                slots: {int: string};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("i1")),
            (
                "slots",
                CfdInputValue::dict([
                    (CfdInputDictKey::from(1_i64), CfdInputValue::from("ok")),
                    (CfdInputDictKey::from(42_i64), CfdInputValue::from(99_i64)),
                ]),
            ),
        ],
    );

    let err = builder.build().expect_err("dict value type mismatch");
    let diag = diagnostic_with_code(&err, CfdErrorCode::TypeMismatch);
    let segments = primary_path_segments(diag);
    assert_eq!(
        segments,
        &[
            CfdPathSegment::Field("slots".to_string()),
            CfdPathSegment::DictKey("42".to_string()),
        ]
    );
}

#[test]
fn dict_path_uses_enum_variant_for_value_errors() {
    let schema = compile_schema(
        r#"
            enum Element { Fire, Ice, Lightning, }
            type Monster {
                @id id: string;
                resist: {Element: float};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Monster",
        [
            ("id", CfdInputValue::from("m1")),
            (
                "resist",
                CfdInputValue::dict([
                    (
                        CfdInputDictKey::enum_variant("Element", "Fire"),
                        CfdInputValue::from(0.5_f64),
                    ),
                    (
                        CfdInputDictKey::enum_variant("Element", "Ice"),
                        CfdInputValue::from("not_float"),
                    ),
                ]),
            ),
        ],
    );

    let err = builder.build().expect_err("dict value type mismatch");
    let diag = diagnostic_with_code(&err, CfdErrorCode::TypeMismatch);
    let segments = primary_path_segments(diag);
    assert_eq!(
        segments,
        &[
            CfdPathSegment::Field("resist".to_string()),
            CfdPathSegment::DictKey("Element.Ice".to_string()),
        ]
    );
}

#[test]
fn dict_path_for_invalid_key_uses_input_key_form() {
    // When the key itself fails validation, we don't have a CfdDictKey yet,
    // so the path uses the raw input form.
    let schema = compile_schema(
        r#"
            enum Rarity { Common, Rare, }
            type Item {
                @id id: string;
                bag: {Rarity: int};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("i1")),
            (
                "bag",
                CfdInputValue::dict([(
                    CfdInputDictKey::enum_variant("Rarity", "MissingVariant"),
                    CfdInputValue::from(1_i64),
                )]),
            ),
        ],
    );

    let err = builder.build().expect_err("invalid enum key variant");
    let diag = diagnostic_with_code(&err, CfdErrorCode::InvalidEnumVariant);
    let segments = primary_path_segments(diag);
    assert_eq!(
        segments,
        &[
            CfdPathSegment::Field("bag".to_string()),
            CfdPathSegment::DictKey("Rarity.MissingVariant".to_string()),
        ]
    );
}

#[test]
fn duplicate_dict_key_reports_actual_key_in_path() {
    let schema = compile_schema(
        r#"
            type Item {
                @id id: string;
                attrs: {string: int};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("i1")),
            (
                "attrs",
                CfdInputValue::dict([
                    (CfdInputDictKey::from("dup"), CfdInputValue::from(1_i64)),
                    (CfdInputDictKey::from("dup"), CfdInputValue::from(2_i64)),
                ]),
            ),
        ],
    );

    let err = builder.build().expect_err("duplicate key");
    let diag = diagnostic_with_code(&err, CfdErrorCode::DuplicateDictKey);
    let segments = primary_path_segments(diag);
    assert_eq!(
        segments,
        &[
            CfdPathSegment::Field("attrs".to_string()),
            CfdPathSegment::DictKey("\"dup\"".to_string()),
        ]
    );
    assert!(!diag.related.is_empty());
}

#[test]
fn nested_field_error_reports_full_path() {
    let schema = compile_schema(
        r#"
            type Stats { hp: int; speed: float; }
            type Monster {
                @id id: string;
                stats: Stats;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Monster",
        [
            ("id", CfdInputValue::from("m1")),
            (
                "stats",
                CfdInputValue::object_with_declared_type([
                    ("hp", CfdInputValue::from("not_int")),
                    ("speed", CfdInputValue::from(1.5_f64)),
                ]),
            ),
        ],
    );

    let err = builder.build().expect_err("nested type error");
    let diag = diagnostic_with_code(&err, CfdErrorCode::TypeMismatch);
    let segments = primary_path_segments(diag);
    assert_eq!(
        segments,
        &[
            CfdPathSegment::Field("stats".to_string()),
            CfdPathSegment::Field("hp".to_string()),
        ]
    );
}

#[test]
fn array_element_error_reports_index_path() {
    let schema = compile_schema(
        r#"
            type Item {
                @id id: string;
                tags: [int];
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("i1")),
            (
                "tags",
                CfdInputValue::Array(vec![
                    CfdInputValue::from(1_i64),
                    CfdInputValue::from("wrong"),
                    CfdInputValue::from(3_i64),
                ]),
            ),
        ],
    );

    let err = builder.build().expect_err("array element error");
    let diag = diagnostic_with_code(&err, CfdErrorCode::TypeMismatch);
    let segments = primary_path_segments(diag);
    assert_eq!(
        segments,
        &[
            CfdPathSegment::Field("tags".to_string()),
            CfdPathSegment::Index(1),
        ]
    );
}

// ───────────────────────────────────────────────────────────────────────────
// 3. Type-system edge cases
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn nullable_field_accepts_explicit_null() {
    let schema = compile_schema(
        r#"
            type Item {
                @id id: string;
                maybe: int?;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("i1")),
            ("maybe", CfdInputValue::Null),
        ],
    );
    let model = builder.build().expect("nullable null should succeed");
    let id = record_id_at(&model, 0);
    assert_eq!(
        model.record(id).and_then(|r| r.field("maybe")),
        Some(&CfdValue::Null)
    );
}

#[test]
fn nullable_field_accepts_inner_value() {
    let schema = compile_schema(
        r#"
            type Item {
                @id id: string;
                maybe: int?;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("i1")),
            ("maybe", CfdInputValue::from(7_i64)),
        ],
    );
    let model = builder.build().unwrap();
    let id = record_id_at(&model, 0);
    assert_eq!(
        model.record(id).and_then(|r| r.field("maybe")),
        Some(&CfdValue::Int(7))
    );
}

#[test]
fn non_nullable_field_rejects_null() {
    let schema = compile_schema(
        r#"
            type Item {
                @id id: string;
                value: int;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("i1")),
            ("value", CfdInputValue::Null),
        ],
    );
    let err = builder.build().expect_err("non-null required");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
}

#[test]
fn nullable_array_accepts_null() {
    let schema = compile_schema(
        r#"
            type Item {
                @id id: string;
                tags: [string]?;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("i1")),
            ("tags", CfdInputValue::Null),
        ],
    );
    let model = builder.build().unwrap();
    let id = record_id_at(&model, 0);
    assert_eq!(
        model.record(id).and_then(|r| r.field("tags")),
        Some(&CfdValue::Null)
    );
}

#[test]
fn nested_array_validates_recursively() {
    let schema = compile_schema(
        r#"
            type Grid {
                @id id: string;
                cells: [[int]];
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Grid",
        [
            ("id", CfdInputValue::from("g1")),
            (
                "cells",
                CfdInputValue::Array(vec![
                    CfdInputValue::Array(vec![
                        CfdInputValue::from(1_i64),
                        CfdInputValue::from(2_i64),
                    ]),
                    CfdInputValue::Array(vec![CfdInputValue::from(3_i64)]),
                ]),
            ),
        ],
    );
    let model = builder.build().unwrap();
    let id = record_id_at(&model, 0);
    let CfdValue::Array(rows) = model.record(id).unwrap().field("cells").unwrap() else {
        panic!("expected nested array");
    };
    assert_eq!(rows.len(), 2);
    let CfdValue::Array(first_row) = &rows[0] else {
        panic!("expected inner array");
    };
    assert_eq!(first_row.len(), 2);
    assert_eq!(first_row[0], CfdValue::Int(1));
}

#[test]
fn nested_array_reports_inner_type_mismatch_with_full_path() {
    let schema = compile_schema(
        r#"
            type Grid {
                @id id: string;
                cells: [[int]];
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Grid",
        [
            ("id", CfdInputValue::from("g1")),
            (
                "cells",
                CfdInputValue::Array(vec![CfdInputValue::Array(vec![
                    CfdInputValue::from(1_i64),
                    CfdInputValue::from("wrong"),
                ])]),
            ),
        ],
    );

    let err = builder.build().expect_err("inner element wrong type");
    let diag = diagnostic_with_code(&err, CfdErrorCode::TypeMismatch);
    let segments = primary_path_segments(diag);
    assert_eq!(
        segments,
        &[
            CfdPathSegment::Field("cells".to_string()),
            CfdPathSegment::Index(0),
            CfdPathSegment::Index(1),
        ]
    );
}

#[test]
fn dict_with_int_keys_round_trips_correctly() {
    let schema = compile_schema(
        r#"
            type Item {
                @id id: string;
                slots: {int: string};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("i1")),
            (
                "slots",
                CfdInputValue::dict([
                    (CfdInputDictKey::from(1_i64), CfdInputValue::from("a")),
                    (CfdInputDictKey::from(2_i64), CfdInputValue::from("b")),
                ]),
            ),
        ],
    );
    let model = builder.build().unwrap();
    let id = record_id_at(&model, 0);
    let CfdValue::Dict(slots) = model.record(id).unwrap().field("slots").unwrap() else {
        panic!("expected dict");
    };
    assert_eq!(slots.len(), 2);
    assert_eq!(
        slots.get(&CfdDictKey::Int(1)),
        Some(&CfdValue::String("a".to_string()))
    );
    assert_eq!(
        slots.get(&CfdDictKey::Int(2)),
        Some(&CfdValue::String("b".to_string()))
    );
}

#[test]
fn dict_with_enum_keys_resolves_variant_values() {
    let schema = compile_schema(
        r#"
            enum Element { Fire = 1, Ice = 2, }
            type Monster {
                @id id: string;
                resist: {Element: float};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Monster",
        [
            ("id", CfdInputValue::from("m1")),
            (
                "resist",
                CfdInputValue::dict([
                    (
                        CfdInputDictKey::enum_variant("Element", "Fire"),
                        CfdInputValue::from(0.25_f64),
                    ),
                    (
                        CfdInputDictKey::enum_variant("Element", "Ice"),
                        CfdInputValue::from(0.75_f64),
                    ),
                ]),
            ),
        ],
    );
    let model = builder.build().unwrap();
    let id = record_id_at(&model, 0);
    let CfdValue::Dict(resist) = model.record(id).unwrap().field("resist").unwrap() else {
        panic!("expected dict");
    };
    assert_eq!(resist.len(), 2);
    let fire_key = CfdDictKey::Enum(CfdEnumValue {
        enum_name: "Element".to_string(),
        variant: "Fire".to_string(),
        value: 1,
    });
    assert_eq!(resist.get(&fire_key), Some(&CfdValue::Float(0.25)));
}

#[test]
fn float_field_rejects_int_value() {
    // CFT is strict about int/float separation.
    let schema = compile_schema(
        r#"
            type Item {
                @id id: string;
                speed: float;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("i1")),
            ("speed", CfdInputValue::from(1_i64)),
        ],
    );
    let err = builder.build().expect_err("int passed for float");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
}

#[test]
fn int_id_field_works_for_indexing_and_refs() {
    let schema = compile_schema(
        r#"
            type Item { @id id: int; }
            type Drop { @ref(Item) item_id: int; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from(42_i64))]);
    builder.add_record(
        "Drop",
        [("item_id", CfdInputValue::Ref(CfdIdValue::from(42_i64)))],
    );

    let model = builder.build().unwrap();
    let item_id = record_id_at(&model, 0);
    let drop_id = record_id_at(&model, 1);
    assert_eq!(
        model.record(drop_id).and_then(|r| r.field("item_id")),
        Some(&CfdValue::Ref {
            id: CfdIdValue::Int(42),
            target: item_id,
        })
    );
}

#[test]
fn wrong_id_value_type_rejected() {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Drop { @ref(Item) item_id: string; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("a"))]);
    // The @ref field expects a string id but we pass an int.
    builder.add_record(
        "Drop",
        [("item_id", CfdInputValue::Ref(CfdIdValue::from(42_i64)))],
    );

    let err = builder.build().expect_err("wrong id type");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
}

#[test]
fn empty_array_default_does_not_constrain_element_type() {
    let schema = compile_schema(
        r#"
            type Item {
                @id id: string;
                tags: [int] = [];
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("a"))]);
    let model = builder.build().unwrap();
    let id = record_id_at(&model, 0);
    assert_eq!(
        model.record(id).and_then(|r| r.field("tags")),
        Some(&CfdValue::Array(Vec::new()))
    );
}

// ───────────────────────────────────────────────────────────────────────────
// 4. Inheritance + reference resolution edge cases
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn inherited_id_field_indexes_concrete_table() {
    let schema = compile_schema(
        r#"
            abstract type Base { @id id: string; }
            type Child : Base { value: int; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Child",
        [
            ("id", CfdInputValue::from("child_1")),
            ("value", CfdInputValue::from(10_i64)),
        ],
    );
    let model = builder.build().unwrap();
    let id = record_id_at(&model, 0);
    let table = model.table("Child").unwrap();
    assert_eq!(
        table.primary_index.get(&CfdIdValue::from("child_1")),
        Some(&id)
    );
}

#[test]
fn inherited_index_field_appears_in_secondary_indexes() {
    let schema = compile_schema(
        r#"
            enum Rarity { Common, Rare, }
            abstract type Base {
                @id id: string;
                @index
                rarity: Rarity;
            }
            type Child : Base { value: int; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Child",
        [
            ("id", CfdInputValue::from("c1")),
            ("rarity", CfdInputValue::enum_variant("Rarity", "Rare")),
            ("value", CfdInputValue::from(5_i64)),
        ],
    );
    let model = builder.build().unwrap();
    let table = model.table("Child").unwrap();
    assert!(table.secondary_indexes.contains_key("rarity"));
}

#[test]
fn multi_level_inheritance_collects_all_fields() {
    let schema = compile_schema(
        r#"
            abstract type A { @id id: string; }
            abstract type B : A { name: string; }
            type C : B { value: int; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "C",
        [
            ("id", CfdInputValue::from("c")),
            ("name", CfdInputValue::from("hello")),
            ("value", CfdInputValue::from(3_i64)),
        ],
    );
    let model = builder.build().unwrap();
    let id = record_id_at(&model, 0);
    let record = model.record(id).unwrap();
    assert_eq!(record.field("id"), Some(&CfdValue::String("c".to_string())));
    assert_eq!(
        record.field("name"),
        Some(&CfdValue::String("hello".to_string()))
    );
    assert_eq!(record.field("value"), Some(&CfdValue::Int(3)));
}

#[test]
fn nullable_ref_with_null_value_succeeds() {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Drop {
                @ref(Item)
                item_id: string?;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("i1"))]);
    builder.add_record("Drop", [("item_id", CfdInputValue::Null)]);
    let model = builder.build().unwrap();
    let drop_id = record_id_at(&model, 1);
    assert_eq!(
        model.record(drop_id).and_then(|r| r.field("item_id")),
        Some(&CfdValue::Null)
    );
}

#[test]
fn forward_reference_resolves_when_target_added_later() {
    // The Drop record is added BEFORE the Item it references.
    // Because resolution happens in a second pass, this should still work.
    let schema = compile_schema(
        r#"
            type Item { @id id: string; }
            type Drop { @ref(Item) item_id: string; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Drop",
        [("item_id", CfdInputValue::Ref(CfdIdValue::from("late")))],
    );
    builder.add_record("Item", [("id", CfdInputValue::from("late"))]);
    let model = builder.build().expect("forward ref should resolve");

    let drop_id = record_id_at(&model, 0);
    let item_id = record_id_at(&model, 1);
    assert_eq!(
        model.record(drop_id).and_then(|r| r.field("item_id")),
        Some(&CfdValue::Ref {
            id: CfdIdValue::from("late"),
            target: item_id,
        })
    );
}

#[test]
fn self_referential_record_resolves_correctly() {
    let schema = compile_schema(
        r#"
            type Person {
                @id
                id: string;
                @ref(Person)
                parent_id: string?;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Person",
        [
            ("id", CfdInputValue::from("alice")),
            ("parent_id", CfdInputValue::Null),
        ],
    );
    builder.add_record(
        "Person",
        [
            ("id", CfdInputValue::from("bob")),
            ("parent_id", CfdInputValue::Ref(CfdIdValue::from("alice"))),
        ],
    );
    let model = builder.build().unwrap();
    let alice_id = record_id_at(&model, 0);
    let bob_id = record_id_at(&model, 1);
    assert_eq!(
        model.record(bob_id).and_then(|r| r.field("parent_id")),
        Some(&CfdValue::Ref {
            id: CfdIdValue::from("alice"),
            target: alice_id,
        })
    );
}

#[test]
fn polymorphic_object_field_with_actual_type_uses_concrete_fields() {
    let schema = compile_schema(
        r#"
            abstract type Reward { @id id: string; }
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
            type Drop { reward: Reward; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Drop",
        [(
            "reward",
            CfdInputValue::object(
                "CurrencyReward",
                [
                    ("id", CfdInputValue::from("r1")),
                    ("amount", CfdInputValue::from(99_i64)),
                ],
            ),
        )],
    );
    let model = builder.build().unwrap();
    let drop_id = record_id_at(&model, 0);
    let CfdValue::Object(reward) = model
        .record(drop_id)
        .and_then(|r| r.field("reward"))
        .unwrap()
    else {
        panic!("expected reward object");
    };
    assert_eq!(reward.actual_type, "CurrencyReward");
    assert_eq!(reward.field("amount"), Some(&CfdValue::Int(99)));
}

#[test]
fn polymorphic_object_with_non_assignable_actual_type_rejected() {
    let schema = compile_schema(
        r#"
            type A { @id id: string; }
            type B { @id id: string; }
            type Holder { item: A; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Holder",
        [(
            "item",
            CfdInputValue::object("B", [("id", CfdInputValue::from("b1"))]),
        )],
    );
    let err = builder.build().expect_err("type B not assignable to A");
    assert_has_code(&err, CfdErrorCode::ObjectTypeMismatch);
}

#[test]
fn deep_nested_default_values_propagate() {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int = 100;
                speed: float = 1.0;
            }
            type Monster {
                @id id: string;
                stats: Stats;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Monster",
        [
            ("id", CfdInputValue::from("m1")),
            (
                "stats",
                CfdInputValue::object_with_declared_type(
                    std::iter::empty::<(&str, CfdInputValue)>(),
                ),
            ),
        ],
    );
    let model = builder.build().unwrap();
    let id = record_id_at(&model, 0);
    let CfdValue::Object(stats) = model.record(id).unwrap().field("stats").unwrap() else {
        panic!("expected stats");
    };
    assert_eq!(stats.field("hp"), Some(&CfdValue::Int(100)));
    assert_eq!(stats.field("speed"), Some(&CfdValue::Float(1.0)));
}

#[test]
fn missing_id_field_value_at_top_level_reports_missing_id_field() {
    // The id field exists in the schema, but the record provides nothing for it
    // and the field has no default. This path goes through MissingRequiredField
    // not MissingIdField (since the record fails validation before indexing).
    let schema = compile_schema(r#"type Item { @id id: string; }"#);
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_input_record(CfdInputRecord::new(
        "Item",
        std::iter::empty::<(&str, CfdInputValue)>(),
    ));
    let err = builder.build().expect_err("missing id");
    assert_has_code(&err, CfdErrorCode::MissingRequiredField);
}

#[test]
fn type_with_no_id_field_has_no_primary_index() {
    let schema = compile_schema(r#"type Item { value: int; }"#);
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("value", CfdInputValue::from(1_i64))]);
    builder.add_record("Item", [("value", CfdInputValue::from(2_i64))]);
    let model = builder.build().unwrap();

    let table = model.table("Item").unwrap();
    assert_eq!(table.records.len(), 2);
    assert!(table.primary_index.is_empty());
}

#[test]
fn dict_value_in_resolved_form_is_btreemap_ordered() {
    // Regression for the BTreeMap migration: keys come out in sorted order
    // regardless of insertion order.
    let schema = compile_schema(
        r#"
            type Item {
                @id id: string;
                attrs: {string: int};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("i1")),
            (
                "attrs",
                CfdInputValue::dict([
                    (CfdInputDictKey::from("zeta"), CfdInputValue::from(3_i64)),
                    (CfdInputDictKey::from("alpha"), CfdInputValue::from(1_i64)),
                    (CfdInputDictKey::from("mu"), CfdInputValue::from(2_i64)),
                ]),
            ),
        ],
    );
    let model = builder.build().unwrap();
    let id = record_id_at(&model, 0);
    let CfdValue::Dict(attrs) = model.record(id).unwrap().field("attrs").unwrap() else {
        panic!("expected dict");
    };

    let keys: Vec<&CfdDictKey> = attrs.keys().collect();
    assert_eq!(
        keys,
        vec![
            &CfdDictKey::String("alpha".to_string()),
            &CfdDictKey::String("mu".to_string()),
            &CfdDictKey::String("zeta".to_string()),
        ]
    );
}

#[test]
fn build_consumes_builder_so_repeated_build_is_compile_error() {
    // This test documents the API: build() consumes self, so calling it twice
    // is statically prevented. We only need it to compile.
    let schema = compile_schema(r#"type Item { @id id: string; }"#);
    let _ = CfdDataModel::builder(&schema).build();

    // Uncommenting the following would be a compile error:
    // let builder = CfdDataModel::builder(&schema);
    // let _ = builder.build();
    // let _ = builder.build();
}

// Smoke check: CfdDataModel still maintains BTreeMap representation for empty defaults.
#[test]
fn empty_dict_default_uses_btreemap() {
    let schema = compile_schema(
        r#"
            type Item {
                @id id: string;
                attrs: {string: int} = {};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("Item", [("id", CfdInputValue::from("i1"))]);
    let model = builder.build().unwrap();
    let id = record_id_at(&model, 0);
    assert_eq!(
        model.record(id).and_then(|r| r.field("attrs")),
        Some(&CfdValue::Dict(BTreeMap::new()))
    );
}
