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
fn cyclic_record_refs_build_and_remain_resolvable() {
    let schema = compile_schema(
        r#"
            table Person {
                parent: Person?;
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
    builder.add_record(
        "self_link",
        "Person",
        [(
            "parent",
            LoadedValueDraft::OptionSome(Box::new(LoadedValueDraft::record_ref("self_link"))),
        )],
    );

    let model = builder.build().expect("record reference cycles are valid");
    let alice = model
        .lookup_assignable(&schema, "Person", "alice")
        .expect("alice");
    let bob = model
        .lookup_assignable(&schema, "Person", "bob")
        .expect("bob");
    let self_record = model
        .lookup_assignable(&schema, "Person", "self_link")
        .expect("self_link");

    assert_eq!(
        model.resolve_ref(&RefSite::new(alice, CfdPath::root().field("parent"))),
        Some(bob)
    );
    assert_eq!(
        model.resolve_ref(&RefSite::new(bob, CfdPath::root().field("parent"))),
        Some(alice)
    );
    assert_eq!(
        model.resolve_ref(&RefSite::new(self_record, CfdPath::root().field("parent"))),
        Some(self_record)
    );
}

#[test]
fn unresolved_record_ref_reports_reference_stage_diagnostic() {
    let schema = compile_schema(
        r#"
            table Item { name: string; }
            table Drop { item: Item; }
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
            abstract table Reward {}
            table CoinReward : Reward { amount: int; }
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
fn invalid_enum_is_rejected_while_non_finite_floats_are_values() {
    let schema = compile_schema(
        r#"
            enum Rarity { Common, Rare, }
            table Item {
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
}
