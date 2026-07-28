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
    assert_has_code(&err, CfdErrorCode::TypeMismatch);

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

    assert_eq!(model.resolve_direct_ref(&item_site), Some(item_id));
    assert_eq!(model.resolve_direct_ref(&array_site), Some(item_id));

    let host_edges = model
        .direct_ref_edges_from_host(holder_id)
        .collect::<Vec<_>>();
    assert_eq!(host_edges.len(), 2);
    assert!(host_edges.iter().any(|edge| edge.site == item_site));
    assert!(host_edges.iter().any(|edge| edge.site == array_site));

    let target_edges = model
        .direct_ref_edges_to_target(item_id)
        .collect::<Vec<_>>();
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
                nested: Nested;
            }
            type Nested {
                item: &Item;
            }
            type Item { name: string; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("sword", "Item", [("name", LoadedValueDraft::from("Sword"))]);
    builder.add_record(
        "base",
        "Stats",
        [
            ("hp", LoadedValueDraft::from(10_i64)),
            ("item", LoadedValueDraft::record_ref("sword")),
            (
                "nested",
                LoadedValueDraft::object_with_declared_type([(
                    "item",
                    LoadedValueDraft::record_ref("sword"),
                )]),
            ),
        ],
    );
    builder.add_loaded_record(LoadedRecordDraft::with_spreads(
        "copy",
        "Stats",
        [LoadedValueDraft::record_ref("base")],
        [("hp", LoadedValueDraft::from(20_i64))],
    ));

    let model = builder.build().expect("spread should build");
    let item_id = model
        .lookup_assignable(&schema, "Item", "sword")
        .expect("item");
    let base_id = model
        .lookup_assignable(&schema, "Stats", "base")
        .expect("base");
    let copy_id = model
        .lookup_assignable(&schema, "Stats", "copy")
        .expect("copy");

    let inherited_item_site = RefSite::new(copy_id, CfdPath::root().field("item"));
    let inherited_nested_item_site =
        RefSite::new(copy_id, CfdPath::root().field("nested").field("item"));
    assert!(model.direct_ref_edges_to_target(base_id).next().is_none());
    assert!(model.resolve_direct_ref(&inherited_item_site).is_none());
    assert!(model
        .resolve_direct_ref(&inherited_nested_item_site)
        .is_none());

    let spread_edge = model
        .spread_edges_from_source(base_id)
        .find(|edge| edge.host == copy_id && edge.path == CfdPath::root())
        .expect("copy root spread edge");
    assert_eq!(spread_edge.source, base_id);
    assert!(spread_edge.fields.contains("item"));
    assert!(spread_edge.fields.contains("nested"));
    assert!(!spread_edge.fields.contains("hp"));
    assert_eq!(model.spread_edges_from_source(base_id).count(), 1);
    assert_eq!(
        model.spread_source_at_path(copy_id, &CfdPath::root().field("item")),
        Some(base_id)
    );
    assert_eq!(
        model.spread_source_at_path(copy_id, &CfdPath::root().field("nested").field("item")),
        Some(base_id)
    );
    assert_eq!(
        model.spread_source_at_path(copy_id, &CfdPath::root().field("hp")),
        None
    );

    assert_eq!(
        model.resolve_direct_ref(&RefSite::new(base_id, CfdPath::root().field("item"))),
        Some(item_id)
    );
    assert_eq!(
        model.resolve_direct_ref(&RefSite::new(
            base_id,
            CfdPath::root().field("nested").field("item")
        )),
        Some(item_id)
    );
}

#[test]
fn fully_overridden_spread_still_records_object_level_edge() {
    let schema = compile_schema(
        r#"
            type Item {
                name: string;
                power: int;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "base",
        "Item",
        [
            ("name", LoadedValueDraft::from("Base")),
            ("power", LoadedValueDraft::from(1_i64)),
        ],
    );
    builder.add_loaded_record(LoadedRecordDraft::with_spreads(
        "copy",
        "Item",
        [LoadedValueDraft::record_ref("base")],
        [
            ("name", LoadedValueDraft::from("Copy")),
            ("power", LoadedValueDraft::from(2_i64)),
        ],
    ));

    let model = builder
        .build()
        .expect("fully overridden spread should build");
    let base_id = model
        .lookup_assignable(&schema, "Item", "base")
        .expect("base");
    let copy_id = model
        .lookup_assignable(&schema, "Item", "copy")
        .expect("copy");
    let edge = model
        .spread_edges_from_source(base_id)
        .find(|edge| edge.host == copy_id && edge.path == CfdPath::root())
        .expect("copy root spread edge");

    assert_eq!(edge.source, base_id);
    assert!(edge.fields.is_empty());
    assert_eq!(model.spread_edges_from_source(base_id).count(), 1);
    assert_eq!(
        model.spread_source_at_path(copy_id, &CfdPath::root().field("name")),
        None
    );
}

#[test]
fn multiple_spreads_at_same_object_site_keep_all_edges() {
    let schema = compile_schema(
        r#"
            type Stats {
                hp: int;
                mp: int;
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "hp_base",
        "Stats",
        [
            ("hp", LoadedValueDraft::from(10_i64)),
            ("mp", LoadedValueDraft::from(0_i64)),
        ],
    );
    builder.add_record(
        "mp_base",
        "Stats",
        [
            ("hp", LoadedValueDraft::from(0_i64)),
            ("mp", LoadedValueDraft::from(20_i64)),
        ],
    );
    builder.add_loaded_record(LoadedRecordDraft::with_spreads(
        "copy",
        "Stats",
        [
            LoadedValueDraft::record_ref("hp_base"),
            LoadedValueDraft::record_ref("mp_base"),
        ],
        [("hp", LoadedValueDraft::from(30_i64))],
    ));

    let model = builder.build().expect("multi-spread should build");
    let copy_id = model
        .lookup_assignable(&schema, "Stats", "copy")
        .expect("copy");
    let hp_base_id = model
        .lookup_assignable(&schema, "Stats", "hp_base")
        .expect("hp base");
    let mp_base_id = model
        .lookup_assignable(&schema, "Stats", "mp_base")
        .expect("mp base");
    let edges = model
        .spread_edges()
        .filter(|edge| edge.host == copy_id && edge.path == CfdPath::root())
        .collect::<Vec<_>>();

    assert_eq!(edges.len(), 2);
    assert!(edges.iter().any(|edge| edge.source == hp_base_id));
    assert!(edges.iter().any(|edge| edge.source == mp_base_id));
    assert_eq!(
        model.spread_source_at_path(copy_id, &CfdPath::root().field("mp")),
        Some(mp_base_id)
    );
}
