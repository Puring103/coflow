#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

mod common;
use common::*;

#[test]
fn path_refs_resolve_fields_arrays_and_enum_dict_keys() {
    let schema = compile_schema(
        r#"
            enum Element { Fire, Ice, }
            type Skill { power: int; }
            type DropTable {
                rewards: [Skill];
                resistances: {Element: float};
            }
            type Holder {
                first_reward: Skill;
                fire_resistance: float;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "table_1",
        "DropTable",
        [
            (
                "rewards",
                CfdInputValue::Array(vec![CfdInputValue::object_with_declared_type([(
                    "power",
                    CfdInputValue::from(7_i64),
                )])]),
            ),
            (
                "resistances",
                CfdInputValue::dict([(
                    CfdInputDictKey::enum_variant("Element", "Fire"),
                    CfdInputValue::from(0.5_f64),
                )]),
            ),
        ],
    );
    builder.add_record(
        "holder_1",
        "Holder",
        [
            (
                "first_reward",
                CfdInputValue::path_ref(
                    "DropTable",
                    "table_1",
                    [
                        CfdRefPathSegment::Field("rewards".to_string()),
                        CfdRefPathSegment::Index(CfdInputRefIndex::Int(0)),
                    ],
                ),
            ),
            (
                "fire_resistance",
                CfdInputValue::path_ref(
                    "DropTable",
                    "table_1",
                    [
                        CfdRefPathSegment::Field("resistances".to_string()),
                        CfdRefPathSegment::Index(CfdInputRefIndex::Variant("Fire".to_string())),
                    ],
                ),
            ),
        ],
    );

    let model = builder.build().expect("path refs should build");
    let holder_id = record_id_at(&model, 1);
    let holder = model.record(holder_id).expect("holder record");
    assert!(matches!(
        holder.field("first_reward"),
        Some(CfdValue::Object(skill)) if skill.field("power") == Some(&CfdValue::Int(7))
    ));
    assert_eq!(holder.field("fire_resistance"), Some(&CfdValue::Float(0.5)));
}

#[test]
fn path_refs_report_array_bounds_and_missing_dict_keys() {
    let schema = compile_schema(
        r#"
            type Skill { power: int; }
            type DropTable {
                rewards: [Skill];
                weights: {string: int};
            }
            type Holder {
                missing_reward: Skill;
                missing_weight: int;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "table_1",
        "DropTable",
        [
            (
                "rewards",
                CfdInputValue::Array(vec![CfdInputValue::object_with_declared_type([(
                    "power",
                    CfdInputValue::from(7_i64),
                )])]),
            ),
            (
                "weights",
                CfdInputValue::dict([(
                    CfdInputDictKey::from("common"),
                    CfdInputValue::from(10_i64),
                )]),
            ),
        ],
    );
    builder.add_record(
        "holder_1",
        "Holder",
        [
            (
                "missing_reward",
                CfdInputValue::path_ref(
                    "DropTable",
                    "table_1",
                    [
                        CfdRefPathSegment::Field("rewards".to_string()),
                        CfdRefPathSegment::Index(CfdInputRefIndex::Int(3)),
                    ],
                ),
            ),
            (
                "missing_weight",
                CfdInputValue::path_ref(
                    "DropTable",
                    "table_1",
                    [
                        CfdRefPathSegment::Field("weights".to_string()),
                        CfdRefPathSegment::Index(CfdInputRefIndex::String("rare".to_string())),
                    ],
                ),
            ),
        ],
    );

    let err = builder.build().expect_err("path refs should fail");
    assert_has_code(&err, CfdErrorCode::CheckIndexOutOfBounds);
    assert_has_code(&err, CfdErrorCode::CheckMissingDictKey);
}

#[test]
fn path_ref_result_type_must_match_destination_field() {
    let schema = compile_schema(
        r#"
            type Skill { power: int; }
            type DropTable { rewards: [Skill]; }
            type Holder { power: int; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "table_1",
        "DropTable",
        [(
            "rewards",
            CfdInputValue::Array(vec![CfdInputValue::object_with_declared_type([(
                "power",
                CfdInputValue::from(7_i64),
            )])]),
        )],
    );
    builder.add_record(
        "holder_1",
        "Holder",
        [(
            "power",
            CfdInputValue::path_ref(
                "DropTable",
                "table_1",
                [
                    CfdRefPathSegment::Field("rewards".to_string()),
                    CfdRefPathSegment::Index(CfdInputRefIndex::Int(0)),
                ],
            ),
        )],
    );

    let err = builder
        .build()
        .expect_err("object path result should not satisfy int field");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
}

#[test]
fn cyclic_record_refs_are_allowed_because_resolution_is_two_phase() {
    let schema = compile_schema(
        r#"
            type Person {
                parent: Person?;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "alice",
        "Person",
        [("parent", CfdInputValue::record_ref("Person", "bob"))],
    );
    builder.add_record(
        "bob",
        "Person",
        [("parent", CfdInputValue::record_ref("Person", "alice"))],
    );

    let model = builder.build().expect("cycles should resolve");
    let alice_id = record_id_at(&model, 0);
    let bob_id = record_id_at(&model, 1);
    assert_eq!(
        model
            .record(alice_id)
            .and_then(|record| record.field("parent")),
        Some(&CfdValue::Ref {
            key: "bob".to_string(),
            target: bob_id,
        })
    );
    assert_eq!(
        model
            .record(bob_id)
            .and_then(|record| record.field("parent")),
        Some(&CfdValue::Ref {
            key: "alice".to_string(),
            target: alice_id,
        })
    );
}

#[test]
fn unresolved_record_ref_reports_reference_stage_diagnostic() {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type Drop { item: Item; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "drop_1",
        "Drop",
        [("item", CfdInputValue::record_ref("Item", "missing"))],
    );

    let err = builder.build().expect_err("missing ref should fail");
    let diag = diagnostic_with_code(&err, CfdErrorCode::RefTargetNotFound);
    assert_eq!(diag.stage, CfdStage::Reference);
    assert_eq!(
        diag.primary.as_ref().map(|label| label.path.clone()),
        Some(CfdPath::root().field("item"))
    );
}

#[test]
fn top_level_abstract_records_are_rejected() {
    let schema = compile_schema(
        r#"
            abstract type Reward {}
            type CoinReward : Reward { amount: int; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "reward_1",
        "Reward",
        std::iter::empty::<(&str, CfdInputValue)>(),
    );
    let err = builder
        .build()
        .expect_err("abstract top-level record should fail");
    assert_has_code(&err, CfdErrorCode::AbstractRecordType);
}

#[test]
fn invalid_enum_and_non_finite_float_inputs_are_rejected() {
    let schema = compile_schema(
        r#"
            enum Rarity { Common, Rare, }
            type Item {
                rarity: Rarity;
                weight: float;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "item_1",
        "Item",
        [
            ("rarity", CfdInputValue::enum_variant("Rarity", "Missing")),
            ("weight", CfdInputValue::from(f64::NAN)),
        ],
    );
    let err = builder.build().expect_err("invalid values should fail");
    assert_has_code(&err, CfdErrorCode::InvalidEnumVariant);
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
}
