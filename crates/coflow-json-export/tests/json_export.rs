use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::{CfdDataModel, CfdInputDictKey, CfdInputValue};
use coflow_json_export::export_json_model;
use serde_json::json;

fn compile_schema(source: &str) -> CftContainer {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("main"), source)
        .expect("schema should parse");
    container.compile().expect("schema should compile");
    container
}

#[test]
fn exports_tables_with_schema_order_defaults_and_scalar_values() {
    let schema = compile_schema(
        r#"
            enum Rarity { Common = 0, Rare = 10, }
            type Item {
                @id
                id: string;
                name: string = "unknown";
                rarity: Rarity = Rarity.Common;
                tags: [string] = [];
                attrs: {string: int} = {};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("iron_sword")),
            ("rarity", CfdInputValue::enum_variant("Rarity", "Rare")),
            (
                "tags",
                CfdInputValue::Array(vec![
                    CfdInputValue::from("weapon"),
                    CfdInputValue::from("melee"),
                ]),
            ),
            (
                "attrs",
                CfdInputValue::dict([
                    (CfdInputDictKey::from("attack"), CfdInputValue::from(12_i64)),
                    (CfdInputDictKey::from("level"), CfdInputValue::from(3_i64)),
                ]),
            ),
        ],
    );
    let model = builder.build().expect("data model should build");
    let tables = export_json_model(&schema, &model).expect("export json");

    assert_eq!(
        tables["Item"],
        json!([
            {
                "id": "iron_sword",
                "name": "unknown",
                "rarity": 10,
                "tags": ["weapon", "melee"],
                "attrs": {
                    "attack": 12,
                    "level": 3
                }
            }
        ])
    );
}

#[test]
fn exports_refs_as_ids_and_polymorphic_objects_with_type_tags() {
    let schema = compile_schema(
        r#"
            type Item { @id id: string; name: string; }
            abstract type Reward { id: string; }
            type ItemReward : Reward {
                @ref(Item)
                item_id: string;
                count: int = 1;
            }
            type CurrencyReward : Reward {
                amount: int;
            }
            type DropTable {
                @id
                id: string;
                rewards: [Reward];
                weights: [int];
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Item",
        [
            ("id", CfdInputValue::from("iron_sword")),
            ("name", CfdInputValue::from("Iron Sword")),
        ],
    );
    builder.add_record(
        "DropTable",
        [
            ("id", CfdInputValue::from("drop_1")),
            (
                "rewards",
                CfdInputValue::Array(vec![
                    CfdInputValue::object(
                        "ItemReward",
                        [
                            ("id", CfdInputValue::from("reward_sword")),
                            ("item_id", CfdInputValue::from("iron_sword")),
                            ("count", CfdInputValue::from(2_i64)),
                        ],
                    ),
                    CfdInputValue::object(
                        "CurrencyReward",
                        [
                            ("id", CfdInputValue::from("reward_gold")),
                            ("amount", CfdInputValue::from(50_i64)),
                        ],
                    ),
                ]),
            ),
            (
                "weights",
                CfdInputValue::Array(vec![
                    CfdInputValue::from(70_i64),
                    CfdInputValue::from(30_i64),
                ]),
            ),
        ],
    );
    let model = builder.build().expect("data model should build");
    let tables = export_json_model(&schema, &model).expect("export json");

    assert_eq!(
        tables["DropTable"],
        json!([
            {
                "id": "drop_1",
                "rewards": [
                    {
                        "$type": "ItemReward",
                        "id": "reward_sword",
                        "item_id": "iron_sword",
                        "count": 2
                    },
                    {
                        "$type": "CurrencyReward",
                        "id": "reward_gold",
                        "amount": 50
                    }
                ],
                "weights": [70, 30]
            }
        ])
    );
}

#[test]
fn exports_type_tag_for_concrete_parent_ranges_even_when_actual_is_parent() {
    let schema = compile_schema(
        r#"
            type Reward { id: string; }
            type ItemReward : Reward { count: int; }
            type Holder { reward: Reward; }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Holder",
        [(
            "reward",
            CfdInputValue::object("Reward", [("id", CfdInputValue::from("base_reward"))]),
        )],
    );
    let model = builder.build().expect("data model should build");
    let tables = export_json_model(&schema, &model).expect("export json");

    assert_eq!(
        tables["Holder"],
        json!([
            {
                "reward": {
                    "$type": "Reward",
                    "id": "base_reward"
                }
            }
        ])
    );
}

#[test]
fn exports_dict_keys_as_json_object_keys() {
    let schema = compile_schema(
        r#"
            enum DamageType { Physical = 0, Fire = 1, Ice = 2, }
            type Resistances {
                by_enum: {DamageType: float};
                by_int: {int: string};
            }
        "#,
    );

    let mut builder = CfdDataModel::builder(&schema);
    builder.add_record(
        "Resistances",
        [
            (
                "by_enum",
                CfdInputValue::dict([
                    (
                        CfdInputDictKey::enum_variant("DamageType", "Fire"),
                        CfdInputValue::from(0.5_f64),
                    ),
                    (
                        CfdInputDictKey::enum_variant("DamageType", "Ice"),
                        CfdInputValue::from(0.2_f64),
                    ),
                ]),
            ),
            (
                "by_int",
                CfdInputValue::dict([
                    (CfdInputDictKey::from(1_i64), CfdInputValue::from("one")),
                    (CfdInputDictKey::from(2_i64), CfdInputValue::from("two")),
                ]),
            ),
        ],
    );
    let model = builder.build().expect("data model should build");
    let tables = export_json_model(&schema, &model).expect("export json");

    assert_eq!(
        tables["Resistances"],
        json!([
            {
                "by_enum": {
                    "1": 0.5,
                    "2": 0.2
                },
                "by_int": {
                    "1": "one",
                    "2": "two"
                }
            }
        ])
    );
}
