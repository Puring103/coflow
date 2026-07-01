#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

mod common;
use coflow_data_model::CfdRecord;
use common::*;

#[test]
fn record_keys_build_indexes_and_record_refs_resolve_by_expected_type() {
    let schema = compile_schema(
        r"
            abstract type Reward {}
            type ItemReward : Reward { count: int; }
            type Drop { reward: &Reward; }
        ",
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "reward_1",
        "ItemReward",
        [("count", CfdInputValue::from(3_i64))],
    );
    builder.add_record(
        "drop_1",
        "Drop",
        [("reward", CfdInputValue::record_ref("reward_1"))],
    );
    let model = builder.build().expect("record-key refs should build");
    let reward_id = record_id_at(&model, 0);
    let drop_id = record_id_at(&model, 1);

    assert_eq!(model.lookup("Reward", "reward_1"), Some(reward_id));
    assert_eq!(model.lookup("ItemReward", "reward_1"), Some(reward_id));
    assert_eq!(
        model.record(reward_id).map(CfdRecord::key),
        Some("reward_1")
    );
    assert_eq!(
        model
            .record(drop_id)
            .and_then(|record| record.field("reward")),
        Some(&CfdValue::Ref("reward_1".to_string()))
    );
    let _ = reward_id;
}

#[test]
fn parent_records_cannot_satisfy_child_typed_refs() {
    let schema = compile_schema(
        r"
            type Base { name: string; }
            type Child : Base { power: int; }
            type Holder { child: &Child; }
        ",
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("base_1", "Base", [("name", CfdInputValue::from("base"))]);
    builder.add_record(
        "holder_1",
        "Holder",
        [("child", CfdInputValue::record_ref("base_1"))],
    );

    let err = builder
        .build()
        .expect_err("parent-typed ref should not satisfy child field");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
}

#[test]
fn object_typed_fields_do_not_accept_bare_string_refs() {
    let schema = compile_schema(
        r"
            type Item { name: string; }
            type Holder { item: Item; }
        ",
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("item_1", "Item", [("name", CfdInputValue::from("Sword"))]);
    builder.add_record(
        "holder_1",
        "Holder",
        [("item", CfdInputValue::from("item_1"))],
    );

    let err = builder
        .build()
        .expect_err("bare string should not satisfy object field");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
}

#[test]
fn duplicate_keys_are_reported_for_concrete_and_polymorphic_ranges() {
    let schema = compile_schema(
        r"
            abstract type Reward {}
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
        ",
    );

    let mut concrete = CfdDataModel::builder(&schema);
    concrete.add_record(
        "same",
        "ItemReward",
        [("count", CfdInputValue::from(1_i64))],
    );
    concrete.add_record(
        "same",
        "ItemReward",
        [("count", CfdInputValue::from(2_i64))],
    );
    let err = concrete.build().expect_err("duplicate concrete key");
    assert_has_code(&err, CfdErrorCode::DuplicateId);

    let mut polymorphic = CfdDataModel::builder(&schema);
    polymorphic.add_record(
        "same",
        "ItemReward",
        [("count", CfdInputValue::from(1_i64))],
    );
    polymorphic.add_record(
        "same",
        "CurrencyReward",
        [("amount", CfdInputValue::from(2_i64))],
    );
    let err = polymorphic.build().expect_err("duplicate polymorphic key");
    assert_has_code(&err, CfdErrorCode::DuplicatePolymorphicId);
}

#[test]
fn unrelated_types_can_reuse_record_keys_in_separate_domains() {
    let schema = compile_schema(
        r"
            type Item { value: int; }
            type Skill { value: int; }
        ",
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("sword", "Item", [("value", CfdInputValue::from(1_i64))]);
    builder.add_record("sword", "Skill", [("value", CfdInputValue::from(2_i64))]);

    let model = builder
        .build()
        .expect("unrelated types should keep separate key domains");
    let item_id = record_id_at(&model, 0);
    let skill_id = record_id_at(&model, 1);
    let item_domain = model.type_domain_id("Item").expect("item domain");
    let skill_domain = model.type_domain_id("Skill").expect("skill domain");

    assert_ne!(item_domain, skill_domain);
    assert_eq!(model.record_by_type_key("Item", "sword"), Some(item_id));
    assert_eq!(model.record_by_type_key("Skill", "sword"), Some(skill_id));
    assert_eq!(
        model.record_by_domain_key(item_domain, "sword"),
        Some(item_id)
    );
    assert_eq!(
        model.record_by_domain_key(skill_domain, "sword"),
        Some(skill_id)
    );
}

#[test]
fn same_abstract_base_children_reject_duplicate_domain_keys() {
    let schema = compile_schema(
        r"
            abstract type Reward {}
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
        ",
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "same",
        "ItemReward",
        [("count", CfdInputValue::from(1_i64))],
    );
    builder.add_record(
        "same",
        "CurrencyReward",
        [("amount", CfdInputValue::from(2_i64))],
    );

    let err = builder
        .build()
        .expect_err("same inheritance domain should reject duplicate keys");
    assert_has_code(&err, CfdErrorCode::DuplicatePolymorphicId);
}

#[test]
fn plain_parent_and_child_reject_duplicate_domain_keys() {
    let schema = compile_schema(
        r"
            type Base { name: string; }
            type Child : Base { power: int; }
        ",
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("same", "Base", [("name", CfdInputValue::from("base"))]);
    builder.add_record(
        "same",
        "Child",
        [
            ("name", CfdInputValue::from("child")),
            ("power", CfdInputValue::from(3_i64)),
        ],
    );

    let err = builder
        .build()
        .expect_err("plain parent/child domain should reject duplicate keys");
    assert_has_code(&err, CfdErrorCode::DuplicatePolymorphicId);
}

#[test]
fn multi_level_inheritance_chain_rejects_duplicate_domain_keys() {
    let schema = compile_schema(
        r"
            type Entity { name: string; }
            type Reward : Entity { value: int; }
            type ItemReward : Reward { count: int; }
        ",
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("same", "Entity", [("name", CfdInputValue::from("entity"))]);
    builder.add_record(
        "same",
        "ItemReward",
        [
            ("name", CfdInputValue::from("item")),
            ("value", CfdInputValue::from(1_i64)),
            ("count", CfdInputValue::from(2_i64)),
        ],
    );

    let err = builder
        .build()
        .expect_err("multi-level inheritance domain should reject duplicate keys");
    assert_has_code(&err, CfdErrorCode::DuplicatePolymorphicId);
}

#[test]
fn domain_lookup_finds_any_member_record_but_type_lookup_uses_actual_type() {
    let schema = compile_schema(
        r"
            type Reward { name: string; }
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
        ",
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("base", "Reward", [("name", CfdInputValue::from("reward"))]);
    builder.add_record(
        "item",
        "ItemReward",
        [
            ("name", CfdInputValue::from("item")),
            ("count", CfdInputValue::from(1_i64)),
        ],
    );
    builder.add_record(
        "currency",
        "CurrencyReward",
        [
            ("name", CfdInputValue::from("currency")),
            ("amount", CfdInputValue::from(2_i64)),
        ],
    );

    let model = builder.build().expect("domain index should build");
    let base_id = record_id_at(&model, 0);
    let item_id = record_id_at(&model, 1);
    let currency_id = record_id_at(&model, 2);
    let domain = model.type_domain_id("Reward").expect("reward domain");
    let members = model.domain_members(domain).expect("domain members");

    assert_eq!(model.type_domain_id("ItemReward"), Some(domain));
    assert_eq!(model.type_domain_id("CurrencyReward"), Some(domain));
    assert!(members.contains(&model.type_id("Reward").expect("Reward type id")));
    assert!(members.contains(&model.type_id("ItemReward").expect("ItemReward type id")));
    assert!(members.contains(
        &model
            .type_id("CurrencyReward")
            .expect("CurrencyReward type id")
    ));
    assert_eq!(model.record_by_domain_key(domain, "base"), Some(base_id));
    assert_eq!(model.record_by_domain_key(domain, "item"), Some(item_id));
    assert_eq!(
        model.record_by_domain_key(domain, "currency"),
        Some(currency_id)
    );
    assert_eq!(model.record_by_type_key("Reward", "item"), None);
    assert_eq!(
        model.record_by_type_key("ItemReward", "item"),
        Some(item_id)
    );
    assert_eq!(model.record_by_type_key("CurrencyReward", "item"), None);
}

#[test]
fn lookup_for_middle_type_does_not_return_ancestor_records() {
    let schema = compile_schema(
        r"
            type Entity { name: string; }
            type Reward : Entity { value: int; }
            type ItemReward : Reward { count: int; }
        ",
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "entity",
        "Entity",
        [("name", CfdInputValue::from("entity"))],
    );
    builder.add_record(
        "item",
        "ItemReward",
        [
            ("name", CfdInputValue::from("item")),
            ("value", CfdInputValue::from(1_i64)),
            ("count", CfdInputValue::from(2_i64)),
        ],
    );

    let model = builder.build().expect("domain index should build");
    let item_id = record_id_at(&model, 1);

    assert_eq!(model.lookup("Reward", "entity"), None);
    assert_eq!(model.lookup("Reward", "item"), Some(item_id));
}

#[test]
fn domain_index_exposes_type_ancestors() {
    let schema = compile_schema(
        r"
            type Entity { name: string; }
            type Reward : Entity { value: int; }
            type ItemReward : Reward { count: int; }
        ",
    );

    let model = CfdDataModel::builder(&schema)
        .build()
        .expect("empty model should build");
    let entity = model.type_id("Entity").expect("Entity type id");
    let reward = model.type_id("Reward").expect("Reward type id");
    let item = model.type_id("ItemReward").expect("ItemReward type id");

    assert_eq!(model.type_ancestors(entity), Some(&[][..]));
    assert_eq!(model.type_ancestors(reward), Some(&[entity][..]));
    assert_eq!(model.type_ancestors(item), Some(&[reward, entity][..]));
}

#[test]
fn concrete_ref_expected_type_rejects_sibling_and_parent_records_in_same_domain() {
    let schema = compile_schema(
        r"
            type Reward { name: string; }
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
            type Holder { item: &ItemReward; }
        ",
    );

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
        [("item", CfdInputValue::record_ref("currency"))],
    );

    let err = sibling_builder
        .build()
        .expect_err("&Child should reject sibling records");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);

    let mut parent_builder = CfdDataModel::builder(&schema);
    parent_builder.add_record("base", "Reward", [("name", CfdInputValue::from("reward"))]);
    parent_builder.add_record(
        "holder",
        "Holder",
        [("item", CfdInputValue::record_ref("base"))],
    );

    let err = parent_builder
        .build()
        .expect_err("&Child should reject parent records");
    assert_has_code(&err, CfdErrorCode::TypeMismatch);
}
