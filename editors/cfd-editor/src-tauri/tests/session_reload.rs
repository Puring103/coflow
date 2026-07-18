#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

use cfd_editor_lib::editor::{
    BatchWriteFieldInput, CollectionEdit, CreateFieldSource, CreateRequiredInput, SessionStore,
};
use coflow_data_model::{CfdObject, CfdPathSegment, CfdValue};
use coflow_runtime::RecordCoordinate;
use std::collections::BTreeMap;

#[test]
fn batch_field_write_updates_records_in_one_revision() {
    let root = temp_project_dir("cfd-editor-batch-write");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root, "Sword");
    std::fs::write(
        root.join("data/items.cfd"),
        r#"sword: Item { name: "Sword" }
shield: Item { name: "Shield" }"#,
    )
    .expect("write batch data");
    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    let path = vec![CfdPathSegment::Field("name".to_string())];

    let outcome = store
        .write_fields(
            snapshot.session_id,
            &[
                BatchWriteFieldInput {
                    coordinate: RecordCoordinate::new("Item", "sword"),
                    field_path: path.clone(),
                    new_value: CfdValue::String("Shared".to_string()),
                },
                BatchWriteFieldInput {
                    coordinate: RecordCoordinate::new("Item", "shield"),
                    field_path: path,
                    new_value: CfdValue::String("Shared".to_string()),
                },
            ],
        )
        .expect("batch write fields");

    assert_eq!(outcome.revision, snapshot.revision + 1);
    assert_eq!(outcome.edits.len(), 2);
    assert_eq!(
        outcome
            .edits
            .iter()
            .map(|edit| edit.old_value.clone())
            .collect::<Vec<_>>(),
        vec![
            Some(CfdValue::String("Sword".to_string())),
            Some(CfdValue::String("Shield".to_string())),
        ]
    );
    let records = store
        .get_file_records(snapshot.session_id, "data/items.cfd")
        .expect("read records after batch");
    assert!(records.records.iter().all(|record| {
        record.fields.iter().any(|field| {
            field.name == "name" && field.value == CfdValue::String("Shared".to_string())
        })
    }));
}

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
    let data_node = snapshot
        .file_tree
        .iter()
        .find(|node| node.path == "data")
        .expect("data directory node");
    assert_eq!(
        data_node.first_source_descendant.as_deref(),
        Some("data/items.cfd")
    );
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
fn create_record_draft_surfaces_required_ref_fields_for_insert_form() {
    let root = temp_project_dir("cfd-editor-create-draft-required-ref");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item { name: string; }
            type Holder {
                item: &Item;
                note: string;
                tags: [string] = [];
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
    let draft = store
        .create_record_draft(snapshot.session_id, "Holder")
        .expect("create record draft");

    let item = draft
        .fields
        .iter()
        .find(|field| field.name == "item")
        .expect("item field");
    assert!(matches!(item.source, CreateFieldSource::RequiredInput));
    assert!(matches!(
        item.required.as_ref(),
        Some(CreateRequiredInput::Ref { target_type }) if target_type == "Item"
    ));
    assert_eq!(
        item.annotation
            .as_ref()
            .and_then(|annotation| annotation.ref_target_type.as_deref()),
        Some("Item")
    );

    let note = draft
        .fields
        .iter()
        .find(|field| field.name == "note")
        .expect("note field");
    assert!(matches!(note.source, CreateFieldSource::TypeSeed));
    assert_eq!(note.value, Some(CfdValue::String(String::new())));

    let tags = draft
        .fields
        .iter()
        .find(|field| field.name == "tags")
        .expect("tags field");
    assert!(matches!(tags.source, CreateFieldSource::SchemaDefault));
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
fn dimension_sources_do_not_expose_synthetic_editor_records() {
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
    assert!(
        records.records.is_empty(),
        "managed dimension files must not appear as synthetic record rows"
    );
    let dimension_records = store
        .get_dimension_file_records(
            snapshot.session_id,
            "data/dimensions/language/Item_name.csv",
        )
        .expect("get dimension file records");
    assert_eq!(dimension_records.field, "name");
    assert_eq!(dimension_records.variants, ["zh", "en"]);
    assert_eq!(dimension_records.rows.len(), 1);
    assert_eq!(dimension_records.rows[0].coordinate.key, "potion");
    assert_eq!(
        dimension_records.rows[0].default_value,
        CfdValue::String("Potion".to_string())
    );
    assert!(matches!(
        dimension_records.rows[0].values.get("zh"),
        Some(coflow_runtime::DimensionValueState::Value(
            CfdValue::String(value)
        )) if value == "药水"
    ));
}

#[test]
fn spread_write_reports_source_and_matches_full_check_diagnostics() {
    let root = temp_project_dir("cfd-editor-spread-write-outcome");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r#"
            type Item {
                name: string;
                power: int;
                check { name == "Base"; }
            }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("data/source.cfd"),
        r#"base: Item { name: "Base", power: 1 }"#,
    )
    .expect("write source");
    std::fs::write(root.join("data/host.cfd"), r"child: Item { ...&base }").expect("write host");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data/source.cfd\n  - path: data/host.cfd\n",
    )
    .expect("write config");

    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    let outcome = store
        .write_field(
            snapshot.session_id,
            &RecordCoordinate::new("Item", "child"),
            &[CfdPathSegment::Field("name".to_string())],
            &CfdValue::String("Changed".to_string()),
        )
        .expect("write spread field");

    assert_eq!(
        outcome.old_value,
        Some(CfdValue::String("Base".to_string()))
    );
    assert_eq!(
        outcome.new_value,
        Some(CfdValue::String("Changed".to_string()))
    );
    assert!(
        outcome
            .affected_files
            .iter()
            .any(|file| file == "data/source.cfd"),
        "affected files should include spread source file: {:?}",
        outcome.affected_files
    );
    let mut incremental_check_diagnostics = outcome
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.stage == "CHECK")
        .cloned()
        .collect::<Vec<_>>();
    let canonical_root = std::fs::canonicalize(&root).expect("canonicalize spread project root");
    for diagnostic in &mut incremental_check_diagnostics {
        if let Some(path) = diagnostic.file_path.as_deref() {
            diagnostic.file_path = Some(
                std::fs::canonicalize(path)
                    .expect("canonicalize incremental diagnostic path")
                    .strip_prefix(&canonical_root)
                    .expect("incremental diagnostic path under project root")
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    let incremental_check_records = incremental_check_diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.record_key.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        incremental_check_records,
        std::collections::BTreeSet::from(["base".to_string(), "child".to_string()])
    );

    let full = store
        .reload_session(snapshot.session_id)
        .expect("reload spread project");
    let full_check_diagnostics = full
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.stage == "CHECK")
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(incremental_check_diagnostics, full_check_diagnostics);
}

#[test]
fn edit_collection_appends_schema_default_item() {
    let root = temp_project_dir("cfd-editor-collection-edit");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Bag {
                nums: [int] = [];
            }
        ",
    )
    .expect("write schema");
    std::fs::write(root.join("data/items.cfd"), r"bag: Bag { nums: [] }").expect("write data");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data/items.cfd\n",
    )
    .expect("write config");

    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    let outcome = store
        .edit_collection(
            snapshot.session_id,
            &RecordCoordinate::new("Bag", "bag"),
            &[CfdPathSegment::Field("nums".to_string())],
            CollectionEdit::ArrayAppend { value: None },
        )
        .expect("append array item");
    let nums = outcome
        .row
        .fields
        .iter()
        .find(|field| field.name == "nums")
        .expect("nums field");
    assert_eq!(nums.value, CfdValue::Array(vec![CfdValue::Int(0)]));
    assert_eq!(outcome.old_value, Some(CfdValue::Array(Vec::new())));
    assert_eq!(
        outcome.new_value,
        Some(CfdValue::Array(vec![CfdValue::Int(0)]))
    );
}

#[test]
fn edit_collection_appends_by_copying_last_item_before_schema_seed() {
    let root = temp_project_dir("cfd-editor-collection-copy-last");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Bag {
                nums: [int] = [];
            }
        ",
    )
    .expect("write schema");
    std::fs::write(root.join("data/items.cfd"), r"bag: Bag { nums: [7] }").expect("write data");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data/items.cfd\n",
    )
    .expect("write config");

    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    let outcome = store
        .edit_collection(
            snapshot.session_id,
            &RecordCoordinate::new("Bag", "bag"),
            &[CfdPathSegment::Field("nums".to_string())],
            CollectionEdit::ArrayAppend { value: None },
        )
        .expect("append array item");
    let nums = outcome
        .row
        .fields
        .iter()
        .find(|field| field.name == "nums")
        .expect("nums field");
    assert_eq!(
        nums.value,
        CfdValue::Array(vec![CfdValue::Int(7), CfdValue::Int(7)])
    );
}

#[test]
fn row_diagnostics_are_precomputed_by_backend() {
    let root = temp_project_dir("cfd-editor-row-diagnostics");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item {
                value: int;
                check { value > 0; }
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data/items.cfd"),
        r"
            sword: Item { value: -1 }
        ",
    )
    .expect("write data");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data/items.cfd\n",
    )
    .expect("write config");

    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    assert!(
        snapshot.diagnostics.iter().any(|diagnostic| {
            diagnostic.record_key.as_deref() == Some("sword")
                && diagnostic.field_path.as_deref() == Some("value")
        }),
        "project should surface the check diagnostic"
    );
    let records = store
        .get_file_records(snapshot.session_id, "data/items.cfd")
        .expect("load file records");
    let item = records
        .records
        .iter()
        .find(|row| row.coordinate.actual_type == "Item" && row.coordinate.key == "sword")
        .expect("item row");

    assert!(item.diagnostic_severity.is_some());
    assert!(
        item.field_diagnostics
            .iter()
            .any(|diagnostic| diagnostic.field_path == "value"),
        "row should carry field diagnostics for frontend rendering: {:?}",
        item.field_diagnostics
    );
}

#[test]
fn file_records_follow_schema_field_definition_order() {
    let root = temp_project_dir("cfd-editor-field-order");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item {
                zulu: string;
                alpha: int;
                middle: bool;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data/items.cfd"),
        r#"sword: Item { zulu: "Z", alpha: 1, middle: true }"#,
    )
    .expect("write data");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data/items.cfd\n",
    )
    .expect("write config");

    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    let records = store
        .get_file_records(snapshot.session_id, "data/items.cfd")
        .expect("load file records");

    assert_eq!(
        records.records[0]
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["zulu", "alpha", "middle"]
    );
    assert_eq!(
        records
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["zulu", "alpha", "middle"]
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

    assert!(
        snapshot.diagnostics.is_empty(),
        "{:?}",
        snapshot.diagnostics
    );
    assert!(
        !root.join("data/dimensions/language/Item_name.csv").exists(),
        "editor project load should not generate dimension files"
    );
}

#[test]
fn localized_write_reports_generated_dimension_source_as_affected() {
    let root = temp_project_dir("cfd-editor-dim-affected-files");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
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
    std::fs::write(root.join("data/items.csv"), "id,name\npotion,Potion\n").expect("write items");
    std::fs::write(
        root.join("data/dimensions/language/Item_name.csv"),
        "id,default,zh,en\npotion,Potion,药水,Potion\n",
    )
    .expect("write dimensions");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/main.cft
sources:
  - path: data/items.csv
    type: csv
    sheets:
      - sheet: items
        type: Item
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
    let outcome = store
        .write_field(
            snapshot.session_id,
            &RecordCoordinate::new("Item", "potion"),
            &[CfdPathSegment::Field("name".to_string())],
            &CfdValue::String("Elixir".to_string()),
        )
        .expect("write localized field");

    assert_eq!(
        outcome.affected_files,
        vec![
            "data/dimensions/language/Item_name.csv".to_string(),
            "data/items.csv".to_string(),
        ]
    );
    assert!(
        std::fs::read_to_string(root.join("data/dimensions/language/Item_name.csv"))
            .expect("read generated dimension source")
            .contains("potion,Elixir,药水,Potion")
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
