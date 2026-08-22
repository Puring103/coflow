#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use cfd_editor_core::editor::{CollectionEdit, SessionStore};
use coflow_runtime::{CfdPathSegment, CfdValue, RecordCoordinate};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

fn chemical_project() -> (PathBuf, PathBuf) {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("cfd-editor-chemical-workflow-{id}"));
    if root.exists() {
        fs::remove_dir_all(&root).expect("remove old test project");
    }
    let data_dir = root.join("data");
    fs::create_dir_all(&data_dir).expect("create test data directory");
    fs::write(
        root.join("coflow.yaml"),
        concat!(
            "schema: schema.cft\n",
            "data: data/\n",
            "codegen:\n",
            "  - language: csharp\n",
            "    dir: generated/csharp\n",
            "    namespace: Game.Config\n",
        ),
    )
    .expect("write project config");
    fs::write(
        root.join("schema.cft"),
        include_str!("../../../../examples/cfd/schema.cft"),
    )
    .expect("write schema");
    let data_file = data_dir.join("04-chemical-equations.cfd");
    fs::write(
        &data_file,
        include_str!("../../../../examples/cfd/data/04-chemical-equations.cfd"),
    )
    .expect("write chemical equation data");
    (root, data_file)
}

fn field(name: &str) -> CfdPathSegment {
    CfdPathSegment::Field(name.to_string())
}

#[test]
fn chemical_equation_editor_mutations_round_trip_through_reload() {
    let (root, data_file) = chemical_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load chemical equation project");
    let session_id = snapshot.session_id;
    let file_path = "data/04-chemical-equations.cfd";
    let original = RecordCoordinate::try_new("ChemicalEquation", "water_synthesis")
        .expect("valid original coordinate");

    store
        .write_field(
            session_id,
            &original,
            &[field("temperature_c")],
            &CfdValue::Float(30.0),
        )
        .expect("write scalar field");
    store
        .edit_collection(
            session_id,
            &original,
            &[field("expression"), field("inputs")],
            CollectionEdit::ArrayAppend { value: None },
        )
        .expect("append nested array item");

    let renamed = store
        .rename_record_key(session_id, &original, "water_synthesis_v2")
        .expect("rename grouped record")
        .renamed;
    let draft = store
        .make_default_object(session_id, "ChemicalEquation")
        .expect("create default chemical equation");
    store
        .insert_record(
            session_id,
            file_path,
            "empty_equation",
            "ChemicalEquation",
            draft,
        )
        .expect("insert chemical equation");
    let inserted = RecordCoordinate::try_new("ChemicalEquation", "empty_equation")
        .expect("valid inserted coordinate");
    store
        .swap_records(session_id, &renamed, &inserted)
        .expect("swap chemical equations");
    store
        .delete_record(session_id, &inserted)
        .expect("delete inserted chemical equation");

    let reloaded = store
        .reload_session(session_id)
        .expect("reload after editor mutations");
    assert!(reloaded.diagnostics.is_empty(), "{:#?}", reloaded.diagnostics);
    let records = store
        .get_file_records(session_id, file_path)
        .expect("read records after reload");
    assert_eq!(records.records.len(), 1);
    assert_eq!(records.records[0].coordinate, renamed);
    let source = fs::read_to_string(data_file).expect("read final CFD source");
    assert!(source.contains("temperature_c: 30.0"), "{source}");
}
