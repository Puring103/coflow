#![allow(clippy::expect_used, clippy::panic)]

use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

use coflow_cft::{DimensionName, FieldName, RecordKey, TypeName, VariantName};
use coflow_data_model::{CfdPathSegment, CfdValue};
use coflow_runtime::{DimensionValueCoordinate, DimensionValueState, RecordCoordinate};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rust_xlsxwriter::Workbook;

use super::SessionStore;
use crate::watcher::filter_relevant_paths;

#[test]
fn stale_reload_candidate_cannot_replace_a_newer_internal_write() {
    let root = temp_project_dir("stale-reload");
    write_project(&root, "Initial");
    let store = Arc::new(SessionStore::new().expect("create session store"));
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");

    write_project(&root, "External candidate");
    let candidate_built = Arc::new(Barrier::new(2));
    let allow_commit = Arc::new(Barrier::new(2));
    let reload_store = Arc::clone(&store);
    let reload_built = Arc::clone(&candidate_built);
    let reload_commit = Arc::clone(&allow_commit);
    let session_id = snapshot.session_id;
    let reload = std::thread::spawn(move || {
        let (entry, candidate) = reload_store
            .build_reload_candidate(session_id)
            .expect("build reload candidate");
        reload_built.wait();
        reload_commit.wait();
        SessionStore::commit_reload_candidate(session_id, &entry, candidate)
            .expect("attempt candidate commit")
            .is_none()
    });

    candidate_built.wait();
    store
        .write_field(
            session_id,
            &RecordCoordinate::try_new("Item", "sword").expect("valid record coordinate"),
            &[CfdPathSegment::Field("name".to_string())],
            &CfdValue::String("Internal write".to_string()),
        )
        .expect("commit internal write");
    allow_commit.wait();

    assert!(reload.join().expect("join reload thread"));
    let records = store
        .get_file_records(session_id, "data/items.cfd")
        .expect("read current session");
    assert_eq!(
        records.records[0].fields[0].value,
        CfdValue::String("Internal write".to_string())
    );
    std::fs::remove_dir_all(root).expect("remove temp project");
}

#[test]
fn dimension_writes_use_authoritative_expected_state() {
    let root = temp_project_dir("dimension-write");
    write_dimension_project(&root);
    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    let coordinate = DimensionValueCoordinate {
        actual_type: TypeName::new("Item").expect("type name"),
        record_key: RecordKey::new("potion").expect("record key"),
        field: FieldName::new("name").expect("field name"),
        dimension: DimensionName::new("language").expect("dimension name"),
        variant: VariantName::new("zh").expect("variant name"),
        path: Vec::new(),
    };
    let initial = DimensionValueState::Value(CfdValue::String("药水".to_string()));
    assert_eq!(
        store
            .get_dimension_value(snapshot.session_id, &coordinate)
            .expect("read dimension value")
            .state,
        initial
    );

    let updated = DimensionValueState::Value(CfdValue::String("治疗药水".to_string()));
    let outcome = store
        .write_dimension_value(snapshot.session_id, &coordinate, &initial, &updated)
        .expect("write dimension value");
    assert_eq!(outcome.old_value, initial);
    assert_eq!(outcome.new_value, updated);

    let stale = store
        .write_dimension_value(
            snapshot.session_id,
            &coordinate,
            &DimensionValueState::Missing,
            &DimensionValueState::Value(CfdValue::String("stale".to_string())),
        )
        .expect_err("stale expected state must fail");
    assert!(stale
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "MUTATION-DIMENSION-STALE"));

    let cleared = store
        .write_dimension_value(
            snapshot.session_id,
            &coordinate,
            &outcome.new_value,
            &DimensionValueState::Missing,
        )
        .expect("clear dimension value");
    assert_eq!(cleared.new_value, DimensionValueState::Missing);
    std::fs::remove_dir_all(root).expect("remove temp project");
}

#[test]
fn file_events_only_match_the_exact_committed_internal_content() {
    let root = temp_project_dir("event-attribution");
    write_project(&root, "Initial");
    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    let source = root.join("data/items.cfd");

    store
        .write_field(
            snapshot.session_id,
            &RecordCoordinate::try_new("Item", "sword").expect("valid record coordinate"),
            &[CfdPathSegment::Field("name".to_string())],
            &CfdValue::String("Internal".to_string()),
        )
        .expect("commit internal write");
    assert!(!store
        .has_external_file_changes(snapshot.session_id, std::slice::from_ref(&source))
        .expect("classify internal event"));

    std::fs::write(&source, "sword: Item { name: \"External\" }").expect("write external content");
    assert!(store
        .has_external_file_changes(snapshot.session_id, std::slice::from_ref(&source))
        .expect("classify external event"));
    std::fs::remove_dir_all(root).expect("remove temp project");
}

#[test]
fn watcher_event_batch_for_internal_write_is_not_external() {
    let root = temp_project_dir("watcher-event-attribution");
    write_project(&root, "Initial");
    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    let paths = observed_watcher_paths(&root, || {
        store
            .write_field(
                snapshot.session_id,
                &RecordCoordinate::try_new("Item", "sword").expect("valid record coordinate"),
                &[CfdPathSegment::Field("name".to_string())],
                &CfdValue::String("Internal".to_string()),
            )
            .expect("commit internal write");
    });

    assert!(
        !paths.is_empty(),
        "watcher did not observe the internal write"
    );
    assert!(
        !store
            .has_external_file_changes(snapshot.session_id, &paths)
            .expect("classify watcher batch"),
        "internal watcher batch was classified as external: {paths:?}",
    );
    std::fs::remove_dir_all(root).expect("remove temp project");
}

#[test]
fn excel_watcher_event_batch_for_internal_write_is_not_external() {
    let root = temp_project_dir("excel-watcher-event-attribution");
    write_excel_project(&root);
    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    let paths = observed_watcher_paths(&root, || {
        store
            .write_field(
                snapshot.session_id,
                &RecordCoordinate::try_new("Item", "sword").expect("valid record coordinate"),
                &[CfdPathSegment::Field("name".to_string())],
                &CfdValue::String("Internal".to_string()),
            )
            .expect("commit internal Excel write");
    });

    assert!(
        !paths.is_empty(),
        "watcher did not observe the internal Excel write"
    );
    let relevant_paths = filter_relevant_paths(&paths);
    assert!(
        !store
            .has_external_file_changes(snapshot.session_id, &relevant_paths)
            .expect("classify Excel watcher batch"),
        "internal Excel watcher batch was classified as external: {paths:?}",
    );
    std::fs::remove_dir_all(root).expect("remove temp project");
}

fn observed_watcher_paths(
    root: &std::path::Path,
    operation: impl FnOnce(),
) -> Vec<std::path::PathBuf> {
    let (sender, receiver) = mpsc::channel();
    let mut watcher = RecommendedWatcher::new(
        move |result| sender.send(result).expect("send watcher event"),
        Config::default(),
    )
    .expect("create watcher");
    watcher
        .watch(root, RecursiveMode::Recursive)
        .expect("watch project");
    operation();

    let mut paths = Vec::new();
    loop {
        match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(Ok(event)) if !matches!(event.kind, EventKind::Access(_)) => {
                paths.extend(event.paths);
            }
            Ok(Ok(_)) => {}
            Ok(Err(error)) => panic!("watcher error: {error}"),
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!("watcher disconnected"),
        }
    }
    drop(watcher);
    paths
}

fn write_excel_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data")).expect("create data directory");
    std::fs::write(root.join("schema.cft"), "type Item { name: string; }").expect("write schema");
    std::fs::write(
            root.join("coflow.yaml"),
            "schema: schema.cft\nsources:\n  - path: data/items.xlsx\n    type: excel\n    sheets:\n      - sheet: Item\n        type: Item\n        columns:\n          ID: id\n          Name: name\n",
        )
        .expect("write project configuration");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("Item").expect("name worksheet");
    sheet.write_string(0, 0, "ID").expect("write ID header");
    sheet.write_string(0, 1, "Name").expect("write name header");
    sheet.write_string(1, 0, "sword").expect("write record ID");
    sheet
        .write_string(1, 1, "Sword")
        .expect("write record name");
    workbook
        .save(root.join("data/items.xlsx"))
        .expect("write workbook");
}

fn write_project(root: &std::path::Path, name: &str) {
    std::fs::create_dir_all(root.join("data")).expect("create data directory");
    std::fs::write(root.join("schema.cft"), "type Item { name: string; }").expect("write schema");
    std::fs::write(
        root.join("data/items.cfd"),
        format!("sword: Item {{ name: \"{name}\" }}"),
    )
    .expect("write source");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\n",
    )
    .expect("write project configuration");
}

fn write_dimension_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data/dimensions/language"))
        .expect("create dimension directory");
    std::fs::write(
        root.join("schema.cft"),
        "type Item { @localized name: string; }",
    )
    .expect("write schema");
    std::fs::write(root.join("data/items.csv"), "id,name\npotion,Potion\n").expect("write records");
    std::fs::write(
        root.join("data/dimensions/language/Item_name.csv"),
        "id,default,zh\npotion,Potion,药水\n",
    )
    .expect("write dimension values");
    std::fs::write(
            root.join("coflow.yaml"),
            "schema: schema.cft\nsources:\n  - path: data/items.csv\n    type: csv\n    sheets:\n      - sheet: items\n        type: Item\ndimensions:\n  language:\n    variants: [zh]\n    out_dir: data/dimensions/language\n",
        )
        .expect("write project configuration");
}

#[test]
fn project_snapshot_uses_unique_sheet_mapping_as_type_display_name() {
    let root = temp_project_dir("type-display-name");
    write_dimension_project(&root);
    let store = SessionStore::new().expect("create session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load project");
    let option = snapshot
        .file_types
        .get("data/items.csv")
        .and_then(|options| options.first())
        .expect("file type option");
    assert_eq!(option.name, "Item");
    assert_eq!(option.display_name, "items");
    assert_eq!(option.record_count, 1);
    std::fs::remove_dir_all(root).expect("remove temp project");
}

fn temp_project_dir(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "coflow-editor-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    ))
}
