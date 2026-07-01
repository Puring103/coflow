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
fn key_only_record_ref_helper_builds_ref_values_for_ref_fields() {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type Holder { item: &Item; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("sword", "Item", [("name", CfdInputValue::from("Sword"))]);
    builder.add_record(
        "holder",
        "Holder",
        [("item", CfdInputValue::record_ref("sword"))],
    );

    let model = builder.build().expect("key-only ref should build");
    let holder_id = model.lookup("Holder", "holder").expect("holder");
    let holder = model.record(holder_id).expect("holder record");
    assert_eq!(
        holder.field("item"),
        Some(&CfdValue::Ref("sword".to_string()))
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
            ("name", CfdInputValue::from("item")),
            ("count", CfdInputValue::from(1_i64)),
        ],
    );
    child_builder.add_record(
        "holder",
        "Holder",
        [
            ("reward", CfdInputValue::record_ref("item")),
            ("item_reward", CfdInputValue::record_ref("item")),
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
                CfdInputValue::object_with_declared_type([("name", CfdInputValue::from("inline"))]),
            ),
            ("item_reward", CfdInputValue::record_ref("missing")),
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
            ("name", CfdInputValue::from("currency")),
            ("amount", CfdInputValue::from(2_i64)),
        ],
    );
    sibling_builder.add_record(
        "holder",
        "Holder",
        [
            ("reward", CfdInputValue::record_ref("currency")),
            ("item_reward", CfdInputValue::record_ref("currency")),
        ],
    );
    let err = sibling_builder
        .build()
        .expect_err("&ItemReward should reject sibling records");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);

    let mut parent_builder = CfdDataModel::builder(&schema);
    parent_builder.add_record("base", "Reward", [("name", CfdInputValue::from("base"))]);
    parent_builder.add_record(
        "holder",
        "Holder",
        [
            ("reward", CfdInputValue::record_ref("base")),
            ("item_reward", CfdInputValue::record_ref("base")),
        ],
    );
    let err = parent_builder
        .build()
        .expect_err("&ItemReward should reject parent records");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
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
    builder.add_record("sword", "Item", [("name", CfdInputValue::from("Sword"))]);
    builder.add_record(
        "holder",
        "Holder",
        [("item", CfdInputValue::record_ref("sword"))],
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
        [("item", CfdInputValue::record_ref("missing"))],
    );

    let err = builder.build().expect_err("missing ref should fail");
    assert_has_code(&err, CfdErrorCode::RefTargetNotFound);
}

#[test]
fn direct_refs_populate_ref_edge_indexes() {
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
    builder.add_record("sword", "Item", [("name", CfdInputValue::from("Sword"))]);
    builder.add_record(
        "holder",
        "Holder",
        [
            ("item", CfdInputValue::record_ref("sword")),
            (
                "items",
                CfdInputValue::Array(vec![CfdInputValue::record_ref("sword")]),
            ),
        ],
    );

    let model = builder.build().expect("refs should build");
    let item_id = model.lookup("Item", "sword").expect("item");
    let holder_id = model.lookup("Holder", "holder").expect("holder");
    let item_site = RefSite::new(holder_id, CfdPath::root().field("item"));
    let array_site = RefSite::new(holder_id, CfdPath::root().field("items").index(0));

    let item_edge_id = model.ref_edge_at(&item_site).expect("item edge");
    let array_edge_id = model.ref_edge_at(&array_site).expect("array edge");
    assert_eq!(model.resolve_ref(&item_site), Some(item_id));
    assert_eq!(model.resolve_ref(&array_site), Some(item_id));
    assert_eq!(
        model.ref_edge(item_edge_id).map(|edge| edge.target),
        Some(item_id)
    );
    assert_eq!(
        model.ref_edge(array_edge_id).map(|edge| edge.target),
        Some(item_id)
    );

    let host_edges = model.ref_edges_from_host(holder_id).collect::<Vec<_>>();
    assert_eq!(host_edges.len(), 2);
    assert!(host_edges.iter().any(|edge| edge.id == item_edge_id));
    assert!(host_edges.iter().any(|edge| edge.id == array_edge_id));

    let target_edges = model.ref_edges_to_target(item_id).collect::<Vec<_>>();
    assert_eq!(target_edges.len(), 2);
    assert!(target_edges.iter().any(|edge| edge.site == item_site));
    assert!(target_edges.iter().any(|edge| edge.site == array_site));
}

#[test]
fn object_spread_source_is_not_a_direct_ref_edge() {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int;
                item: &Item;
            }
            type Item { name: string; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("sword", "Item", [("name", CfdInputValue::from("Sword"))]);
    builder.add_record(
        "base",
        "Stats",
        [
            ("hp", CfdInputValue::from(10_i64)),
            ("item", CfdInputValue::record_ref("sword")),
        ],
    );
    builder.add_input_record(CfdInputRecord::with_spreads(
        "copy",
        "Stats",
        [CfdInputValue::record_ref("base")],
        [("hp", CfdInputValue::from(20_i64))],
    ));

    let model = builder.build().expect("spread should build");
    let item_id = model.lookup("Item", "sword").expect("item");
    let base_id = model.lookup("Stats", "base").expect("base");
    let copy_id = model.lookup("Stats", "copy").expect("copy");

    assert!(model.ref_edges_to_target(base_id).next().is_none());
    assert_eq!(
        model.resolve_ref_at(copy_id, &CfdPath::root().field("item")),
        Some(item_id)
    );
}
