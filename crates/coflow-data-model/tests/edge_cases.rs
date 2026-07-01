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
fn cyclic_record_refs_are_allowed_because_resolution_is_two_phase() {
    let schema = compile_schema(
        r#"
            type Person {
                parent: &Person?;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "alice",
        "Person",
        [("parent", CfdInputValue::record_ref("bob"))],
    );
    builder.add_record(
        "bob",
        "Person",
        [("parent", CfdInputValue::record_ref("alice"))],
    );

    let model = builder.build().expect("cycles should resolve");
    let alice_id = record_id_at(&model, 0);
    let bob_id = record_id_at(&model, 1);
    assert_eq!(
        model
            .record(alice_id)
            .and_then(|record| record.field("parent")),
        Some(&CfdValue::Ref("bob".to_string()))
    );
    assert_eq!(
        model
            .record(bob_id)
            .and_then(|record| record.field("parent")),
        Some(&CfdValue::Ref("alice".to_string()))
    );
}

#[test]
fn unresolved_record_ref_reports_reference_stage_diagnostic() {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type Drop { item: &Item; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "drop_1",
        "Drop",
        [("item", CfdInputValue::record_ref("missing"))],
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
