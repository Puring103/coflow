#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

use cfd_editor_lib::editor::SessionStore;
use coflow_data_model::{CfdObject, CfdValue};
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
    let (store, _cleanup, session_id) = load_ref_metadata_project();
    let holder = holder_row(&store, session_id);
    let item_ref = holder_field(&holder, "item_ref");
    let item_inline = holder_field(&holder, "item_inline");
    let nested = holder_field(&holder, "nested");

    assert_eq!(
        item_ref
            .annotation
            .as_ref()
            .and_then(|annotation| annotation.ref_target_file.as_deref()),
        Some("data/items.cfd")
    );
    assert_eq!(
        item_ref
            .annotation
            .as_ref()
            .and_then(|annotation| annotation.declared_type.as_deref()),
        Some("&Item")
    );
    assert_eq!(
        item_ref
            .annotation
            .as_ref()
            .and_then(|annotation| annotation.ref_target_type.as_deref()),
        Some("Item")
    );
    assert!(
        item_inline
            .annotation
            .as_ref()
            .is_some_and(|annotation| annotation.declared_type.as_deref() == Some("Item")),
        "inline fields should carry schema display type annotations"
    );
    let nested_ref_annotation = nested
        .annotation
        .as_ref()
        .and_then(|annotation| annotation.children.get("nested_ref"))
        .expect("nested ref annotation");
    assert_eq!(
        nested_ref_annotation.declared_type.as_deref(),
        Some("&Item")
    );
    assert_eq!(
        nested_ref_annotation.ref_target_type.as_deref(),
        Some("Item")
    );
    assert_eq!(
        nested_ref_annotation.ref_target_file.as_deref(),
        Some("data/items.cfd")
    );
}

#[test]
fn file_records_include_collection_declared_type_metadata() {
    let (store, _cleanup, session_id) = load_ref_metadata_project();
    let holder = holder_row(&store, session_id);
    let item_refs = holder_field(&holder, "item_refs");

    assert_eq!(
        item_refs
            .annotation
            .as_ref()
            .and_then(|annotation| annotation.declared_type.as_deref()),
        Some("[&Item]")
    );
    assert_eq!(
        item_refs
            .annotation
            .as_ref()
            .and_then(|annotation| annotation.ref_target_type.as_deref()),
        None
    );
    let item_template = item_refs
        .annotation
        .as_ref()
        .and_then(|annotation| annotation.item_annotation.as_deref())
        .expect("collection element template");
    assert_eq!(
        item_template.declared_type.as_deref(),
        Some("&Item"),
        "collection annotator should include element declared_type so the editor doesn't parse strings",
    );
    assert_eq!(
        item_template.ref_target_type.as_deref(),
        Some("Item"),
        "collection element template should surface the ref target type",
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

#[test]
fn graph_includes_table_array_reference_edges() {
    let root = temp_project_dir("cfd-editor-table-array-graph");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            type MyEvent {
                yesRes: [&MyEvent] = [];
                noRes: [&MyEvent] = [];
                content: string = "";
            }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("Events.csv"),
        "\
id,yesRes,noRes,content
root,&yes,&missing,Root
yes,,,Yes
missing,,,No
",
    )
    .expect("write csv");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema:\n  - schema/main.cft\nsources:\n  - path: data/Events.csv\n    type: csv\n    sheets:\n      - sheet: Events\n        type: MyEvent\n        key: id\n",
    )
    .expect("write config");

    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    let graph = store
        .get_graph(
            snapshot.session_id,
            &cfd_editor_lib::editor::GraphQuery {
                file_path: "data/Events.csv".to_string(),
                active_type: Some("MyEvent".to_string()),
                enabled_fields: None,
                depth: Some(3),
                limit: Some(100),
            },
        )
        .expect("get graph");

    assert_eq!(graph.available_fields, vec!["noRes", "yesRes"]);
    assert!(graph.edges.iter().any(|edge| {
        edge.source.key == "root" && edge.target.key == "yes" && edge.field_path == "yesRes[0]"
    }));
    assert!(graph.edges.iter().any(|edge| {
        edge.source.key == "root" && edge.target.key == "missing" && edge.field_path == "noRes[0]"
    }));
}

#[test]
fn dimension_synth_default_field_carries_read_only_annotation() {
    let root = temp_project_dir("cfd-editor-dim-read-only");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::create_dir_all(root.join("data/dimensions/language")).expect("create dimensions dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r"
            type Item {
                @localized
                name: string;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"potion: Item { name: "Potion" }"#,
    )
    .expect("write items");
    std::fs::write(
        root.join("data/dimensions/language").join("Item_name.csv"),
        "id,default,zh,en\npotion,Potion,药水,Potion\n",
    )
    .expect("write dimension csv");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/main.cft
sources:
  - path: data/items.cfd
dimensions:
  language:
    variants: [zh, en]
    out_dir: data/dimensions/language
",
    )
    .expect("write config");

    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    let records = store
        .get_file_records(
            snapshot.session_id,
            "data/dimensions/language/Item_name.csv",
        )
        .expect("get dimension records");
    let synth = records
        .records
        .iter()
        .find(|row| row.coordinate.actual_type == "Item_nameVariants")
        .expect("synth row");
    let default_field = synth
        .fields
        .iter()
        .find(|field| field.name == "default")
        .expect("default field");
    let default_annotation = default_field
        .annotation
        .as_ref()
        .expect("default field annotation");
    assert!(
        default_annotation.read_only,
        "dimension-synth `default` field should be flagged read-only so the editor \
         steers writes to the source record; got annotation: {default_annotation:?}",
    );
    let variant_field = synth
        .fields
        .iter()
        .find(|field| field.name == "zh")
        .expect("zh field");
    assert!(
        !variant_field
            .annotation
            .as_ref()
            .is_some_and(|annotation| annotation.read_only),
        "variant slots stay editable",
    );
}

#[test]
fn load_project_does_not_generate_missing_dimension_sources() {
    let root = temp_project_dir("cfd-editor-dim-no-generate");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r"
            type Item {
                @localized
                name: string;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"potion: Item { name: "Potion" }"#,
    )
    .expect("write items");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/main.cft
sources:
  - path: data/items.cfd
dimensions:
  language:
    variants: [zh, en]
    out_dir: data/dimensions/language
",
    )
    .expect("write config");

    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");

    assert!(snapshot.diagnostics.is_empty(), "{:?}", snapshot.diagnostics);
    assert!(
        !root.join("data/dimensions/language/Item_name.csv").exists(),
        "editor project load should not generate dimension files"
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

fn object_value(actual_type: &str, fields: BTreeMap<String, CfdValue>) -> CfdValue {
    CfdValue::Object(Box::new(CfdObject::new(actual_type, fields)))
}

fn load_ref_metadata_project() -> (SessionStore, TempDirCleanup, u32) {
    let root = temp_project_dir("cfd-editor-field-mode");
    let cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item { name: string; }
            type Holder {
                item_ref: &Item;
                item_refs: [&Item] = [];
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
              item_refs: [&sword],
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
    (store, cleanup, snapshot.session_id)
}

fn holder_row(store: &SessionStore, session_id: u32) -> cfd_editor_lib::editor::types::RecordRow {
    let records = store
        .get_file_records(session_id, "data/items.cfd")
        .expect("get file records");
    records
        .records
        .into_iter()
        .find(|row| row.coordinate.actual_type == "Holder")
        .expect("holder row")
}

fn holder_field<'a>(
    holder: &'a cfd_editor_lib::editor::types::RecordRow,
    field_name: &str,
) -> &'a cfd_editor_lib::editor::types::FieldCell {
    holder
        .fields
        .iter()
        .find(|field| field.name == field_name)
        .expect("holder field")
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
