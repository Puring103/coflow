#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

use cfd_editor_lib::editor::SessionStore;
use coflow_data_model::{CfdRecord, CfdValue, RecordOrigin};
use std::collections::BTreeMap;

#[test]
fn reload_session_rebuilds_from_changed_project_files() {
    let root = temp_project_dir("cfd-editor-reload");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root, "Sword");

    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    assert_record_name(&store, snapshot.session_id, "Sword");

    write_project(&root, "Blade");

    let reloaded = store
        .reload_session(snapshot.session_id)
        .expect("reload project from disk");
    assert_eq!(reloaded.session_id, snapshot.session_id);
    assert_record_name(&store, snapshot.session_id, "Blade");
}

#[test]
fn file_records_load_ref_type_fields_without_mode_wire_metadata() {
    let root = temp_project_dir("cfd-editor-field-mode");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item { name: string; }
            type Holder {
                item_ref: &Item;
                item_inline: Item;
                nested: Nested;
            }
            type Nested {
                nested_ref: &Item;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"
            sword: Item { name: "Sword" }
            holder: Holder {
              item_ref: &sword,
              item_inline: { name: "Inline" },
              nested: { nested_ref: &sword },
            }
        "#,
    )
    .expect("write data");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\n",
    )
    .expect("write config");

    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    let records = store
        .get_file_records(snapshot.session_id, "data/items.cfd")
        .expect("get file records");
    let holder = records
        .records
        .iter()
        .find(|row| row.coordinate.actual_type == "Holder")
        .expect("holder row");
    let item_ref = holder
        .fields
        .iter()
        .find(|field| field.name == "item_ref")
        .expect("item_ref field");
    let item_inline = holder
        .fields
        .iter()
        .find(|field| field.name == "item_inline")
        .expect("item_inline field");

    assert_eq!(
        item_ref
            .annotation
            .as_ref()
            .and_then(|annotation| annotation.ref_target_file.as_deref()),
        Some("data/items.cfd")
    );
    assert!(
        item_inline.annotation.is_none(),
        "inline fields should not carry ref or field-mode annotations"
    );
}

#[test]
fn insert_record_uses_engine_minimal_materialization_for_empty_editor_payload() {
    let root = temp_project_dir("cfd-editor-insert-minimal");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            enum Rarity { Common = 0, Rare = 1 }
            type Item {
                name: string;
                price: int;
                rarity: Rarity = Rarity.Common;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(root.join("data").join("items.cfd"), "").expect("write data");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\n",
    )
    .expect("write config");

    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    let payload = object_value("Item", BTreeMap::new());
    let outcome = store
        .insert_record(
            snapshot.session_id,
            "data/items.cfd",
            "potion",
            "Item",
            payload,
        )
        .expect("insert record");

    assert!(outcome.diagnostics.is_empty());
    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read data");
    assert!(text.contains("potion"));
    assert!(text.contains("name"));
    assert!(text.contains("price"));
    assert!(!text.contains("rarity"));
}

#[test]
fn insert_record_returns_mutation_diagnostics_for_missing_required_ref() {
    let root = temp_project_dir("cfd-editor-insert-ref-diagnostic");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item { name: string; }
            type Holder {
                item: &Item;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"sword: Item { name: "Sword" }"#,
    )
    .expect("write data");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\n",
    )
    .expect("write config");

    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    let err = store
        .insert_record(
            snapshot.session_id,
            "data/items.cfd",
            "bad_holder",
            "Holder",
            object_value("Holder", BTreeMap::new()),
        )
        .expect_err("missing required ref should fail");

    assert!(err
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-DEFAULT"));
    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read data");
    assert!(!text.contains("bad_holder"));
}

fn write_project(root: &std::path::Path, name: &str) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item {
                name: string;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        format!(r#"sword: Item {{ name: "{name}" }}"#),
    )
    .expect("write data");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\n",
    )
    .expect("write config");
}

fn object_value(actual_type: &str, fields: BTreeMap<String, CfdValue>) -> CfdValue {
    CfdValue::Object(Box::new(CfdRecord {
        key: String::new(),
        actual_type: actual_type.to_string(),
        fields,
        origin: RecordOrigin::None,
        spread_field_sources: BTreeMap::new(),
    }))
}

fn assert_record_name(store: &SessionStore, session_id: u32, expected: &str) {
    let records = store
        .get_file_records(session_id, "data/items.cfd")
        .expect("get file records");
    let row = records.records.first().expect("one row");
    let cell = row
        .fields
        .iter()
        .find(|field| field.name == "name")
        .expect("name field");
    assert_eq!(cell.value, CfdValue::String(expected.to_string()));
}

fn temp_project_dir(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("coflow-{name}-{}", unique_suffix()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean old temp dir");
    }
    root
}

fn unique_suffix() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    )
}

struct TempDirCleanup(std::path::PathBuf);

impl Drop for TempDirCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
