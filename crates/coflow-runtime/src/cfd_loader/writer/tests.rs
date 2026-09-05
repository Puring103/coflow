//! Round-trip tests for `CfdWriter`: write a value, re-parse the file from
//! disk, assert the new value is reflected and that other records / fields
//! are unchanged.
#![allow(
    clippy::expect_used,
    clippy::needless_borrow,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::redundant_field_names,
    clippy::unwrap_used
)]

use super::CfdWriter;
use crate::api::{
    CfdSource, CfdSourcePath, DeleteRecordRequest, InsertRecordRequest, ReorderRecordsOperation,
    ReorderRecordsRequest, WriteCellRequest, WriteFieldPathSegment, WriteRecordRef,
};
use crate::{load_cfd_model, parse_cfd_input_records, CfdObject, CfdValue};
use crate::{RecordOrigin, TextSpan};
use coflow_format::format_cfd;
use coflow_language::cft::{
    build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, ModuleId,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

fn temp_dir(name: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("coflow-language-writer-{name}-{id}"));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("remove temp dir");
    }
    fs::create_dir_all(&dir).expect("mkdir temp");
    dir
}

fn compile_schema(source: &str) -> CftSchema {
    let modules = parse_modules([CftFile::from_source(ModuleId::from("main"), source)]);
    build_schema(&modules, &CftDimensionInputs::default()).expect("schema compile")
}

fn empty_source(path: &Path) -> CfdSource {
    CfdSource {
        location: CfdSourcePath::new(path.to_path_buf()),
        display_name: path.display().to_string(),
    }
}

fn origin_for(path: &Path) -> RecordOrigin {
    RecordOrigin::File {
        path: path.to_path_buf(),
        span: Some(TextSpan {
            start_line: 0,
            start_character: 0,
            end_line: 0,
            end_character: 0,
        }),
    }
}

#[test]
fn reorders_records_without_changing_document_slots() {
    let dir = temp_dir("reorder");
    let file = dir.join("items.cfd");
    fs::write(
        &file,
        "a: Item { value: 1 }\n\nb: Item { value: 2 }\n\nc: Item { value: 3 }\n",
    )
    .expect("write seed");
    let source = empty_source(&file);
    let origin = origin_for(&file);
    let writer = CfdWriter::new();
    writer
        .reorder_records(&ReorderRecordsRequest {
            source: &source,
            operation: ReorderRecordsOperation::Swap {
                first: WriteRecordRef {
                    origin: &origin,
                    record_key: "a",
                    actual_type: "Item",
                },
                second: WriteRecordRef {
                    origin: &origin,
                    record_key: "c",
                    actual_type: "Item",
                },
            },
    })
        .expect("swap records");
    writer.publish().expect("publish staged swap");
    let swapped = fs::read_to_string(&file).expect("read swapped");
    assert!(swapped.find("c: Item").unwrap() < swapped.find("b: Item").unwrap());
    assert!(swapped.find("b: Item").unwrap() < swapped.find("a: Item").unwrap());

    writer
        .reorder_records(&ReorderRecordsRequest {
            source: &source,
            operation: ReorderRecordsOperation::MoveBefore {
                record: WriteRecordRef {
                    origin: &origin,
                    record_key: "c",
                    actual_type: "Item",
                },
                before: None,
            },
        })
        .expect("move record");
    writer.publish().expect("publish staged move");
    let moved = fs::read_to_string(&file).expect("read moved");
    assert!(moved.find("b: Item").unwrap() < moved.find("a: Item").unwrap());
    assert!(moved.find("a: Item").unwrap() < moved.find("c: Item").unwrap());
}

#[test]
fn reorders_grouped_and_top_level_records_without_losing_record_types() {
    let dir = temp_dir("reorder-group-boundary");
    let file = dir.join("items.cfd");
    fs::write(
        &file,
        r"Item {
  grouped {
    value: 1,
  }
}

top: Item {
  value: 2,
}
",
    )
    .expect("write seed");
    let schema = compile_schema("type Item { value: int; }");
    let source = empty_source(&file);
    let origin = origin_for(&file);
    let writer = CfdWriter::new();

    writer
        .reorder_records(&ReorderRecordsRequest {
            source: &source,
            operation: ReorderRecordsOperation::Swap {
                first: WriteRecordRef {
                    origin: &origin,
                    record_key: "grouped",
                    actual_type: "Item",
                },
                second: WriteRecordRef {
                    origin: &origin,
                    record_key: "top",
                    actual_type: "Item",
                },
            },
        })
        .expect("swap across group boundary");
    writer.publish().expect("publish staged swap");

    let after = fs::read_to_string(&file).expect("read reordered source");
    let model = load_cfd_model(&schema, &after).expect("reload reordered source");
    assert!(model
        .lookup_assignable(&schema, "Item", "grouped")
        .is_some());
    assert!(model.lookup_assignable(&schema, "Item", "top").is_some());
    assert!(after.contains("grouped: Item"), "{after}");
}

#[test]
fn writes_scalar_field_and_preserves_siblings() {
    let dir = temp_dir("scalar");
    let file = dir.join("items.cfd");
    fs::write(
        &file,
        r#"sword: Item {
  name: "Old",
  value: 10,
}

shield: Item {
  name: "Round",
  value: 5,
}
"#,
    )
    .expect("write seed");

    let schema = compile_schema(
        r"
        type Item {
          name: string;
          value: int;
        }
        ",
    );

    let schema = &schema;
    let writer = CfdWriter::new();
    let request_value = CfdValue::Int(42);
    let segments = vec![WriteFieldPathSegment::Field("value".to_string())];
    let origin = origin_for(&file);
    let request = WriteCellRequest {
        origin: &origin,
        record_key: "sword",
        actual_type: "Item",
        field_path: &segments,
        new_value: &request_value,
        schema: schema,
    };
    writer.write_field(&request).expect("write succeeds");
    writer.publish().expect("publish staged field");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(after.contains("value: 42"), "expected 42 in: {after}");
    // The other record's value must be untouched.
    assert!(
        after.contains("value: 5"),
        "shield.value should remain 5: {after}"
    );
    // And the unchanged name lines too.
    assert!(after.contains("\"Old\""), "sword.name unchanged: {after}");
    assert!(
        after.contains("\"Round\""),
        "shield.name unchanged: {after}"
    );
}

#[test]
fn inserts_missing_field_with_two_space_indentation() {
    let dir = temp_dir("insert-missing-field-indentation");
    let file = dir.join("items.cfd");
    fs::write(&file, "sword: Item {\n}\n").expect("write seed");
    let schema = compile_schema("type Item { value: int; }");
    let origin = origin_for(&file);
    let value = CfdValue::Int(42);
    let segments = vec![WriteFieldPathSegment::Field("value".to_string())];

    let writer = CfdWriter::new();
    writer
        .write_field(&WriteCellRequest {
            origin: &origin,
            record_key: "sword",
            actual_type: "Item",
            field_path: &segments,
            new_value: &value,
            schema: &schema,
        })
        .expect("insert missing field");
    writer.publish().expect("publish staged field");

    let after = fs::read_to_string(&file).expect("re-read");
    assert_eq!(after, "sword: Item {\n  value: 42,\n}\n");
}

#[test]
fn writes_field_inside_polymorphic_block_using_type_marker() {
    let dir = temp_dir("polymorphic-field");
    let file = dir.join("stages.cfd");
    fs::write(
        &file,
        r"stage_start: Stage {
  first_clear_reward: ItemReward { item: &sword, count: 1 },
}
",
    )
    .expect("write seed");

    let schema = compile_schema(
        r"
        type Item {
          name: string;
        }

        abstract type Reward {}

        type ItemReward : Reward {
          item: &Item;
          count: int;
        }

        type Stage {
          first_clear_reward: Reward;
        }
        ",
    );

    let schema = &schema;
    let writer = CfdWriter::new();
    let request_value = CfdValue::record_ref("blade").unwrap();
    let segments = vec![
        WriteFieldPathSegment::Field("first_clear_reward".to_string()),
        WriteFieldPathSegment::Field("item".to_string()),
    ];
    let origin = origin_for(&file);
    let request = WriteCellRequest {
        origin: &origin,
        record_key: "stage_start",
        actual_type: "Stage",
        field_path: &segments,
        new_value: &request_value,
        schema: schema,
    };
    writer.write_field(&request).expect("write succeeds");
    writer.publish().expect("publish staged field");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("ItemReward { item: &blade, count: 1 }"),
        "expected polymorphic field ref update: {after}"
    );
}

#[test]
fn writes_record_by_exact_type_when_unrelated_types_share_key() {
    let dir = temp_dir("same-key-write");
    let file = dir.join("records.cfd");
    fs::write(
        &file,
        r#"shared: Item {
  name: "Old Item",
}

shared: Skill {
  name: "Old Skill",
}
"#,
    )
    .expect("write seed");

    let schema = compile_schema(
        r"
        type Item { name: string; }
        type Skill { name: string; }
        ",
    );

    let schema = &schema;
    let writer = CfdWriter::new();
    let request_value = CfdValue::String("New Skill".to_string());
    let segments = vec![WriteFieldPathSegment::Field("name".to_string())];
    let origin = origin_for(&file);

    writer
        .write_field(&WriteCellRequest {
            origin: &origin,
            record_key: "shared",
            actual_type: "Skill",
            field_path: &segments,
            new_value: &request_value,
            schema: schema,
        })
        .expect("write skill");
    writer.publish().expect("publish staged field");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("shared: Item {\n  name: \"Old Item\""),
        "item should be untouched: {after}"
    );
    assert!(
        after.contains("shared: Skill {\n  name: \"New Skill\""),
        "skill should be updated: {after}"
    );
}

#[test]
fn writes_ref_type_as_key_ref() {
    let dir = temp_dir("ref");
    let file = dir.join("data.cfd");
    fs::write(
        &file,
        r#"target_a: Item {
  name: "Apple",
}

target_b: Item {
  name: "Banana",
}

picker: Holder {
  current: &target_a,
}
"#,
    )
    .expect("write seed");

    let schema = compile_schema(
        r"
        type Item {
          name: string;
        }

        type Holder {
          current: &Item;
        }
        ",
    );

    let schema = &schema;
    let model = load_cfd_model(&schema, &fs::read_to_string(&file).expect("read seed"))
        .expect("load model");
    let _ = model
        .lookup_assignable(&schema, "Item", "target_b")
        .expect("target_b id");

    let writer = CfdWriter::new();
    let new_value = CfdValue::record_ref("target_b").unwrap();
    let segments = vec![WriteFieldPathSegment::Field("current".to_string())];
    let origin = origin_for(&file);
    writer
        .write_field(&WriteCellRequest {
            origin: &origin,
            record_key: "picker",
            actual_type: "Holder",
            field_path: &segments,
            new_value: &new_value,
            schema: schema,
        })
        .expect("write succeeds");
    writer.publish().expect("publish staged field");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("&target_b"),
        "expected key ref form, got: {after}"
    );
    // The new file must still re-parse with the same loader.
    let records = parse_cfd_input_records(&schema, &after).expect("re-parse");
    let picker = records
        .iter()
        .find(|r| r.key == "picker")
        .expect("picker record");
    let _ = picker;
}

#[test]
fn ref_to_unknown_target_uses_short_form() {
    // Unknown targets remain key references; semantic validation happens
    // when the project generation is rebuilt.
    let dir = temp_dir("ref-fallback");
    let file = dir.join("data.cfd");
    fs::write(
        &file,
        r#"target: Item {
  name: "X",
}

picker: Holder {
  current: &target,
}
"#,
    )
    .expect("write seed");

    let schema = compile_schema(
        r"
        type Item {
          name: string;
        }

        type Holder {
          current: &Item;
        }
        ",
    );

    let schema = &schema;

    let writer = CfdWriter::new();
    let new_value = CfdValue::record_ref("ghost").unwrap();
    let segments = vec![WriteFieldPathSegment::Field("current".to_string())];
    let origin = origin_for(&file);
    writer
        .write_field(&WriteCellRequest {
            origin: &origin,
            record_key: "picker",
            actual_type: "Holder",
            field_path: &segments,
            new_value: &new_value,
            schema: schema,
        })
        .expect("write succeeds");
    writer.publish().expect("publish staged field");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("&ghost"),
        "expected key ref form, got: {after}"
    );
}

#[test]
fn rejects_empty_reference_key_at_value_boundary() {
    assert!(CfdValue::record_ref("").is_err());
}

#[test]
fn inserts_record_at_end_of_cfd_file() {
    let dir = temp_dir("insert-record");
    let file = dir.join("items.cfd");
    fs::write(
        &file,
        r#"sword: Item {
  name: "Sword",
  value: 10,
}
"#,
    )
    .expect("write seed");
    let schema = compile_schema(
        r"
        type Item {
          name: string;
          value: int;
        }
        ",
    );
    let schema = &schema;
    let source = empty_source(&file);
    let writer = CfdWriter::new();
    let fields = std::collections::BTreeMap::from([
        ("name".to_string(), CfdValue::String("Potion".to_string())),
        ("value".to_string(), CfdValue::Int(3)),
    ]);

    let outcome = writer
        .insert_record(&InsertRecordRequest {
            source: &source,
            record_key: "potion",
            actual_type: "Item",
            fields: &fields,
            schema: schema,
            before: None,
        })
        .expect("insert succeeds");
    writer.publish().expect("publish staged insert");

    assert!(outcome.diagnostics.is_empty());
    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("potion: Item {\n  name: \"Potion\",\n  value: 3,\n}\n"),
        "inserted records should use two-space indentation: {after}"
    );
    let model = load_cfd_model(&schema, &after).expect("reload");
    assert!(model.lookup_assignable(&schema, "Item", "potion").is_some());
}

#[test]
fn insert_record_allows_same_key_for_unrelated_types_in_same_file() {
    let dir = temp_dir("same-key-insert");
    let file = dir.join("records.cfd");
    fs::write(
        &file,
        r#"shared: Item {
  name: "Item",
}
"#,
    )
    .expect("write seed");
    let schema = compile_schema(
        r"
        type Item { name: string; }
        type Skill { name: string; }
        ",
    );
    let schema = &schema;
    let source = empty_source(&file);
    let writer = CfdWriter::new();
    let fields = std::collections::BTreeMap::from([(
        "name".to_string(),
        CfdValue::String("Skill".to_string()),
    )]);

    writer
        .insert_record(&InsertRecordRequest {
            source: &source,
            record_key: "shared",
            actual_type: "Skill",
            fields: &fields,
            schema: schema,
            before: None,
        })
        .expect("insert unrelated same-key skill");
    writer.publish().expect("publish staged insert");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("shared: Item"),
        "original item should remain: {after}"
    );
    assert!(
        after.contains("shared: Skill"),
        "same-key skill should be appended: {after}"
    );
    load_cfd_model(&schema, &after).expect("same-key unrelated domains should load");
}

#[test]
fn inserts_record_serializes_nested_ref_fields_with_ref_syntax() {
    let dir = temp_dir("insert-nested-ref");
    let file = dir.join("loot.cfd");
    fs::write(
        &file,
        r#"sword: Item {
  name: "Sword",
}
"#,
    )
    .expect("write seed");
    let schema = compile_schema(
        r"
        type Item {
          name: string;
        }

        type Slot {
          item: &Item;
        }

        type Loot {
          slot: Slot;
        }
        ",
    );
    let schema = &schema;
    let source = empty_source(&file);
    let writer = CfdWriter::new();
    let slot_fields = std::collections::BTreeMap::from([(
        "item".to_string(),
        CfdValue::record_ref("sword").unwrap(),
    )]);
    let fields = std::collections::BTreeMap::from([(
        "slot".to_string(),
        CfdValue::Object(Box::new(CfdObject::try_new("Slot", slot_fields).unwrap())),
    )]);

    writer
        .insert_record(&InsertRecordRequest {
            source: &source,
            record_key: "starter",
            actual_type: "Loot",
            fields: &fields,
            schema: schema,
            before: None,
        })
        .expect("insert succeeds");
    writer.publish().expect("publish staged insert");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("  slot: Slot {\n    item: &sword,\n  },"),
        "nested fields should use one two-space indentation unit per level: {after}"
    );
    let model = load_cfd_model(&schema, &after).expect("reload");
    assert!(model
        .lookup_assignable(&schema, "Loot", "starter")
        .is_some());
}

#[test]
fn deletes_record_span_from_cfd_file() {
    let dir = temp_dir("delete-record");
    let file = dir.join("items.cfd");
    fs::write(
        &file,
        r#"sword: Item {
  name: "Sword",
}

shield: Item {
  name: "Shield",
}
"#,
    )
    .expect("write seed");
    let schema = compile_schema(
        r"
        type Item {
          name: string;
        }
        ",
    );
    let schema = &schema;
    let origin = origin_for(&file);
    let writer = CfdWriter::new();

    writer
        .delete_record(&DeleteRecordRequest {
            origin: &origin,
            record_key: "sword",
            actual_type: "Item",
        })
        .expect("delete succeeds");
    writer.publish().expect("publish staged delete");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(!after.contains("sword: Item"));
    assert!(after.contains("shield: Item"));
    let model = load_cfd_model(&schema, &after).expect("reload");
    assert!(model.lookup_assignable(&schema, "Item", "sword").is_none());
    assert!(model.lookup_assignable(&schema, "Item", "shield").is_some());
}

#[test]
fn delete_record_uses_exact_type_when_unrelated_types_share_key() {
    let dir = temp_dir("same-key-delete");
    let file = dir.join("records.cfd");
    fs::write(
        &file,
        r#"shared: Item {
  name: "Item",
}

shared: Skill {
  name: "Skill",
}
"#,
    )
    .expect("write seed");
    let origin = origin_for(&file);
    let writer = CfdWriter::new();

    writer
        .delete_record(&DeleteRecordRequest {
            origin: &origin,
            record_key: "shared",
            actual_type: "Skill",
        })
        .expect("delete skill");
    writer.publish().expect("publish staged delete");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("shared: Item"),
        "item should remain after deleting skill: {after}"
    );
    assert!(
        !after.contains("shared: Skill"),
        "skill should be deleted: {after}"
    );
}

#[test]
fn writes_enum_dict_key_path_using_member_display_text() {
    let dir = temp_dir("enum-dict-key-path");
    let file = dir.join("loot.cfd");
    fs::write(
        &file,
        r"starter: Loot {
  resistances: { Element::Fire: 10 },
}
",
    )
    .expect("write seed");

    let schema = compile_schema(
        r"
        enum Element { Fire = 1, Ice = 2 }

        type Loot {
          resistances: {Element: int};
        }
        ",
    );

    let schema = &schema;
    let writer = CfdWriter::new();
    let new_value = CfdValue::Int(20);
    let segments = vec![
        WriteFieldPathSegment::Field("resistances".to_string()),
        WriteFieldPathSegment::DictKey("Element::Fire".to_string()),
    ];
    let origin = origin_for(&file);
    writer
        .write_field(&WriteCellRequest {
            origin: &origin,
            record_key: "starter",
            actual_type: "Loot",
            field_path: &segments,
            new_value: &new_value,
            schema: schema,
        })
        .expect("write succeeds");
    writer.publish().expect("publish staged field");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("Fire: 20"),
        "expected enum dict entry to be updated: {after}"
    );
}

#[test]
fn writes_group_record_without_required_commas() {
    let dir = temp_dir("group-no-commas");
    let file = dir.join("effects.cfd");
    fs::write(
        &file,
        r"DamageEffect {
  eff_fireball_damage {
    damage: { lo: 6, hi: 6 },
    pierce_divine: false,
  }

  eff_execute_damage {
    damage: { lo: 999, hi: 999 },
    pierce_divine: false,
  }
}
",
    )
    .expect("write seed");

    let schema = compile_schema(
        r"
        type IntRange {
          lo: int;
          hi: int;
        }

        type DamageEffect {
          damage: IntRange;
          pierce_divine: bool;
        }
        ",
    );

    let schema = &schema;
    let writer = CfdWriter::new();
    let new_value = CfdValue::Int(7);
    let segments = vec![
        WriteFieldPathSegment::Field("damage".to_string()),
        WriteFieldPathSegment::Field("lo".to_string()),
    ];
    let origin = origin_for(&file);
    writer
        .write_field(&WriteCellRequest {
            origin: &origin,
            record_key: "eff_fireball_damage",
            actual_type: "DamageEffect",
            field_path: &segments,
            new_value: &new_value,
            schema: schema,
        })
        .expect("write succeeds");
    writer.publish().expect("publish staged field");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("damage: { lo: 7, hi: 6 }"),
        "target record should be updated: {after}"
    );
    assert!(
        after.contains("damage: { lo: 999, hi: 999 }"),
        "sibling record should remain unchanged: {after}"
    );
}

#[test]
fn write_reports_parse_diagnostics_instead_of_missing_record_for_bad_cfd() {
    let dir = temp_dir("parse-diagnostic");
    let file = dir.join("items.cfd");
    fs::write(
        &file,
        r"// not a CFD comment
sword: Item {
  value: 1,
}
",
    )
    .expect("write seed");

    let schema = compile_schema(
        r"
        type Item {
          value: int;
        }
        ",
    );

    let schema = &schema;
    let writer = CfdWriter::new();
    let new_value = CfdValue::Int(2);
    let segments = vec![WriteFieldPathSegment::Field("value".to_string())];
    let origin = origin_for(&file);
    let err = writer
        .write_field(&WriteCellRequest {
            origin: &origin,
            record_key: "sword",
            actual_type: "Item",
            field_path: &segments,
            new_value: &new_value,
            schema: schema,
        })
        .expect_err("invalid CFD syntax should fail before patching");

    assert!(
        err.iter()
            .any(|diagnostic| diagnostic.message.contains("failed to parse")),
        "expected parse diagnostic, got: {err:?}"
    );
    assert!(
        err.iter()
            .all(|diagnostic| !diagnostic.message.contains("not found in AST")),
        "parse errors should not be masked as missing records: {err:?}"
    );
}

#[test]
fn rewrites_polymorphic_objects_and_arrays_as_valid_cfd() {
    let dir = temp_dir("polymorphic-object-round-trip");
    let file = dir.join("07-inheritance.cfd");
    let original = format_cfd(include_str!(
        "../../../../../examples/showcase/data/07-inheritance.cfd"
    ));
    fs::write(&file, &original).expect("write inheritance seed");
    let schema = compile_schema(include_str!(
        "../../../../../examples/showcase/schema/07-inheritance.cft"
    ));
    let model = load_cfd_model(&schema, &original).expect("load inheritance model");
    let record_id = model
        .lookup_assignable(&schema, "EffectBundle", "starter_effects")
        .expect("effect bundle record");
    let record = model.record(record_id).expect("effect bundle value");
    let primary = record.field("primary").expect("primary effect").clone();
    let CfdValue::Object(primary_object) = &primary else {
        panic!("primary effect should be an object");
    };
    assert_eq!(primary_object.actual_type.as_str(), "HealEffect");
    let CfdValue::Array(additional) = record.field("additional").expect("additional effects")
    else {
        panic!("additional effects should be an array");
    };
    let mut appended_effects = additional.clone();
    appended_effects.push(additional[0].clone());
    let expected_additional_len = additional.len() + 1;

    let writer = CfdWriter::new();
    let origin = origin_for(&file);
    let primary_path = vec![WriteFieldPathSegment::Field("primary".to_string())];
    writer
        .write_field(&WriteCellRequest {
            origin: &origin,
            record_key: "starter_effects",
            actual_type: "EffectBundle",
            field_path: &primary_path,
            new_value: &primary,
            schema: &schema,
        })
        .expect("rewrite primary effect object");

    let additional_path = vec![WriteFieldPathSegment::Field("additional".to_string())];
    writer
        .write_field(&WriteCellRequest {
            origin: &origin,
            record_key: "starter_effects",
            actual_type: "EffectBundle",
            field_path: &additional_path,
            new_value: &CfdValue::Array(appended_effects),
            schema: &schema,
        })
        .expect("rewrite additional effects");
    writer.publish().expect("publish staged rewrite");

    let after = fs::read_to_string(&file).expect("read rewritten chemical equation");
    assert_eq!(after, format_cfd(&after), "writer output must already be formatted");
    let rewritten = load_cfd_model(&schema, &after).expect("reload rewritten effect bundle");
    let rewritten_id = rewritten
        .lookup_assignable(&schema, "EffectBundle", "starter_effects")
        .expect("rewritten effect bundle record");
    let rewritten_record = rewritten.record(rewritten_id).expect("rewritten record");
    let CfdValue::Array(rewritten_additional) = rewritten_record
        .field("additional")
        .expect("rewritten additional effects")
    else {
        panic!("rewritten additional effects should be an array");
    };
    assert_eq!(rewritten_additional.len(), expected_additional_len);
}
