#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use cfd_editor_core::editor::{CollectionEdit, LanguagePosition, SessionStore};
use coflow_runtime::{CfdPathSegment, CfdValue, RecordCoordinate};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

fn array_project() -> (PathBuf, PathBuf) {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("cfd-editor-array-workflow-{id}"));
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
        include_str!("../../../../examples/showcase/schema/05-arrays.cft"),
    )
    .expect("write schema");
    let data_file = data_dir.join("05-arrays.cfd");
    fs::write(
        &data_file,
        include_str!("../../../../examples/showcase/data/05-arrays.cfd"),
    )
    .expect("write array data");
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

fn function_defaults_project() -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("cfd-editor-function-defaults-{id}"));
    if root.exists() {
        fs::remove_dir_all(&root).expect("remove old function defaults project");
    }
    fs::create_dir_all(root.join("data")).expect("create function defaults data directory");
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
    .expect("write function defaults config");
    fs::write(
        root.join("schema.cft"),
        concat!(
            "type Rule {\n",
            "  name: string = \"base\";\n",
            "  label: string = \"rule {name}\";\n",
            "  apply: fn(value: int) -> int = fn(input: int) -> int {\n",
            "    var total = input + 1;\n",
            "    total\n",
            "  };\n",
            "}\n",
        ),
    )
    .expect("write function defaults schema");
    fs::write(root.join("data/empty.cfd"), "").expect("write empty data file");
    root
}

fn field(name: &str) -> CfdPathSegment {
    CfdPathSegment::Field(name.to_string())
}

#[test]
fn repository_examples_open_in_editor() {
    let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");

    for example in ["showcase"] {
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

        let expected_root = config.parent().unwrap().canonicalize().unwrap();
        assert_eq!(
            snapshot.project_root.replace("\\\\?\\", ""),
            expected_root.display().to_string().replace("\\\\?\\", "")
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
    assert!(source.contains("note: \"new\""), "{source}");
    let empty_source = fs::read_to_string(root.join("data/empty.cfd"))
        .expect("read formerly empty source");
    assert!(empty_source.contains("new_reward: ItemReward"), "{empty_source}");
}

#[test]
fn array_editor_mutations_round_trip_through_reload() {
    let (root, data_file) = array_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load array project");
    let session_id = snapshot.session_id;
    let file_path = "data/05-arrays.cfd";
    let original = RecordCoordinate::try_new("ArrayExample", "featured_tags")
        .expect("valid original coordinate");
    store
        .edit_collection(
            session_id,
            &original,
            &[field("tags")],
            CollectionEdit::ArrayAppend { value: None },
        )
        .expect("append array item");

    let renamed = store
        .rename_record_key(session_id, &original, "featured_tags_v2")
        .expect("rename grouped record")
        .renamed;
    let draft = store
        .make_default_object(session_id, "ArrayExample")
        .expect("create default array record");
    store
        .insert_record(
            session_id,
            file_path,
            "empty_array",
            "ArrayExample",
            draft,
        )
        .expect("insert array record");
    let inserted = RecordCoordinate::try_new("ArrayExample", "empty_array")
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
    assert!(source.contains("\"recommended\", \"recommended\""), "{source}");
}

#[test]
fn source_text_edit_validates_before_atomic_save() {
    let (root, data_file) = array_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load source editing project");
    let session_id = snapshot.session_id;
    let file_path = "data/05-arrays.cfd";
    let original = store
        .read_source_text(session_id, file_path)
        .expect("read source text");

    let invalid = "broken: ArrayExample {\n    tags: 12,\n}\n";
    let diagnostics = store
        .validate_source_text(session_id, file_path, invalid)
        .expect("validate invalid draft");
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.severity == "error"),
        "{diagnostics:#?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.range.is_some_and(|range| range.start.line == 1)),
        "{diagnostics:#?}"
    );
    store
        .write_source_text(session_id, file_path, invalid)
        .expect_err("invalid source must not be written");
    assert_eq!(
        fs::read_to_string(&data_file).expect("read unchanged source"),
        original
    );

    let edited = format!("{original}\n");
    let saved = store
        .write_source_text(session_id, file_path, &edited)
        .expect("save valid source text");
    assert!(saved.revision > snapshot.revision);
    assert_eq!(
        store
            .read_source_text(session_id, file_path)
            .expect("read saved source"),
        edited
    );
}

#[test]
fn editor_language_features_are_served_by_embedded_lsp() {
    let (root, _) = array_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load language-service project");
    let file_path = "data/05-arrays.cfd";
    let source = store
        .read_source_text(snapshot.session_id, file_path)
        .expect("read source text");

    let document = store
        .sync_language_document(snapshot.session_id, file_path, &source, 1)
        .expect("synchronize through embedded LSP");
    assert!(document.diagnostics.is_empty(), "{:#?}", document.diagnostics);
    assert!(document.syntax_valid);
    assert!(!document.semantic_token_data.is_empty());
    assert!(document.semantic_token_types.iter().any(|kind| kind == "type"));

    let unformatted = source.replace("  tags:", "tags:");
    let formatted = store
        .format_language_document(snapshot.session_id, file_path, &unformatted, 2)
        .expect("format CFD through embedded LSP");
    assert!(formatted.text.contains("  tags:"), "{}", formatted.text);
    assert!(!formatted.edits.is_empty());

    let completions = store
        .complete_language_document(
            snapshot.session_id,
            file_path,
            &source,
            3,
            &LanguagePosition { line: 0, character: 0 },
        )
        .expect("complete through embedded LSP");
    let record_completion = completions
        .iter()
        .find(|item| item.label == "ArrayExample")
        .expect("record completion");
    assert_eq!(record_completion.insert_text_format, Some(2));
    assert!(record_completion
        .insert_text
        .as_deref()
        .is_some_and(|text| text.contains("${1:key}")));

    let function = store
        .function_document(
            snapshot.session_id,
            "fn(value: int) -> int { value + 1 }",
            None,
        )
        .expect("analyze function virtual document through embedded LSP");
    assert_eq!(function.body, "value + 1");
    assert!(function.completions.iter().any(|item| item.label == "value"));
    assert!(!function.semantic_token_data.is_empty());

    let invalid = "broken: ArrayExample {";
    let first_invalid = store
        .sync_language_document(snapshot.session_id, file_path, invalid, 4)
        .expect("publish invalid document diagnostics");
    let repeated_invalid = store
        .sync_language_document(snapshot.session_id, file_path, invalid, 4)
        .expect("reuse diagnostics for an unchanged LSP version");
    assert!(!first_invalid.diagnostics.is_empty());
    assert!(!first_invalid.syntax_valid);
    assert_eq!(
        repeated_invalid.diagnostics.len(),
        first_invalid.diagnostics.len()
    );
    let invalid_formatting = store
        .format_language_document(snapshot.session_id, file_path, invalid, 5)
        .expect("ignore formatting for invalid CFD");
    assert_eq!(invalid_formatting.text, invalid);
    assert!(invalid_formatting.edits.is_empty());
}

#[test]
fn cft_function_defaults_are_supported_through_the_editor_backend() {
    let root = function_defaults_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load function defaults project");
    let source = store
        .read_source_text(snapshot.session_id, "schema.cft")
        .expect("read function defaults schema");

    let document = store
        .sync_language_document(snapshot.session_id, "schema.cft", &source, 1)
        .expect("synchronize CFT defaults through embedded LSP");
    assert!(document.diagnostics.is_empty(), "{:#?}", document.diagnostics);
    assert!(document.syntax_valid);
    assert!(!document.semantic_token_data.is_empty());

    let completions = store
        .complete_language_document(
            snapshot.session_id,
            "schema.cft",
            &source,
            2,
            &LanguagePosition {
                line: 5,
                character: 4,
            },
        )
        .expect("complete inside CFT default function");
    assert!(completions.iter().any(|item| item.label == "input"));
    assert!(completions.iter().any(|item| item.label == "total"));
    assert!(completions.iter().any(|item| item.label == "var"));

    let unformatted = source.replace("    var total = input + 1;", "var total=input+1;");
    let formatted = store
        .format_language_document(snapshot.session_id, "schema.cft", &unformatted, 3)
        .expect("format CFT default function");
    assert!(formatted.text.contains("    var total = input + 1;"));
    let idempotent = store
        .format_language_document(snapshot.session_id, "schema.cft", &formatted.text, 4)
        .expect("format canonical CFT default function");
    assert!(idempotent.edits.is_empty());

    let value = store
        .make_default_object(snapshot.session_id, "Rule")
        .expect("materialize CFT defaults");
    let CfdValue::Object(rule) = value else {
        panic!("expected Rule object");
    };
    assert!(matches!(rule.field("label"), Some(CfdValue::FormattedString(_))));
    assert!(matches!(rule.field("apply"), Some(CfdValue::Function(_))));
}

#[test]
fn cft_source_is_visible_editable_and_validated() {
    let (root, _) = array_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.join("coflow.yaml"))
        .expect("load schema editing project");
    let file_path = "schema.cft";
    let source = store
        .read_source_text(snapshot.session_id, file_path)
        .expect("read configured CFT source");
    assert!(source.contains("type ArrayExample"));

    let invalid = source.replace("[string]", "MissingType");
    let diagnostics = store
        .validate_source_text(snapshot.session_id, file_path, &invalid)
        .expect("validate invalid CFT draft");
    assert!(diagnostics.iter().any(|item| item.severity == "error"));
    store
        .write_source_text(snapshot.session_id, file_path, &invalid)
        .expect_err("invalid CFT must not be written");

    let unformatted = source.replace("  tags:", "tags:");
    let formatted = store
        .format_language_document(snapshot.session_id, file_path, &unformatted, 1)
        .expect("format CFT through embedded LSP");
    assert!(formatted.text.contains("  tags:"), "{}", formatted.text);
    assert!(!formatted.edits.is_empty());
    store
        .write_source_text(snapshot.session_id, file_path, &formatted.text)
        .expect("save valid CFT source");

    let invalid_syntax = source.replacen("type ArrayExample {", "type ArrayExample", 1);
    let invalid_formatting = store
        .format_language_document(snapshot.session_id, file_path, &invalid_syntax, 2)
        .expect("ignore formatting for invalid CFT");
    assert_eq!(invalid_formatting.text, invalid_syntax);
    assert!(invalid_formatting.edits.is_empty());
}
