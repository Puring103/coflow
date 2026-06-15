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
fn records_use_top_level_names_as_keys_and_do_not_emit_id_fields() -> TestResult {
    let schema = compile_schema(
        r"
            type Item {
                name: string;
            }
        ",
    );

    let records = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item = {
                name: "Iron Sword";
            };
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
fn explicit_record_and_path_refs_follow_latest_model() -> TestResult {
    let schema = compile_schema(
        r"
            type Item { name: string; }
            type Reward { item: Item; count: int; }
            type DropTable { rewards: [Reward]; }
            type Holder {
                item: Item;
                first_item: Item;
            }
        ",
    );

    let records = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item = { name: "Iron Sword"; };

            reward_holder: Holder = {
                item: @sword;
                first_item: @drop_1.rewards[0].item;
            };

            drop_1: DropTable = {
                rewards: [
                    { item: @sword; count: 2; }
                ];
            };
        "#,
    )?;

    assert_eq!(
        records[1].fields.get("item"),
        Some(&CfdInputValue::record_ref("sword"))
    );
    assert_eq!(
        records[1].fields.get("first_item"),
        Some(&CfdInputValue::path_ref(
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
            sword: Item = { name: "Iron Sword"; };
            reward_holder: Holder = {
                item: @sword;
                first_item: @drop_1.rewards[0].item;
            };
            drop_1: DropTable = {
                rewards: [{ item: @sword; count: 2; }];
            };
        "#,
    )?;

    let item_id = model.lookup("Item", "sword").expect("item record");
    let holder_id = model
        .lookup("Holder", "reward_holder")
        .expect("holder record");
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
fn cfd_rejects_reserved_id_fields() {
    let schema = compile_schema(
        r"
            type Item {
                name: string;
            }
        ",
    );

    let err = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item = {
                id: "sword";
                name: "Iron Sword";
            };
        "#,
    )
    .expect_err("id must be reserved");

    assert_has_text_code(&err, CfdTextErrorCode::ReservedIdField);
}

#[test]
fn cfd_rejects_bare_object_references_with_marker_hint() {
    let schema = compile_schema(
        r"
            type Item { name: string; }
            type Holder { item: Item; }
        ",
    );

    let err = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item = { name: "Iron Sword"; };
            holder: Holder = {
                item: sword;
            };
        "#,
    )
    .expect_err("object references must use @");

    assert_has_text_code(&err, CfdTextErrorCode::ReferenceNeedsMarker);
}

#[test]
fn cfd_allows_cyclic_record_references() -> TestResult {
    let schema = compile_schema(
        r"
            type Node {
                label: string;
                next: Node? = null;
            }
        ",
    );

    let model = load_cfd_model(
        &schema,
        r#"
            a: Node = { label: "A"; next: @b; };
            b: Node = { label: "B"; next: @a; };
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
fn cfd_accepts_equals_field_separators_and_keeps_at_strings_plain() -> TestResult {
    let schema = compile_schema(
        r"
            type Text {
                value: string;
            }
        ",
    );

    let records = parse_cfd_input_records(
        &schema,
        r"
            text_1: Text = {
                value = @not_a_ref;
            };
        ",
    )?;

    assert_eq!(
        records[0].fields.get("value"),
        Some(&CfdInputValue::from("@not_a_ref"))
    );
    Ok(())
}

#[test]
fn cfd_path_refs_parse_string_and_enum_indexes() -> TestResult {
    let schema = compile_schema(
        r"
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
        ",
    );

    let records = parse_cfd_input_records(
        &schema,
        r#"
            tables: Tables = {
                by_name: { "main": { name: "Main"; } };
                by_element: { Fire: { name: "Fire"; } };
            };
            holder: Holder = {
                named: @tables.by_name["main"];
                elemental: @tables.by_element[Element.Fire];
            };
        "#,
    )?;

    assert_eq!(
        records[1].fields.get("named"),
        Some(&CfdInputValue::path_ref(
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
        r"
            enum Element { Fire, Ice, }
            type Tables {
                resistances: {Element: float};
                labels: {string: string};
            }
            type Holder {
                fire_resistance: float;
                label: string;
            }
        ",
    );

    let source = r#"
        tables: Tables = {
            resistances: { Fire: 0.5; };
            labels: { "main": "primary"; };
        };
        holder: Holder = {
            fire_resistance: @tables.resistances[Fire];
            label: @tables.labels["main"];
        };
    "#;

    let records = parse_cfd_input_records(&schema, source)?;
    assert_eq!(
        records[1].fields.get("fire_resistance"),
        Some(&CfdInputValue::path_ref(
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
        r"
            type Item {
                name: string;
            }
        ",
    );

    let err = parse_cfd_input_records(
        &schema,
        r#"
            sword: Item = {
                name: "Iron Sword";
                check { true; }
            };
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
