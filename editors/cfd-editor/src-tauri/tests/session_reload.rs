#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

use cfd_editor_lib::editor::SessionStore;
use coflow_data_model::CfdValue;

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
fn file_records_include_ref_inline_field_mode_annotations() {
    let root = temp_project_dir("cfd-editor-field-mode");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item { name: string; }
            type Holder {
                @ref
                item_ref: Item;
                @inline
                item_inline: Item;
                nested: Nested;
            }
            type Nested {
                @ref
                nested_ref: Item;
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
        item_ref.annotation.as_ref().and_then(|a| a.field_mode),
        Some(cfd_editor_lib::editor::types::FieldMode::Ref)
    );
    assert_eq!(
        item_inline.annotation.as_ref().and_then(|a| a.field_mode),
        Some(cfd_editor_lib::editor::types::FieldMode::Inline)
    );
    assert_eq!(
        records
            .field_modes
            .get("Nested")
            .and_then(|fields| fields.get("nested_ref"))
            .copied(),
        Some(cfd_editor_lib::editor::types::FieldMode::Ref)
    );
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
