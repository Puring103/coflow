//! Round-trip tests for `CfdWriter`: write a value, re-parse the file from
//! disk, assert the new value is reflected and that other records / fields
//! are unchanged.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

use coflow_api::{
    CfdValue, DataWriter, RecordOrigin, ResolvedSource, SourceLocationSpec, TextSpan,
    WriteCellRequest, WriteContext, WriteFieldPathSegment,
};
use coflow_cft::{CftContainer, ModuleId};
use coflow_data_model::CfdDataModel;
use coflow_loader_cfd::{load_cfd_model, parse_cfd_input_records, CfdWriter};
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

fn compile_schema(source: &str) -> CftContainer {
    let mut container = CftContainer::new();
    container
        .add_module(ModuleId::from("main"), source)
        .expect("schema parse");
    container.compile().expect("schema compile");
    container
}

fn empty_source(path: &Path) -> ResolvedSource {
    ResolvedSource {
        provider_id: "cfd".to_string(),
        location: SourceLocationSpec::Path(path.to_path_buf()),
        options: serde_json::Value::default(),
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
        schema: &schema,
        source: &source,
    };
    writer
        .write_field(
            WriteContext {
                project_root: &dir,
                schema: &schema,
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
fn writes_typed_ref_uses_qualified_form() {
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
  current: @Item.target_a,
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
          current: Item;
        }
        ",
    );
    let model = load_cfd_model(&schema, &fs::read_to_string(&file).expect("read seed"))
        .expect("load model");
    // Resolve target_b's record id so the writer emits @Item.target_b.
    let target_b_id = model.lookup("Item", "target_b").expect("target_b id");

    let writer = CfdWriter::new();
    let new_value = CfdValue::Ref {
        key: "target_b".to_string(),
        target: target_b_id,
    };
    let segments = vec![WriteFieldPathSegment::Field("current".to_string())];
    let source = empty_source(&file);
    let origin = origin_for(&file);
    writer
        .write_field(
            WriteContext {
                project_root: &dir,
                schema: &schema,
                model: Some(&model),
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "picker",
                actual_type: "Holder",
                field_path: &segments,
                new_value: &new_value,
                schema: &schema,
                source: &source,
            },
        )
        .expect("write succeeds");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("@Item.target_b"),
        "expected qualified ref form, got: {after}"
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
  current: @Item.target,
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
          current: Item;
        }
        ",
    );

    let writer = CfdWriter::new();
    let new_value = CfdValue::Ref {
        key: "ghost".to_string(),
        // Sentinel id — the writer will not find this in the (None) model
        // and must fall back to short-form. The text might fail to type-check
        // on reload; that's the caller's problem, not the writer's.
        target: coflow_data_model::CfdRecordId::from_index(usize::MAX),
    };
    let segments = vec![WriteFieldPathSegment::Field("current".to_string())];
    let source = empty_source(&file);
    let origin = origin_for(&file);
    writer
        .write_field(
            WriteContext {
                project_root: &dir,
                schema: &schema,
                model: None,
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "picker",
                actual_type: "Holder",
                field_path: &segments,
                new_value: &new_value,
                schema: &schema,
                source: &source,
            },
        )
        .expect("write succeeds");

    let after = fs::read_to_string(&file).expect("re-read");
    assert!(
        after.contains("&ghost"),
        "expected short-form ref fallback, got: {after}"
    );
}

#[test]
fn rejects_empty_ref_key() {
    let dir = temp_dir("empty-ref");
    let file = dir.join("data.cfd");
    fs::write(
        &file,
        r"picker: Holder {
  current: @Item.x,
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
          current: Item;
        }
        ",
    );

    let writer = CfdWriter::new();
    let new_value = CfdValue::Ref {
        key: String::new(),
        target: coflow_data_model::CfdRecordId::from_index(0),
    };
    let segments = vec![WriteFieldPathSegment::Field("current".to_string())];
    let source = empty_source(&file);
    let origin = origin_for(&file);
    let model = empty_model(&schema);
    let result = writer.write_field(
        WriteContext {
            project_root: &dir,
            schema: &schema,
            model: Some(&model),
        },
        &WriteCellRequest {
            origin: &origin,
            record_key: "picker",
            actual_type: "Holder",
            field_path: &segments,
            new_value: &new_value,
            schema: &schema,
            source: &source,
        },
    );
    let Err(diag) = result else {
        panic!("empty ref should be rejected");
    };
    assert!(diag.iter().any(|d| d.message.contains("empty reference")));
}

fn empty_model(schema: &CftContainer) -> CfdDataModel {
    CfdDataModel::builder(schema).build().expect("empty model")
}

#[test]
fn writes_into_nested_spread_block_inserts_local_override() {
    // The classic spread pattern: `elite_monster.stats` is built from a
    // `...@Monster.basic_monster.stats` spread plus an explicit `hp: 250`
    // override. Editing `elite_monster.stats.attack` should leave the
    // spread intact and *insert* a local `attack:` line into that nested
    // block, taking precedence on the next reload.
    let dir = temp_dir("nested-spread");
    let file = dir.join("monsters.cfd");
    fs::write(
        &file,
        r#"basic_monster: Monster {
  name: "Dummy",
  stats: { hp: 100, attack: 5 },
}

elite_monster: Monster {
  ...@Monster.basic_monster,
  name: "Elite",
  stats: {
    ...@Monster.basic_monster.stats,
    hp: 250,
  },
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
    let model = load_cfd_model(&schema, &fs::read_to_string(&file).expect("read seed"))
        .expect("load model");

    let writer = CfdWriter::new();
    let new_value = CfdValue::Int(99);
    let segments = vec![
        WriteFieldPathSegment::Field("stats".to_string()),
        WriteFieldPathSegment::Field("attack".to_string()),
    ];
    let source = empty_source(&file);
    let origin = origin_for(&file);
    writer
        .write_field(
            WriteContext {
                project_root: &dir,
                schema: &schema,
                model: Some(&model),
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "elite_monster",
                actual_type: "Monster",
                field_path: &segments,
                new_value: &new_value,
                schema: &schema,
                source: &source,
            },
        )
        .expect("write succeeds");

    let after = fs::read_to_string(&file).expect("re-read");
    // The spread line must still be there.
    assert!(
        after.contains("...@Monster.basic_monster.stats"),
        "spread should be preserved: {after}"
    );
    // The new override is in the elite stats block.
    assert!(
        after.contains("attack: 99"),
        "expected local override `attack: 99`: {after}"
    );
    // Re-parse and verify the model now has elite_monster.stats.attack = 99
    // and basic_monster.stats.attack is still 5.
    let model = load_cfd_model(&schema, &after).expect("re-load model");
    let elite = model
        .lookup("Monster", "elite_monster")
        .and_then(|id| model.record(id))
        .expect("elite_monster");
    let CfdValue::Object(stats) = elite.field("stats").expect("stats") else {
        panic!("elite_monster.stats should be an Object");
    };
    assert_eq!(
        stats.field("attack"),
        Some(&CfdValue::Int(99)),
        "elite override took effect"
    );
    let basic = model
        .lookup("Monster", "basic_monster")
        .and_then(|id| model.record(id))
        .expect("basic_monster");
    let CfdValue::Object(basic_stats) = basic.field("stats").expect("stats") else {
        panic!("basic_monster.stats should be an Object");
    };
    assert_eq!(
        basic_stats.field("attack"),
        Some(&CfdValue::Int(5)),
        "basic_monster.stats.attack must NOT be mutated"
    );
}

#[test]
fn writes_into_top_level_spread_creates_local_override() {
    // Editing a top-level field that was inherited via a record-level
    // `...@Source` spread (no local declaration) should also insert a
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
  ...@Monster.basic_monster,
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
                schema: &schema,
                model: Some(&model),
            },
            &WriteCellRequest {
                origin: &origin,
                record_key: "elite_monster",
                actual_type: "Monster",
                field_path: &segments,
                new_value: &new_value,
                schema: &schema,
                source: &source,
            },
        )
        .expect("write succeeds");

    let after = fs::read_to_string(&file).expect("re-read");
    // The spread should remain.
    assert!(
        after.contains("...@Monster.basic_monster"),
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
        .lookup("Monster", "elite_monster")
        .and_then(|id| model.record(id))
        .expect("elite");
    assert_eq!(
        elite.field("name"),
        Some(&CfdValue::String("Boss".to_string()))
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
  ...@Monster.basic,
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
            schema: &schema,
            model: Some(&model),
        },
        &WriteCellRequest {
            origin: &origin,
            record_key: "elite",
            actual_type: "Monster",
            field_path: &segments,
            new_value: &new_value,
            schema: &schema,
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
