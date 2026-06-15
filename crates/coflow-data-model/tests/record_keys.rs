#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

mod common;
use common::*;

#[test]
fn record_keys_build_indexes_and_record_refs_resolve_by_expected_type() {
    let schema = compile_schema(
        r#"
            abstract type Reward {}
            type ItemReward : Reward { count: int; }
            type Drop { reward: Reward; }
        "#,
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
        model.record(reward_id).map(|record| record.key()),
        Some("reward_1")
    );
    assert_eq!(
        model
            .record(drop_id)
            .and_then(|record| record.field("reward")),
        Some(&CfdValue::Ref {
            key: "reward_1".to_string(),
            target: reward_id,
        })
    );
}

#[test]
fn parent_records_cannot_satisfy_child_typed_refs() {
    let schema = compile_schema(
        r#"
            type Base { name: string; }
            type Child : Base { power: int; }
            type Holder { child: Child; }
        "#,
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
        .expect_err("parent key should not satisfy child field");
    assert_has_code(&err, CfdErrorCode::RefTargetNotFound);
}

#[test]
fn object_typed_fields_do_not_accept_bare_string_refs() {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type Holder { item: Item; }
        "#,
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
fn path_refs_resolve_fields_arrays_and_dict_keys() {
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
fn path_refs_can_follow_record_refs_before_field_access() {
    let schema = compile_schema(
        r#"
            type Skill { power: int; }
            type Loadout { primary: Skill; }
            type Holder { copied_power: int; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record("skill_1", "Skill", [("power", CfdInputValue::from(9_i64))]);
    builder.add_record(
        "loadout_1",
        "Loadout",
        [("primary", CfdInputValue::record_ref("skill_1"))],
    );
    builder.add_record(
        "holder_1",
        "Holder",
        [(
            "copied_power",
            CfdInputValue::path_ref(
                "loadout_1",
                [
                    CfdRefPathSegment::Field("primary".to_string()),
                    CfdRefPathSegment::Field("power".to_string()),
                ],
            ),
        )],
    );

    let model = builder
        .build()
        .expect("path refs should follow record refs");
    let holder_id = record_id_at(&model, 2);
    let holder = model.record(holder_id).expect("holder record");
    assert_eq!(holder.field("copied_power"), Some(&CfdValue::Int(9)));
}

#[test]
fn duplicate_keys_are_reported_for_concrete_and_polymorphic_ranges() {
    let schema = compile_schema(
        r#"
            abstract type Reward {}
            type ItemReward : Reward { count: int; }
            type CurrencyReward : Reward { amount: int; }
        "#,
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
