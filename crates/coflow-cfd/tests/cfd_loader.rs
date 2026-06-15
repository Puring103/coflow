#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

use coflow_cfd::{load_cfd_model, parse_cfd_input_records, CfdTextErrorCode, CfdTextLoadError};
use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::{CfdInputRefIndex, CfdInputValue, CfdRefPathSegment, CfdValue};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn compile_schema(source: &str) -> CftContainer {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("main"), source)
        .expect("schema should parse");
    container.compile().expect("schema should compile");
    container
}

#[test]
fn records_use_colon_blocks_and_do_not_emit_id_fields() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item {
                name: string;
            }
        "#,
    );

    let records = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item {
                name: "Iron Sword",
            }
        "#,
    )?;

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].key, "sword");
    assert_eq!(records[0].actual_type, "Item");
    assert_eq!(
        records[0].fields.get("name"),
        Some(&CfdInputValue::from("Iron Sword"))
    );
    assert!(!records[0].fields.contains_key("id"));
    Ok(())
}

#[test]
fn typed_refs_and_direct_ref_shorthand_follow_latest_model() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type Reward { item: Item; count: int; }
            type DropTable { rewards: [Reward]; }
            type Holder {
                item: Item;
                first_item: Item;
            }
        "#,
    );

    let records = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item { name: "Iron Sword" }

            drop_1: DropTable {
                rewards: [
                    { item: &sword, count: 2 },
                ],
            }

            holder: Holder {
                item: &sword,
                first_item: @DropTable.drop_1.rewards[0].item,
            }
        "#,
    )?;

    assert_eq!(
        records[2].fields.get("item"),
        Some(&CfdInputValue::record_ref("Item", "sword"))
    );
    assert_eq!(
        records[2].fields.get("first_item"),
        Some(&CfdInputValue::path_ref(
            "DropTable",
            "drop_1",
            [
                CfdRefPathSegment::Field("rewards".to_string()),
                CfdRefPathSegment::Index(CfdInputRefIndex::Int(0)),
                CfdRefPathSegment::Field("item".to_string()),
            ],
        ))
    );

    let model = load_cfd_model(
        &schema,
        r#"
            sword: Item { name: "Iron Sword" }
            drop_1: DropTable {
                rewards: [{ item: &sword, count: 2 }],
            }
            holder: Holder {
                item: &sword,
                first_item: @DropTable.drop_1.rewards[0].item,
            }
        "#,
    )?;

    let item_id = model.lookup("Item", "sword").expect("item record");
    let holder_id = model.lookup("Holder", "holder").expect("holder record");
    let holder = model.record(holder_id).expect("holder");
    assert_eq!(
        holder.field("item"),
        Some(&CfdValue::Ref {
            key: "sword".to_string(),
            target: item_id,
        })
    );
    assert_eq!(
        holder.field("first_item"),
        Some(&CfdValue::Ref {
            key: "sword".to_string(),
            target: item_id,
        })
    );
    Ok(())
}

#[test]
fn cfd_rejects_legacy_and_bare_object_references() {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            type Holder { item: Item; }
        "#,
    );

    let legacy_at = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item { name: "Iron Sword" }
            holder: Holder { item: @sword }
        "#,
    )
    .expect_err("@key is no longer valid");
    assert_has_text_code(&legacy_at, CfdTextErrorCode::Syntax);

    let direct_path = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item { name: "Iron Sword" }
            holder: Holder { item: &sword.name }
        "#,
    )
    .expect_err("&key must not support paths");
    assert_has_text_code(&direct_path, CfdTextErrorCode::Syntax);

    let bare = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item { name: "Iron Sword" }
            holder: Holder { item: sword }
        "#,
    )
    .expect_err("object references must use markers");
    assert_has_text_code(&bare, CfdTextErrorCode::ReferenceNeedsMarker);
}

#[test]
fn grouped_records_expand_to_records_of_the_same_type() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
        "#,
    );

    let records = parse_cfd_input_records(
        &schema,
        r#"
            Item {
                sword { name: "Sword" }
                shield { name: "Shield" }
            }
        "#,
    )?;

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].key, "sword");
    assert_eq!(records[0].actual_type, "Item");
    assert_eq!(records[1].key, "shield");
    assert_eq!(records[1].actual_type, "Item");
    Ok(())
}

#[test]
fn grouped_polymorphic_records_can_choose_concrete_types() -> TestResult {
    let schema = compile_schema(
        r#"
            type Item { name: string; }
            abstract type Reward {}
            type CurrencyReward : Reward { amount: int; }
            type ItemReward : Reward { item: Item; count: int; }
        "#,
    );

    let model = load_cfd_model(
        &schema,
        r#"
            sword: Item { name: "Sword" }

            Reward {
                coin: CurrencyReward { amount: 100 }
                item: ItemReward { item: &sword, count: 1 }
            }
        "#,
    )?;

    let coin_id = model
        .lookup("CurrencyReward", "coin")
        .expect("currency reward");
    let item_id = model.lookup("ItemReward", "item").expect("item reward");
    assert_eq!(model.lookup("Reward", "coin"), Some(coin_id));
    assert_eq!(model.lookup("Reward", "item"), Some(item_id));
    Ok(())
}

#[test]
fn cfd_object_and_dict_spreads_merge_before_local_overrides() -> TestResult {
    let schema = compile_schema(
        r#"
            enum Element { Fire, Ice, }
            type Stats { hp: int; attack: int; }
            type Monster {
                name: string;
                stats: Stats;
                weights: {Element: int};
            }
        "#,
    );

    let model = load_cfd_model(
        &schema,
        r#"
            base: Monster {
                name: "Base",
                stats: { hp: 100, attack: 20 },
                weights: { Fire: 10, Ice: 5 },
            }

            elite: Monster {
                ...@Monster.base,
                name: "Elite",
                stats: { ...@Monster.base.stats, hp: 180 },
                weights: { ...@Monster.base.weights, Fire: 20 },
            }
        "#,
    )?;

    let elite_id = model.lookup("Monster", "elite").expect("elite record");
    let elite = model.record(elite_id).expect("elite");
    assert_eq!(
        elite.field("name"),
        Some(&CfdValue::String("Elite".to_string()))
    );
    let Some(CfdValue::Object(stats)) = elite.field("stats") else {
        panic!("expected stats object");
    };
    assert_eq!(stats.field("hp"), Some(&CfdValue::Int(180)));
    assert_eq!(stats.field("attack"), Some(&CfdValue::Int(20)));
    let Some(CfdValue::Dict(weights)) = elite.field("weights") else {
        panic!("expected weights dict");
    };
    assert_eq!(weights.len(), 2);
    assert!(weights.iter().any(|(_, value)| value == &CfdValue::Int(20)));
    assert!(weights.iter().any(|(_, value)| value == &CfdValue::Int(5)));
    Ok(())
}

#[test]
fn cfd_rejects_reserved_id_fields() {
    let schema = compile_schema(
        r#"
            type Item {
                name: string;
            }
        "#,
    );

    let err = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item {
                id: "sword",
                name: "Iron Sword",
            }
        "#,
    )
    .expect_err("id must be reserved");

    assert_has_text_code(&err, CfdTextErrorCode::ReservedIdField);
}

#[test]
fn cfd_allows_cyclic_record_references() -> TestResult {
    let schema = compile_schema(
        r#"
            type Node {
                label: string;
                next: Node? = null;
            }
        "#,
    );

    let model = load_cfd_model(
        &schema,
        r#"
            a: Node { label: "A", next: &b }
            b: Node { label: "B", next: &a }
        "#,
    )?;

    let a_id = model.lookup("Node", "a").expect("a record");
    let b_id = model.lookup("Node", "b").expect("b record");
    assert_eq!(
        model.record(a_id).and_then(|record| record.field("next")),
        Some(&CfdValue::Ref {
            key: "b".to_string(),
            target: b_id,
        })
    );
    assert_eq!(
        model.record(b_id).and_then(|record| record.field("next")),
        Some(&CfdValue::Ref {
            key: "a".to_string(),
            target: a_id,
        })
    );
    Ok(())
}

#[test]
fn cfd_path_refs_parse_string_and_enum_indexes() -> TestResult {
    let schema = compile_schema(
        r#"
            enum Element { Fire, Ice, }
            type Item { name: string; }
            type Tables {
                by_name: {string: Item};
                by_element: {Element: Item};
            }
            type Holder {
                named: Item;
                elemental: Item;
            }
        "#,
    );

    let records = parse_cfd_input_records(
        &schema,
        r#"
            tables: Tables {
                by_name: { "main": { name: "Main" } },
                by_element: { Fire: { name: "Fire" } },
            }
            holder: Holder {
                named: @Tables.tables.by_name["main"],
                elemental: @Tables.tables.by_element[Element.Fire],
            }
        "#,
    )?;

    assert_eq!(
        records[1].fields.get("named"),
        Some(&CfdInputValue::path_ref(
            "Tables",
            "tables",
            [
                CfdRefPathSegment::Field("by_name".to_string()),
                CfdRefPathSegment::Index(CfdInputRefIndex::String("main".to_string())),
            ],
        ))
    );
    assert_eq!(
        records[1].fields.get("elemental"),
        Some(&CfdInputValue::path_ref(
            "Tables",
            "tables",
            [
                CfdRefPathSegment::Field("by_element".to_string()),
                CfdRefPathSegment::Index(CfdInputRefIndex::EnumVariant {
                    enum_name: "Element".to_string(),
                    variant: "Fire".to_string(),
                }),
            ],
        ))
    );
    Ok(())
}

#[test]
fn cfd_path_refs_can_target_scalar_fields() -> TestResult {
    let schema = compile_schema(
        r#"
            enum Element { Fire, Ice, }
            type Tables {
                resistances: {Element: float};
                labels: {string: string};
            }
            type Holder {
                fire_resistance: float;
                label: string;
            }
        "#,
    );

    let source = r#"
        tables: Tables {
            resistances: { Fire: 0.5 },
            labels: { "main": "primary" },
        }
        holder: Holder {
            fire_resistance: @Tables.tables.resistances[Fire],
            label: @Tables.tables.labels["main"],
        }
    "#;

    let records = parse_cfd_input_records(&schema, source)?;
    assert_eq!(
        records[1].fields.get("fire_resistance"),
        Some(&CfdInputValue::path_ref(
            "Tables",
            "tables",
            [
                CfdRefPathSegment::Field("resistances".to_string()),
                CfdRefPathSegment::Index(CfdInputRefIndex::Variant("Fire".to_string())),
            ],
        ))
    );
    assert_eq!(
        records[1].fields.get("label"),
        Some(&CfdInputValue::path_ref(
            "Tables",
            "tables",
            [
                CfdRefPathSegment::Field("labels".to_string()),
                CfdRefPathSegment::Index(CfdInputRefIndex::String("main".to_string())),
            ],
        ))
    );

    let model = load_cfd_model(&schema, source)?;
    let holder_id = model.lookup("Holder", "holder").expect("holder record");
    let holder = model.record(holder_id).expect("holder");
    assert_eq!(holder.field("fire_resistance"), Some(&CfdValue::Float(0.5)));
    assert_eq!(
        holder.field("label"),
        Some(&CfdValue::String("primary".to_string()))
    );
    Ok(())
}

#[test]
fn cfd_rejects_check_blocks_as_data_syntax() {
    let schema = compile_schema(
        r#"
            type Item {
                name: string;
            }
        "#,
    );

    let err = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item {
                name: "Iron Sword",
                check { true }
            }
        "#,
    )
    .expect_err("check blocks are not CFD data syntax");

    assert_has_text_code(&err, CfdTextErrorCode::Syntax);
}

fn assert_has_text_code(err: &CfdTextLoadError, code: CfdTextErrorCode) {
    let CfdTextLoadError::Text(diagnostics) = err else {
        panic!("expected text diagnostics, got {err:?}");
    };
    assert!(
        diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == code),
        "expected {code:?}, got {:?}",
        diagnostics
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>()
    );
}
