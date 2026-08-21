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
fn key_only_record_ref_helper_builds_ref_values_for_ref_fields() {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type Holder { item: &Item; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("sword", "Item", [("name", LoadedValueDraft::from("Sword"))]);
    builder.add_record(
        "holder",
        "Holder",
        [("item", LoadedValueDraft::record_ref("sword"))],
    );

    let model = builder.build().expect("key-only ref should build");
    let holder_id = model
        .lookup_assignable(&schema, "Holder", "holder")
        .expect("holder");
    let holder = model.record(holder_id).expect("holder record");
    assert_eq!(
        holder.field("item"),
        Some(&CfdValue::record_ref("sword").unwrap())
    );
}

#[test]
fn ref_fields_accept_child_records_and_reject_inline_objects_siblings_and_parents() {
    let schema = compile_schema(
        r#"
            type Reward { name: string; }
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
            type Holder {
                reward: &Reward;
                item_reward: &ItemReward;
            }
        "#,
    );

    let mut child_builder = CfdDataModel::builder(&schema);
    child_builder.add_record(
        "item",
        "ItemReward",
        [
            ("name", LoadedValueDraft::from("item")),
            ("count", LoadedValueDraft::from(1_i64)),
        ],
    );
    child_builder.add_record(
        "holder",
        "Holder",
        [
            ("reward", LoadedValueDraft::record_ref("item")),
            ("item_reward", LoadedValueDraft::record_ref("item")),
        ],
    );
    child_builder
        .build()
        .expect("&Reward and &ItemReward should accept an ItemReward record");

    let mut inline_builder = CfdDataModel::builder(&schema);
    inline_builder.add_record(
        "holder",
        "Holder",
        [
            (
                "reward",
                LoadedValueDraft::object_with_declared_type([(
                    "name",
                    LoadedValueDraft::from("inline"),
                )]),
            ),
            ("item_reward", LoadedValueDraft::record_ref("missing")),
        ],
    );
    let err = inline_builder
        .build()
        .expect_err("&Reward should reject inline objects");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);

    let mut sibling_builder = CfdDataModel::builder(&schema);
    sibling_builder.add_record(
        "currency",
        "CurrencyReward",
        [
            ("name", LoadedValueDraft::from("currency")),
            ("amount", LoadedValueDraft::from(2_i64)),
        ],
    );
    sibling_builder.add_record(
        "holder",
        "Holder",
        [
            ("reward", LoadedValueDraft::record_ref("currency")),
            ("item_reward", LoadedValueDraft::record_ref("currency")),
        ],
    );
    let err = sibling_builder
        .build()
        .expect_err("&ItemReward should reject sibling records");
    assert_has_code(&err, CfdErrorCode::RefTargetTypeMismatch);

    let mut parent_builder = CfdDataModel::builder(&schema);
    parent_builder.add_record("base", "Reward", [("name", LoadedValueDraft::from("base"))]);
    parent_builder.add_record(
        "holder",
        "Holder",
        [
            ("reward", LoadedValueDraft::record_ref("base")),
            ("item_reward", LoadedValueDraft::record_ref("base")),
        ],
    );
    let err = parent_builder
        .build()
        .expect_err("&ItemReward should reject parent records");
    assert_has_code(&err, CfdErrorCode::RefTargetTypeMismatch);
}

#[test]
fn inline_object_fields_reject_record_refs() {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type Holder { item: Item; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("sword", "Item", [("name", LoadedValueDraft::from("Sword"))]);
    builder.add_record(
        "holder",
        "Holder",
        [("item", LoadedValueDraft::record_ref("sword"))],
    );

    let err = builder
        .build()
        .expect_err("inline object fields should reject record refs");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
}

#[test]
fn missing_key_reports_ref_target_not_found() {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type Holder { item: &Item; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "holder",
        "Holder",
        [("item", LoadedValueDraft::record_ref("missing"))],
    );

    let err = builder.build().expect_err("missing ref should fail");
    assert_has_code(&err, CfdErrorCode::RefTargetNotFound);
}

#[test]
fn editable_build_preserves_unresolved_refs() {
    let schema = compile_schema("type Item {} type Holder { item: &Item; }");
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "holder",
        "Holder",
        [("item", LoadedValueDraft::record_ref("missing"))],
    );

    let output = builder
        .build_editable()
        .expect("unresolved refs are representable in an editable model");
    assert_has_code(&output.diagnostics, CfdErrorCode::RefTargetNotFound);
    let holder_id = output
        .model
        .lookup_assignable(&schema, "Holder", "holder")
        .expect("holder remains addressable");
    assert_eq!(
        output
            .model
            .record(holder_id)
            .and_then(|record| record.field("item")),
        Some(&CfdValue::record_ref("missing").unwrap())
    );
}

#[test]
fn wrong_ref_target_type_has_a_reference_diagnostic() {
    let schema = compile_schema(
        "abstract type Reward {} type ItemReward : Reward {} type CurrencyReward : Reward {} type Holder { reward: &ItemReward; }",
    );
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "reward",
        "CurrencyReward",
        std::iter::empty::<(&str, LoadedValueDraft)>(),
    );
    builder.add_record(
        "holder",
        "Holder",
        [("reward", LoadedValueDraft::record_ref("reward"))],
    );

    let output = builder
        .build_editable()
        .expect("wrong-type refs remain editable");
    let diagnostic = diagnostic_with_code(&output.diagnostics, CfdErrorCode::RefTargetTypeMismatch);
    assert_eq!(diagnostic.stage, CfdStage::Reference);
    assert_eq!(diagnostic.code.as_str(), "REF-002");
}

#[test]
fn refs_populate_ref_edge_indexes() {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type Holder {
                item: &Item;
                items: [&Item];
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("sword", "Item", [("name", LoadedValueDraft::from("Sword"))]);
    builder.add_record(
        "holder",
        "Holder",
        [
            ("item", LoadedValueDraft::record_ref("sword")),
            (
                "items",
                LoadedValueDraft::Array(vec![LoadedValueDraft::record_ref("sword")]),
            ),
        ],
    );

    let model = builder.build().expect("refs should build");
    let item_id = model
        .lookup_assignable(&schema, "Item", "sword")
        .expect("item");
    let holder_id = model
        .lookup_assignable(&schema, "Holder", "holder")
        .expect("holder");
    let item_site = RefSite::new(holder_id, CfdPath::root().field("item"));
    let array_site = RefSite::new(holder_id, CfdPath::root().field("items").index(0));

    assert_eq!(model.resolve_ref(&item_site), Some(item_id));
    assert_eq!(model.resolve_ref(&array_site), Some(item_id));

    let host_edges = model.ref_edges_from_host(holder_id).collect::<Vec<_>>();
    assert_eq!(host_edges.len(), 2);
    assert!(host_edges.iter().any(|edge| edge.site == item_site));
    assert!(host_edges.iter().any(|edge| edge.site == array_site));

    let target_edges = model.ref_edges_to_target(item_id).collect::<Vec<_>>();
    assert_eq!(target_edges.len(), 2);
    assert!(target_edges.iter().any(|edge| edge.site == item_site));
    assert!(target_edges.iter().any(|edge| edge.site == array_site));
}
