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
fn reference_plan_includes_polymorphic_recursive_objects_and_preserves_edge_order() {
    let schema = compile_schema(
        r#"
            table Target {}
            abstract data Base {}
            abstract data Middle : Base {}
            data Link : Middle {
                z: &Target;
                a: &Target;
                next: Option<Base> = None;
            }
            data Plain { values: [int]; }
            data Wrapper { nodes: Option<{string: [Base]}>; }
            table Holder { value: Wrapper; plain: Plain; }
        "#,
    );
    let link = || {
        LoadedValueDraft::object(
            "Link",
            [
                ("z", LoadedValueDraft::record_ref("second")),
                ("a", LoadedValueDraft::record_ref("first")),
            ],
        )
    };
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "first",
        "Target",
        std::iter::empty::<(&str, LoadedValueDraft)>(),
    );
    builder.add_record(
        "second",
        "Target",
        std::iter::empty::<(&str, LoadedValueDraft)>(),
    );
    builder.add_record(
        "holder",
        "Holder",
        [
            (
                "value",
                LoadedValueDraft::object(
                    "Wrapper",
                    [(
                        "nodes",
                        LoadedValueDraft::OptionSome(Box::new(LoadedValueDraft::dict([
                            (
                                LoadedDictKeyDraft::from("zslot"),
                                LoadedValueDraft::Array(vec![link()]),
                            ),
                            (
                                LoadedDictKeyDraft::from("aslot"),
                                LoadedValueDraft::Array(vec![link()]),
                            ),
                        ]))),
                    )],
                ),
            ),
            (
                "plain",
                LoadedValueDraft::object(
                    "Plain",
                    [("values", LoadedValueDraft::Array(vec![1_i64.into()]))],
                ),
            ),
        ],
    );
    let model = builder.build().unwrap();
    let host = model.record_by_type_id("Holder", "holder").unwrap();
    let first = model.record_by_type_id("Target", "first").unwrap();
    let second = model.record_by_type_id("Target", "second").unwrap();
    let expected = ["zslot", "aslot"]
        .into_iter()
        .flat_map(|slot| {
            [("a", first), ("z", second)]
                .into_iter()
                .map(move |(field, target)| {
                    (
                        CfdPath::root()
                            .field("value")
                            .field("nodes")
                            .dict_key_value(&CfdDictKey::String(slot.to_string()))
                            .index(0)
                            .field(field),
                        target,
                    )
                })
        })
        .collect::<Vec<_>>();
    let actual = model
        .ref_edges_from_host(host)
        .map(|edge| (edge.site.path.clone(), edge.target))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    for (path, target) in expected {
        assert_eq!(model.resolve_ref(&RefSite::new(host, path)), Some(target));
    }
    assert_eq!(model.ref_edges_to_target(first).count(), 2);
    assert_eq!(model.ref_edges_to_target(second).count(), 2);
}

#[test]
fn repeated_validation_preserves_nested_editable_diagnostic_severity_and_order() {
    for invalid_enum in [false, true] {
        let schema = compile_schema(
            "enum Kind { Valid } data Child { value: Kind; } table Holder { child: Child; }",
        );
        let fields = if invalid_enum {
            vec![("value", LoadedValueDraft::enum_variant("Kind", "Missing"))]
        } else {
            Vec::new()
        };
        let mut builder = CfdDataModel::builder(&schema);
        builder.add_record(
            "holder",
            "Holder",
            [("child", LoadedValueDraft::object("Child", fields))],
        );
        let diagnostics = builder
            .build_partial()
            .expect_err("nested incomplete objects remain blocking");
        let codes = diagnostics
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>();
        assert_eq!(
            codes,
            [
                CfdErrorCode::MissingRequiredField,
                if invalid_enum {
                    CfdErrorCode::InvalidEnumVariant
                } else {
                    CfdErrorCode::MissingRequiredField
                },
            ]
        );
        for diagnostic in &diagnostics.diagnostics {
            assert_eq!(
                diagnostic.primary.as_ref().unwrap().path,
                CfdPath::root().field("child").field("value")
            );
        }
    }
}

#[test]
fn key_only_record_ref_helper_builds_ref_values_for_ref_fields() {
    let schema = compile_schema(
        r#"
            table Item { name: string; }
            table Holder { item: &Item; }
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
            table Reward { name: string; }
            table ItemReward : Reward { count: int; }
            table CurrencyReward : Reward { amount: int; }
            table Holder {
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
            data Item { name: string; }
            table Holder { item: Item; }
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
            table Item { name: string; }
            table Holder { item: &Item; }
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
fn partial_build_preserves_unresolved_refs() {
    let schema = compile_schema("table Item {} table Holder { item: &Item; }");
    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "holder",
        "Holder",
        [("item", LoadedValueDraft::record_ref("missing"))],
    );

    let output = builder
        .build_partial()
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
        "abstract table Reward {} table ItemReward : Reward {} table CurrencyReward : Reward {} table Holder { reward: &ItemReward; }",
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
        .build_partial()
        .expect("wrong-type refs remain editable");
    let diagnostic = diagnostic_with_code(&output.diagnostics, CfdErrorCode::RefTargetTypeMismatch);
    assert_eq!(diagnostic.stage, CfdStage::Reference);
    assert_eq!(diagnostic.code.as_str(), "REF-002");
}

#[test]
fn refs_populate_ref_edge_indexes() {
    let schema = compile_schema(
        r#"
            table Item { name: string; }
            table Holder {
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
