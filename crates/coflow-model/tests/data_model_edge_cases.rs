#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

#[path = "data_model_common/mod.rs"]
mod common;
use common::*;

#[test]
fn cyclic_record_refs_report_the_closing_reference() {
    let schema = compile_schema(
        r#"
            type Person {
                parent: Option<&Person>;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "alice",
        "Person",
        [(
            "parent",
            LoadedValueDraft::OptionSome(Box::new(LoadedValueDraft::record_ref("bob"))),
        )],
    );
    builder.add_record(
        "bob",
        "Person",
        [(
            "parent",
            LoadedValueDraft::OptionSome(Box::new(LoadedValueDraft::record_ref("alice"))),
        )],
    );

    let err = builder.build().expect_err("record reference cycle should fail");
    let diag = diagnostic_with_code(&err, CfdErrorCode::RefCycle);
    assert_eq!(diag.stage, CfdStage::Reference);
    let primary = diag.primary.as_ref().expect("closing reference label");
    assert!(primary.record.is_some());
    assert_eq!(primary.path, CfdPath::root().field("parent"));
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
        [("item", LoadedValueDraft::record_ref("missing"))],
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
        std::iter::empty::<(&str, LoadedValueDraft)>(),
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
            (
                "rarity",
                LoadedValueDraft::enum_variant("Rarity", "Missing"),
            ),
            ("weight", LoadedValueDraft::from(f64::NAN)),
        ],
    );
    let err = builder.build().expect_err("invalid values should fail");
    assert_has_code(&err, CfdErrorCode::InvalidEnumVariant);
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
}
