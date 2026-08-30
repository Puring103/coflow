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

fn inheritance_project() -> (PathBuf, PathBuf) {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("cfd-editor-inheritance-workflow-{id}"));
    if root.exists() {
        fs::remove_dir_all(&root).expect("remove old inheritance test project");
    }
    let data_dir = root.join("data");
    fs::create_dir_all(&data_dir).expect("create inheritance data directory");
    fs::write(
        root.join("coflow.yaml"),
        concat!(
            "schema: schema.cft\n",
            "data: data/\n",
            "codegen:\n",
            "  - language: csharp\n",
            "    dir: generated/csharp\n",
            "    namespace: Test.Config\n",
        ),
    )
    .expect("write inheritance project config");
    fs::write(
        root.join("schema.cft"),
        concat!(
            "abstract type Reward { label: string; }\n",
            "type ItemReward : Reward { count: int; tags: [string]; }\n",
            "type CurrencyReward : Reward { amount: int; }\n",
            "type Holder { reward: Option<Reward>; outcome: Result<ItemReward, CurrencyReward>; note: Option<string>; }\n",
        ),
    )
    .expect("write inheritance schema");
    let populated = data_dir.join("rewards.cfd");
    fs::write(
        &populated,
        concat!(
            "holder: Holder {\n",
            "    reward: Some(ItemReward{\n",
            "        label: \"starter\",\n",
            "        count: 1,\n",
            "        tags: [],\n",
            "    }),\n",
            "    note: Some(\"old\"),\n",
            "    outcome: Ok(ItemReward{\n",
            "        label: \"result\",\n",
            "        count: 3,\n",
            "        tags: [],\n",
            "    }),\n",
            "}\n",
        ),
    )
    .expect("write inheritance data");
    fs::write(data_dir.join("empty.cfd"), "").expect("write empty data file");
    (root, populated)
}

fn field(name: &str) -> CfdPathSegment {
    CfdPathSegment::Field(name.to_string())
}

#[test]
fn repository_examples_open_in_editor() {
    let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");

    for example in ["card_game", "cfd", "cft", "csharp-runtime"] {
        let config = repository_root
            .join("examples")
            .join(example)
            .join("coflow.yaml");
        let store = SessionStore::new().expect("create editor session store");
        let snapshot = store.load_project(&config).unwrap_or_else(|error| {
            panic!(
                "editor failed to open repository example `{example}` at {}: {error}",
                config.display()
            )
        });

        assert_eq!(
            PathBuf::from(snapshot.project_root),
            config.parent().unwrap().canonicalize().unwrap()
        );
        assert!(
            snapshot
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity != "error"),
            "editor reported errors for repository example `{example}`: {:#?}",
            snapshot.diagnostics
        );
    }
}

#[test]
fn inherited_and_optional_polymorphic_values_edit_end_to_end() {
    let (root, populated) = inheritance_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load inheritance project");
    let session_id = snapshot.session_id;

    let empty_types = snapshot
        .file_types
        .get("data/empty.cfd")
        .expect("empty file type navigation");
    assert!(empty_types.iter().any(|ty| ty.name == "ItemReward"));
    assert!(empty_types.iter().any(|ty| ty.name == "CurrencyReward"));
    assert!(!empty_types.iter().any(|ty| ty.name == "Reward"));

    let draft = store
        .create_record_draft(session_id, "ItemReward")
        .expect("create concrete child draft");
    assert_eq!(
        draft.fields.iter().map(|field| field.name.as_str()).collect::<Vec<_>>(),
        ["label", "count", "tags"]
    );
    let new_reward = store
        .make_default_object(session_id, "ItemReward")
        .expect("materialize concrete child defaults");
    store
        .insert_record(
            session_id,
            "data/empty.cfd",
            "new_reward",
            "ItemReward",
            new_reward,
        )
        .expect("insert concrete child into empty file");

    let records = store
        .get_file_records(session_id, "data/rewards.cfd")
        .expect("read polymorphic records");
    let reward = records.records[0]
        .fields
        .iter()
        .find(|field| field.name == "reward")
        .expect("reward field");
    let annotation = reward.annotation.as_ref().expect("reward annotation");
    assert!(annotation.nullable);
    let mut polymorphic_types = annotation.polymorphic_types.clone();
    polymorphic_types.sort();
    assert_eq!(polymorphic_types, ["CurrencyReward", "ItemReward"]);

    let holder = RecordCoordinate::try_new("Holder", "holder").expect("holder coordinate");
    store
        .write_field(
            session_id,
            &holder,
            &[field("reward"), field("label")],
            &CfdValue::String("updated".to_string()),
        )
        .expect("write inherited field through Option<AbstractType>");
    store
        .write_field(
            session_id,
            &holder,
            &[field("reward"), field("count")],
            &CfdValue::Int(2),
        )
        .expect("write child field through Option<AbstractType>");
    store
        .write_field(
            session_id,
            &holder,
            &[field("outcome"), field("count")],
            &CfdValue::Int(4),
        )
        .expect("write child field through Result branch");
    store
        .edit_collection(
            session_id,
            &holder,
            &[field("reward"), field("tags")],
            CollectionEdit::ArrayAppend { value: None },
        )
        .expect("append first item to child collection through Option<AbstractType>");
    store
        .write_field(
            session_id,
            &holder,
            &[field("note")],
            &CfdValue::String("new".to_string()),
        )
        .expect("wrap a bare editor value for an Option field");

    let reloaded = store
        .reload_session(session_id)
        .expect("reload inheritance project");
    assert!(reloaded.diagnostics.is_empty(), "{:#?}", reloaded.diagnostics);
    let source = fs::read_to_string(populated).expect("read inheritance source");
    assert!(source.contains("label: \"updated\""), "{source}");
    assert!(source.contains("count: 2"), "{source}");
    assert!(source.contains("count: 4"), "{source}");
    assert!(source.contains("tags: [\"\"]"), "{source}");
    assert!(source.contains("note: Some(\"new\")"), "{source}");
    let empty_source = fs::read_to_string(root.join("data/empty.cfd"))
        .expect("read formerly empty source");
    assert!(empty_source.contains("new_reward: ItemReward"), "{empty_source}");
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
