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

use coflow_api::{
    DeleteRecordRequest, InsertRecordRequest, ResolvedSource, RewriteRecordReferencesRequest,
    SourceLocationSpec, SourceProvider, SourceWriter, SpreadRewriteTarget, WriteCellRequest,
    WriteContext, WriteFieldPathSegment,
};
use coflow_cft::{build_schema, parse_modules, CftDimensionInputs, CftFile, CftSchema, ModuleId};
use coflow_data_model::{CfdDataModel, CfdObject, CfdValue, RecordOrigin, TextSpan};
use coflow_loader_cfd::{load_cfd_model, parse_cfd_input_records, CfdLoader, CfdWriter};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

fn temp_dir(name: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("coflow-cfd-writer-{name}-{id}"));
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

fn empty_source(path: &Path) -> ResolvedSource {
    ResolvedSource {
        provider_id: "cfd".to_string(),
        location: SourceLocationSpec::Path(path.to_path_buf()),
        options: CfdLoader
            .decode_options(&serde_json::Value::Null)
            .expect("decode cfd options"),
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
    let source = empty_source(&file);
    let origin = origin_for(&file);
    let model = empty_model(&schema);
    let request = WriteCellRequest {
        origin: &origin,
        record_key: "sword",
        actual_type: "Item",
        field_path: &segments,
        new_value: &request_value,
        schema: schema,
        source: &source,
    };
    writer
        .write_field(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: Some(&model),
            },
            &request,
        )
        .expect("write succeeds");

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
    let request_value = CfdValue::Ref("blade".to_string());
    let segments = vec![
        WriteFieldPathSegment::Field("first_clear_reward".to_string()),
        WriteFieldPathSegment::Field("item".to_string()),
    ];
    let source = empty_source(&file);
    let origin = origin_for(&file);
    let model = empty_model(&schema);
    let request = WriteCellRequest {
        origin: &origin,
        record_key: "stage_start",
        actual_type: "Stage",
        field_path: &segments,
        new_value: &request_value,
        schema: schema,
        source: &source,
    };
    writer
        .write_field(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: Some(&model),
            },
            &request,
        )
        .expect("write succeeds");

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
    let source = empty_source(&file);
    let origin = origin_for(&file);
    let model = load_cfd_model(&schema, &fs::read_to_string(&file).expect("read seed"))
        .expect("load model");

    writer
        .write_field(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: Some(&model),
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "shared",
                actual_type: "Skill",
                field_path: &segments,
                new_value: &request_value,
                schema: schema,
                source: &source,
            },
        )
        .expect("write skill");

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
        .lookup_assignable("Item", "target_b")
        .expect("target_b id");

    let writer = CfdWriter::new();
    let new_value = CfdValue::Ref("target_b".to_string());
    let segments = vec![WriteFieldPathSegment::Field("current".to_string())];
    let source = empty_source(&file);
    let origin = origin_for(&file);
    writer
        .write_field(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: Some(&model),
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "picker",
                actual_type: "Holder",
                field_path: &segments,
                new_value: &new_value,
                schema: schema,
                source: &source,
            },
        )
        .expect("write succeeds");

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
    // When the model is None (or doesn't contain the target), the writer
    // falls back to `&key`. This test pins that behavior so callers know
    // they need to provide a model to get qualified refs.
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
    let new_value = CfdValue::Ref("ghost".to_string());
    let segments = vec![WriteFieldPathSegment::Field("current".to_string())];
    let source = empty_source(&file);
    let origin = origin_for(&file);
    writer
        .write_field(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: None,
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "picker",
                actual_type: "Holder",
                field_path: &segments,
                new_value: &new_value,
                schema: schema,
                source: &source,
            },
        )
        .expect("write succeeds");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("&ghost"),
        "expected key ref form, got: {after}"
    );
}

#[test]
fn rejects_empvalue_type_key() {
    let dir = temp_dir("empty-ref");
    let file = dir.join("data.cfd");
    fs::write(
        &file,
        r"picker: Holder {
  current: &x,
}
",
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
    let new_value = CfdValue::Ref(String::new());
    let segments = vec![WriteFieldPathSegment::Field("current".to_string())];
    let source = empty_source(&file);
    let origin = origin_for(&file);
    let model = empty_model(&schema);
    let result = writer.write_field(
        WriteContext {
            project_root: &dir,
            schema: schema,
            model: Some(&model),
        },
        &WriteCellRequest {
            origin: &origin,
            record_key: "picker",
            actual_type: "Holder",
            field_path: &segments,
            new_value: &new_value,
            schema: schema,
            source: &source,
        },
    );
    let Err(diag) = result else {
        panic!("empty ref should be rejected");
    };
    assert!(diag.iter().any(|d| d.message.contains("empty reference")));
}

fn empty_model(schema: &CftSchema) -> CfdDataModel {
    CfdDataModel::builder(schema).build().expect("empty model")
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
        .insert_record(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: None,
            },
            &InsertRecordRequest {
                source: &source,
                sheet: None,
                record_key: "potion",
                actual_type: "Item",
                fields: &fields,
                schema: schema,
            },
        )
        .expect("insert succeeds");

    assert!(outcome.diagnostics.is_empty());
    let after = fs::read_to_string(&file).expect("re-read");
    assert!(after.contains("potion: Item"));
    assert!(after.contains("name: \"Potion\""));
    assert!(after.contains("value: 3"));
    let model = load_cfd_model(&schema, &after).expect("reload");
    assert!(model.lookup_assignable("Item", "potion").is_some());
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
        .insert_record(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: None,
            },
            &InsertRecordRequest {
                source: &source,
                sheet: None,
                record_key: "shared",
                actual_type: "Skill",
                fields: &fields,
                schema: schema,
            },
        )
        .expect("insert unrelated same-key skill");

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
        CfdValue::Ref("sword".to_string()),
    )]);
    let fields = std::collections::BTreeMap::from([(
        "slot".to_string(),
        CfdValue::Object(Box::new(CfdObject::new("Slot", slot_fields))),
    )]);

    writer
        .insert_record(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: None,
            },
            &InsertRecordRequest {
                source: &source,
                sheet: None,
                record_key: "starter",
                actual_type: "Loot",
                fields: &fields,
                schema: schema,
            },
        )
        .expect("insert succeeds");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("item: &sword"),
        "expected & ref syntax: {after}"
    );
    let model = load_cfd_model(&schema, &after).expect("reload");
    assert!(model.lookup_assignable("Loot", "starter").is_some());
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
    let source = empty_source(&file);
    let origin = origin_for(&file);
    let writer = CfdWriter::new();

    writer
        .delete_record(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: None,
            },
            &DeleteRecordRequest {
                origin: &origin,
                record_key: "sword",
                actual_type: "Item",
                source: &source,
            },
        )
        .expect("delete succeeds");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(!after.contains("sword: Item"));
    assert!(after.contains("shield: Item"));
    let model = load_cfd_model(&schema, &after).expect("reload");
    assert!(model.lookup_assignable("Item", "sword").is_none());
    assert!(model.lookup_assignable("Item", "shield").is_some());
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
    let schema = compile_schema(
        r"
        type Item { name: string; }
        type Skill { name: string; }
        ",
    );
    let schema = &schema;
    let source = empty_source(&file);
    let origin = origin_for(&file);
    let writer = CfdWriter::new();

    writer
        .delete_record(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: None,
            },
            &DeleteRecordRequest {
                origin: &origin,
                record_key: "shared",
                actual_type: "Skill",
                source: &source,
            },
        )
        .expect("delete skill");

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
fn writes_into_top_level_spread_creates_local_override() {
    // Editing a top-level field that was inherited via a record-level
    // `...&source` spread (no local declaration) should also insert a
    // local override on the elite record, not mutate the source.
    let dir = temp_dir("top-spread");
    let file = dir.join("monsters.cfd");
    fs::write(
        &file,
        r#"basic_monster: Monster {
  name: "Dummy",
  stats: { hp: 100, attack: 5 },
}

elite_monster: Monster {
  ...&basic_monster,
}
"#,
    )
    .expect("write seed");

    let schema = compile_schema(
        r"
        type Stats {
          hp: int;
          attack: int;
        }

        type Monster {
          name: string;
          stats: Stats;
        }
        ",
    );

    let schema = &schema;
    let model = load_cfd_model(&schema, &fs::read_to_string(&file).expect("read seed"))
        .expect("load model");

    let writer = CfdWriter::new();
    let new_value = CfdValue::String("Boss".to_string());
    let segments = vec![WriteFieldPathSegment::Field("name".to_string())];
    let source = empty_source(&file);
    let origin = origin_for(&file);
    writer
        .write_field(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: Some(&model),
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "elite_monster",
                actual_type: "Monster",
                field_path: &segments,
                new_value: &new_value,
                schema: schema,
                source: &source,
            },
        )
        .expect("write succeeds");

    let after = fs::read_to_string(&file).expect("re-read");
    // The spread should remain.
    assert!(
        after.contains("...&basic_monster"),
        "top-level spread should remain: {after}"
    );
    // basic_monster.name MUST stay "Dummy".
    assert!(
        after.contains("\"Dummy\""),
        "source record name unchanged: {after}"
    );
    // The new local override appears.
    assert!(
        after.contains("\"Boss\""),
        "elite override appears: {after}"
    );
    // Verify the model picks up the override.
    let model = load_cfd_model(&schema, &after).expect("re-load");
    let elite = model
        .lookup_assignable("Monster", "elite_monster")
        .and_then(|id| model.record(id))
        .expect("elite");
    assert_eq!(
        elite.field("name"),
        Some(&CfdValue::String("Boss".to_string()))
    );
}

#[test]
fn rewrites_only_requested_spread_source_site() {
    let dir = temp_dir("rewrite-spread-site");
    let file = dir.join("records.cfd");
    fs::write(
        &file,
        r#"base: Holder {
  item: &sword,
  label: "&base",
}

copy: Holder {
  ...&base,
}

direct: Holder {
  item: &base,
  label: "&base",
}

base: OtherHolder {
  item: &sword,
  label: "other",
}

other_copy: OtherHolder {
  ...&base,
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
          item: &Item;
          label: string;
        }

        type OtherHolder {
          item: &Item;
          label: string;
        }
        ",
    );
    let schema = &schema;
    let writer = CfdWriter::new();
    let source = empty_source(&file);
    let origin = origin_for(&file);
    let targets = [SpreadRewriteTarget {
        origin,
        record_key: "copy".to_string(),
        actual_type: "Holder".to_string(),
        object_path: Vec::new(),
    }];
    let request = RewriteRecordReferencesRequest {
        source: &source,
        old_key: "base",
        new_key: "renamed",
        targets: &targets,
        schema: schema,
    };

    writer
        .rewrite_record_references(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: None,
            },
            &request,
        )
        .expect("rewrite spread");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("copy: Holder {\n  ...&renamed"),
        "requested spread should update: {after}"
    );
    assert!(
        after.contains("direct: Holder {\n  item: &base"),
        "direct ref should not be rewritten by spread rewrite: {after}"
    );
    assert!(
        after.contains("label: \"&base\""),
        "quoted string should not be rewritten: {after}"
    );
    assert!(
        after.contains("other_copy: OtherHolder {\n  ...&base"),
        "same-file unrelated same-key spread should not update: {after}"
    );
}

#[test]
fn rewrites_nested_array_and_dict_spread_source_sites() {
    let dir = temp_dir("rewrite-nested-spread-site");
    let file = dir.join("records.cfd");
    fs::write(
        &file,
        r#"base: Stats {
  hp: 1,
}

host: Loadout {
  items: [
    { ...&base },
  ],
  map: { "first": { ...&base } },
}
"#,
    )
    .expect("write seed");
    let schema = compile_schema(
        r"
        type Stats {
          hp: int;
        }

        type Loadout {
          items: [Stats];
          map: {string: Stats};
        }
        ",
    );
    let schema = &schema;
    let writer = CfdWriter::new();
    let source = empty_source(&file);
    let origin = origin_for(&file);
    let targets = [
        SpreadRewriteTarget {
            origin: origin.clone(),
            record_key: "host".to_string(),
            actual_type: "Loadout".to_string(),
            object_path: vec![
                WriteFieldPathSegment::Field("items".to_string()),
                WriteFieldPathSegment::Index(0),
            ],
        },
        SpreadRewriteTarget {
            origin,
            record_key: "host".to_string(),
            actual_type: "Loadout".to_string(),
            object_path: vec![
                WriteFieldPathSegment::Field("map".to_string()),
                WriteFieldPathSegment::DictKey("\"first\"".to_string()),
            ],
        },
    ];
    let request = RewriteRecordReferencesRequest {
        source: &source,
        old_key: "base",
        new_key: "renamed",
        targets: &targets,
        schema: schema,
    };

    writer
        .rewrite_record_references(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: None,
            },
            &request,
        )
        .expect("rewrite nested spreads");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("{ ...&renamed }"),
        "array spread should update: {after}"
    );
    assert!(
        after.contains(r#""first": { ...&renamed }"#),
        "dict spread should update: {after}"
    );
}

#[test]
fn deep_drill_into_nonexistent_spread_field_errors_clearly() {
    // Path that drills *past* an inherited-but-not-locally-materialised
    // field is unsupported. The writer surfaces a clear error rather than
    // corrupting the file.
    let dir = temp_dir("deep-drill");
    let file = dir.join("monsters.cfd");
    fs::write(
        &file,
        r#"basic: Monster {
  name: "X",
}

elite: Monster {
  ...&basic,
}
"#,
    )
    .expect("write seed");

    let schema = compile_schema(
        r"
        type Stats {
          hp: int;
          attack: int;
        }

        type Monster {
          name: string;
          stats: Stats?;
        }
        ",
    );

    let schema = &schema;

    let writer = CfdWriter::new();
    let new_value = CfdValue::Int(42);
    let segments = vec![
        WriteFieldPathSegment::Field("stats".to_string()),
        WriteFieldPathSegment::Field("attack".to_string()),
    ];
    let source = empty_source(&file);
    let origin = origin_for(&file);
    let model = empty_model(&schema);
    let result = writer.write_field(
        WriteContext {
            project_root: &dir,
            schema: schema,
            model: Some(&model),
        },
        &WriteCellRequest {
            origin: &origin,
            record_key: "elite",
            actual_type: "Monster",
            field_path: &segments,
            new_value: &new_value,
            schema: schema,
            source: &source,
        },
    );
    let Err(diag) = result else {
        panic!("deep drill into spread should fail");
    };
    assert!(
        diag.iter()
            .any(|d| d.message.contains("not found") || d.message.contains("spread")),
        "expected actionable diagnostic, got: {diag:?}"
    );
}

#[test]
fn writes_enum_dict_key_path_using_qualified_display_text() {
    let dir = temp_dir("enum-dict-key-path");
    let file = dir.join("loot.cfd");
    fs::write(
        &file,
        r"starter: Loot {
  resistances: { Fire: 10 },
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
    let model = load_cfd_model(&schema, &fs::read_to_string(&file).expect("read seed"))
        .expect("load model");

    let writer = CfdWriter::new();
    let new_value = CfdValue::Int(20);
    let segments = vec![
        WriteFieldPathSegment::Field("resistances".to_string()),
        WriteFieldPathSegment::DictKey("Element.Fire".to_string()),
    ];
    let source = empty_source(&file);
    let origin = origin_for(&file);
    writer
        .write_field(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: Some(&model),
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "starter",
                actual_type: "Loot",
                field_path: &segments,
                new_value: &new_value,
                schema: schema,
                source: &source,
            },
        )
        .expect("write succeeds");

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
    let model = load_cfd_model(&schema, &fs::read_to_string(&file).expect("read seed"))
        .expect("load model");

    let writer = CfdWriter::new();
    let new_value = CfdValue::Int(7);
    let segments = vec![
        WriteFieldPathSegment::Field("damage".to_string()),
        WriteFieldPathSegment::Field("lo".to_string()),
    ];
    let source = empty_source(&file);
    let origin = origin_for(&file);
    writer
        .write_field(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: Some(&model),
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "eff_fireball_damage",
                actual_type: "DamageEffect",
                field_path: &segments,
                new_value: &new_value,
                schema: schema,
                source: &source,
            },
        )
        .expect("write succeeds");

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
    let source = empty_source(&file);
    let origin = origin_for(&file);
    let model = empty_model(&schema);
    let err = writer
        .write_field(
            WriteContext {
                project_root: &dir,
                schema: schema,
                model: Some(&model),
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "sword",
                actual_type: "Item",
                field_path: &segments,
                new_value: &new_value,
                schema: schema,
                source: &source,
            },
        )
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
