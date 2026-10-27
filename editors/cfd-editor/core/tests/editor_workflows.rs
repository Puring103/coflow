// 测试文本中的 `${1:name}` 用于验证 LSP snippet 协议。
#![allow(
    clippy::expect_used,
    clippy::literal_string_with_formatting_args,
    clippy::panic,
    clippy::unwrap_used
)]

use cfd_editor_core::editor::{
    BatchWriteFieldInput, CollectionEdit, LanguagePosition, SessionStore,
};
use coflow_runtime::{CfdObject, CfdPathSegment, CfdValue, RecordCoordinate};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn project_directory(prefix: &str) -> TempDir {
    // 目录由操作系统原子创建并随机隔离，避免并行测试进程互删夹具或覆盖待提交源码。
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .expect("create isolated editor project")
}

#[test]
fn independent_sessions_preserve_concurrent_settings_updates() {
    let root = project_directory("settings-concurrent-");
    fs::write(
        root.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data.cfd\ncodegen:\n  - language: csharp\n    dir: generated/\n",
    )
    .unwrap();
    fs::write(root.path().join("schema.cft"), "table Item { value: int; }").unwrap();
    fs::write(root.path().join("data.cfd"), "a: Item { value: 1 }").unwrap();
    let workers = (0..8)
        .map(|_| {
            let store = SessionStore::new().unwrap();
            let session = store
                .load_project(&root.path().join("coflow.yaml"))
                .unwrap()
                .session_id;
            (store, session)
        })
        .collect::<Vec<_>>();
    let barrier = std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        for (index, (store, session)) in workers.iter().enumerate() {
            let barrier = &barrier;
            scope.spawn(move || {
                barrier.wait();
                let key =
                    serde_json::json!(["data.cfd", format!("view-{index}"), "Item"]).to_string();
                store
                    .set_graph_positions(
                        *session,
                        key,
                        BTreeMap::from([("Item::a".into(), [index as f64, 1.0])]),
                    )
                    .unwrap();
            });
        }
    });
    let settings = workers[0].0.get_project_settings(workers[0].1).unwrap();
    assert_eq!(settings.graph_positions.len(), 8);
}

#[test]
fn settings_process_worker() {
    let Some(root) = std::env::var_os("COFLOW_SETTINGS_TEST_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let index = std::env::var("COFLOW_SETTINGS_TEST_INDEX").unwrap();
    let store = SessionStore::new().unwrap();
    let session = store
        .load_project(&root.join("coflow.yaml"))
        .unwrap()
        .session_id;
    fs::write(root.join(format!("ready-{index}")), "").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !root.join("start").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "settings worker barrier timeout"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let key = serde_json::json!(["data.cfd", format!("process-{index}"), "Item"]).to_string();
    store
        .set_graph_positions(
            session,
            key,
            BTreeMap::from([("Item::a".into(), [1.0, 2.0])]),
        )
        .unwrap();
}

#[test]
fn separate_processes_preserve_settings_updates() {
    let root = project_directory("settings-processes-");
    fs::write(
        root.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data.cfd\ncodegen:\n  - language: csharp\n    dir: generated/\n",
    )
    .unwrap();
    fs::write(root.path().join("schema.cft"), "table Item { value: int; }").unwrap();
    fs::write(root.path().join("data.cfd"), "a: Item { value: 1 }").unwrap();
    let mut children = (0..4)
        .map(|index| {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "settings_process_worker", "--nocapture"])
                .env("COFLOW_SETTINGS_TEST_ROOT", root.path())
                .env("COFLOW_SETTINGS_TEST_INDEX", index.to_string())
                .spawn()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !(0..4).all(|index| root.path().join(format!("ready-{index}")).exists()) {
        assert!(
            std::time::Instant::now() < deadline,
            "settings parent barrier timeout"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    fs::write(root.path().join("start"), "").unwrap();
    for child in &mut children {
        assert!(child.wait().unwrap().success());
    }
    let store = SessionStore::new().unwrap();
    let session = store
        .load_project(&root.path().join("coflow.yaml"))
        .unwrap()
        .session_id;
    assert_eq!(
        store
            .get_project_settings(session)
            .unwrap()
            .graph_positions
            .len(),
        4
    );
}

#[test]
fn configured_external_sources_can_be_opened() {
    let root = project_directory("cfd-editor-external-source-");
    let project_root = root.path().join("project");
    let shared_root = root.path().join("shared");
    fs::create_dir_all(&project_root).expect("create project directory");
    fs::create_dir_all(&shared_root).expect("create shared directory");
    fs::write(
        project_root.join("coflow.yaml"),
        concat!(
            "schema: ../shared/schema.cft\n",
            "data: ../shared/items.cfd\n",
            "codegen:\n",
            "  - language: csharp\n",
            "    dir: generated/csharp\n",
        ),
    )
    .expect("write project config");
    let schema_path = shared_root.join("schema.cft");
    let data_path = shared_root.join("items.cfd");
    fs::write(&schema_path, "table Item { value: int; }\n").expect("write external schema");
    fs::write(&data_path, "one: Item { value: 1 }\n").expect("write external data");

    let sessions = SessionStore::new().expect("create session store");
    let bootstrap = sessions
        .load_project(&project_root.join("coflow.yaml"))
        .expect("load project with external sources");
    let external_data = bootstrap
        .first_source_file
        .expect("external data is present in file tree");
    assert_eq!(
        sessions
            .source_file_path(bootstrap.session_id, &external_data)
            .expect("resolve external data source"),
        coflow_runtime::canonicalize_path(&data_path).expect("canonicalize external data")
    );
    let external_schema = coflow_runtime::path_to_slash(
        &schema_path
            .canonicalize()
            .expect("canonicalize external schema"),
    );
    assert_eq!(
        sessions
            .source_file_path(bootstrap.session_id, &external_schema)
            .expect("resolve external schema source"),
        coflow_runtime::canonicalize_path(&schema_path).expect("canonicalize external schema")
    );

    root.close().expect("remove external-source project");
}

fn array_project() -> (TempDir, PathBuf) {
    let root = project_directory("cfd-editor-array-workflow-");
    let data_dir = root.path().join("data");
    fs::create_dir_all(&data_dir).expect("create test data directory");
    fs::write(
        root.path().join("coflow.yaml"),
        concat!(
            "schema: schema.cft\n",
            "data: data/\n",
            "codegen:\n",
            "  - language: csharp\n",
            "    dir: generated/csharp\n",
        ),
    )
    .expect("write project config");
    fs::write(
        root.path().join("schema.cft"),
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

fn nested_default_collection_project() -> (TempDir, PathBuf) {
    let root = project_directory("cfd-editor-nested-default-");
    let data_dir = root.path().join("data");
    fs::create_dir_all(&data_dir).expect("create nested-default data directory");
    fs::write(
        root.path().join("coflow.yaml"),
        concat!(
            "schema: schema.cft\n",
            "data: data/\n",
            "codegen:\n",
            "  - language: csharp\n",
            "    dir: generated/csharp\n",
        ),
    )
    .expect("write nested-default project config");
    fs::write(
        root.path().join("schema.cft"),
        concat!(
            "enum Element { Fire, Ice, }\n",
            "data Combat { health: int = 100; resistances: {Element: float} = {}; }\n",
            "data Stats { label: string = \"default\"; health: int = 100; ",
            "resistances: {Element: float} = {}; combat: Combat = Combat {}; }\n",
            "table Unit { stats: Stats = Stats {}; }\n",
        ),
    )
    .expect("write nested-default schema");
    let data_file = data_dir.join("units.cfd");
    fs::write(
        &data_file,
        concat!(
            "scalar: Unit {}\n",
            "collection: Unit {}\n",
            "deep_collection: Unit { stats: Stats { label: \"keep\" } }\n",
            "deep_batch: Unit { stats: Stats { label: \"keep\" } }\n",
        ),
    )
    .expect("write nested-default data");
    (root, data_file)
}

fn inheritance_project() -> (TempDir, PathBuf) {
    let root = project_directory("cfd-editor-inheritance-workflow-");
    let data_dir = root.path().join("data");
    fs::create_dir_all(&data_dir).expect("create inheritance data directory");
    fs::write(
        root.path().join("coflow.yaml"),
        concat!(
            "schema: schema.cft\n",
            "data: data/\n",
            "codegen:\n",
            "  - language: csharp\n",
            "    dir: generated/csharp\n",
        ),
    )
    .expect("write inheritance project config");
    fs::write(
        root.path().join("schema.cft"),
        concat!(
            "abstract data Reward { label: string; }\n",
            "data ItemReward : Reward { count: int; tags: [string]; }\n",
            "data CurrencyReward : Reward { amount: int; }\n",
            "table Holder { reward: Option<Reward>; note: Option<string>; }\n",
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
            "}\n",
        ),
    )
    .expect("write inheritance data");
    fs::write(data_dir.join("empty.cfd"), "").expect("write empty data file");
    (root, populated)
}

fn string_defaults_project() -> TempDir {
    let root = project_directory("cfd-editor-string-defaults-");
    fs::create_dir_all(root.path().join("data")).expect("create string defaults data directory");
    fs::write(
        root.path().join("coflow.yaml"),
        concat!(
            "schema: schema.cft\n",
            "data: data/\n",
            "codegen:\n",
            "  - language: csharp\n",
            "    dir: generated/csharp\n",
        ),
    )
    .expect("write string defaults config");
    fs::write(
        root.path().join("schema.cft"),
        concat!(
            "table Rule {\n",
            "  name: string = \"base\";\n",
            "  label: string = f\"rule {self.name}\";\n",
            "}\n",
        ),
    )
    .expect("write string defaults schema");
    fs::write(root.path().join("data/empty.cfd"), "").expect("write empty data file");
    root
}

fn repairable_invalid_project() -> TempDir {
    let root = project_directory("cfd-editor-repairable-invalid-");
    fs::create_dir_all(root.path().join("data")).expect("create repairable data directory");
    fs::write(
        root.path().join("coflow.yaml"),
        concat!(
            "schema: schema.cft\n",
            "data: data/\n",
            "codegen:\n",
            "  - language: csharp\n",
            "    dir: generated/csharp\n",
        ),
    )
    .expect("write repairable config");
    fs::write(
        root.path().join("schema.cft"),
        concat!(
            "enum Rarity { Common, Rare }\n",
            "table Target { label: string; }\n",
            "table Item {\n",
            "  name: string;\n",
            "  note: Option<string>;\n",
            "  count: int = 7;\n",
            "  target: &Target;\n",
            "  rarity: Rarity;\n",
            "}\n",
        ),
    )
    .expect("write repairable schema");
    fs::write(
        root.path().join("data/items.cfd"),
        concat!(
            "target: Target { label: \"usable\" }\n",
            "item: Item {\n",
            "  target: &missing,\n",
            "  rarity: Rarity::Unknown,\n",
            "}\n",
        ),
    )
    .expect("write repairable data");
    root
}

fn cyclic_reference_project() -> TempDir {
    let root = project_directory("cfd-editor-cyclic-reference-");
    fs::create_dir_all(root.path().join("data")).expect("create cyclic-reference data directory");
    fs::write(
        root.path().join("coflow.yaml"),
        concat!(
            "schema: schema.cft\n",
            "data: data/\n",
            "codegen:\n",
            "  - language: csharp\n",
            "    dir: generated/csharp\n",
            "    namespace: Test.Config\n",
        ),
    )
    .expect("write cyclic-reference project config");
    fs::write(
        root.path().join("schema.cft"),
        "table Quest { prerequisite: Option<&Quest> = None; }\n",
    )
    .expect("write cyclic-reference schema");
    fs::write(root.path().join("data/quests.cfd"), "advanced: Quest {}\n")
        .expect("write cyclic-reference data");
    root
}

fn record_default(store: &SessionStore, session: u32, name: &str) -> CfdValue {
    let draft = store
        .create_record_draft(session, name)
        .expect("record draft");
    let fields = draft
        .fields
        .into_iter()
        .filter_map(|field| field.value.map(|value| (field.name, value)))
        .collect();
    CfdValue::Object(Box::new(
        CfdObject::try_new(name, fields).expect("typed draft"),
    ))
}

fn field(name: &str) -> CfdPathSegment {
    CfdPathSegment::Field(name.to_string())
}

#[test]
fn short_names_are_type_scoped_persisted_and_refreshed() {
    let (root, data_file) = array_project();
    fs::write(
        root.path().join("schema.cft"),
        "table Item { name: string; title: string = \"\"; count: int = 1; }\n",
    )
    .expect("write schema");
    fs::write(
        &data_file,
        "sword: Item { name: \"Sword\", title: \"Blade\" }\n",
    )
    .expect("write first file");
    fs::write(
        root.path().join("data/other.cfd"),
        "shield: Item { name: \"Shield\" }\n",
    )
    .expect("write second file");
    let store = SessionStore::new().expect("session store");
    let project = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("load project");
    let id = project.session_id;
    assert!(store
        .get_ref_targets(id, "Item")
        .expect("initial targets")
        .iter()
        .all(|target| target.short_name.is_none()));
    store
        .set_short_name_field(id, "Item".into(), Some("name".into()))
        .expect("set name");
    let targets = store.get_ref_targets(id, "Item").expect("named targets");
    assert_eq!(targets.len(), 2);
    assert!(targets
        .iter()
        .any(|target| target.short_name.as_deref() == Some("Sword")));
    assert!(targets
        .iter()
        .any(|target| target.short_name.as_deref() == Some("Shield")));
    assert!(store
        .set_short_name_field(id, "Item".into(), Some("count".into()))
        .is_err());
    assert!(store
        .set_short_name_field(id, "Item".into(), Some("missing".into()))
        .is_err());
    let settings = store
        .set_short_name_field(id, "Item".into(), Some("title".into()))
        .expect("replace name field");
    assert_eq!(settings.short_name_fields.len(), 1);
    assert_eq!(settings.short_name_fields["Item"], "title");
    let reloaded = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("reopen project");
    assert_eq!(
        store
            .get_project_settings(reloaded.session_id)
            .expect("read saved settings")
            .short_name_fields["Item"],
        "title"
    );
    let targets = store.get_ref_targets(id, "Item").expect("renamed targets");
    assert!(targets
        .iter()
        .any(|target| target.short_name.as_deref() == Some("Blade")));
    assert!(targets
        .iter()
        .any(|target| target.coordinate.id.as_str() == "shield" && target.short_name.is_none()));
    // 修改字段值后，引用缓存必须读取新值，且记录 ID 保持不变。
    let coordinate = RecordCoordinate::try_new("Item", "sword").expect("coordinate");
    store
        .write_field(
            id,
            &coordinate,
            &[field("title")],
            &CfdValue::String("New blade".into()),
        )
        .expect("edit title");
    assert!(store
        .get_ref_targets(id, "Item")
        .expect("updated targets")
        .iter()
        .any(|target| target.coordinate == coordinate
            && target.short_name.as_deref() == Some("New blade")));
    store
        .set_short_name_field(id, "Item".into(), None)
        .expect("clear name field");
    assert!(store
        .get_ref_targets(id, "Item")
        .expect("cleared targets")
        .iter()
        .all(|target| target.short_name.is_none()));
    root.close().expect("remove project");
}

#[test]
fn graph_positions_persist_per_view_without_changing_data_revision() {
    let (root, _) = array_project();
    let store = SessionStore::new().expect("session store");
    let project = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("load project");
    let positions = BTreeMap::from([("Item::one".to_string(), [-320.5, 480.25])]);
    store
        .set_graph_positions(
            project.session_id,
            r#"["data/05-arrays.cfd","first-view","ArrayExample"]"#.into(),
            positions.clone(),
        )
        .expect("save positions");
    store
        .set_graph_positions(
            project.session_id,
            r#"["data/05-arrays.cfd","second-view","ArrayExample"]"#.into(),
            BTreeMap::new(),
        )
        .expect("save second view");
    assert!(store
        .set_graph_positions(
            project.session_id,
            r#"["data/05-arrays.cfd","first-view","ArrayExample"]"#.into(),
            BTreeMap::from([("Item::one".to_string(), [f64::NAN, 0.0])])
        )
        .is_err());
    assert_eq!(
        store
            .get_file_records(project.session_id, "data/05-arrays.cfd")
            .expect("read records")
            .revision,
        project.revision
    );
    let reopened = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("reopen project");
    let settings = store
        .get_project_settings(reopened.session_id)
        .expect("read positions");
    assert_eq!(
        settings.graph_positions[r#"["data/05-arrays.cfd","first-view","ArrayExample"]"#],
        positions
    );
    assert!(
        settings.graph_positions[r#"["data/05-arrays.cfd","second-view","ArrayExample"]"#]
            .is_empty()
    );
    root.close().expect("remove project");
}

#[test]
fn editor_field_write_preserves_a_self_referencing_record() {
    let root = cyclic_reference_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("load cyclic-reference project");
    let coordinate = RecordCoordinate::try_new("Quest", "advanced").expect("coordinate");

    // 记录引用是图边；自引用写入后，当前记录及其引用索引都必须继续可用。
    let outcome = store
        .write_field(
            snapshot.session_id,
            &coordinate,
            &[field("prerequisite")],
            &CfdValue::OptionSome(Box::new(
                CfdValue::record_ref("advanced").expect("reference"),
            )),
        )
        .expect("write self reference");
    assert_eq!(
        outcome.diagnostics,
        [] as [coflow_runtime::FlatDiagnostic; 0]
    );
    assert_eq!(outcome.row.coordinate, coordinate);

    let records = store
        .get_file_records(snapshot.session_id, "data/quests.cfd")
        .expect("read records after self reference");
    assert_eq!(records.records.len(), 1);
    assert_eq!(records.records[0].coordinate, coordinate);

    let targets = store
        .get_ref_targets(snapshot.session_id, "Quest")
        .expect("read reference targets after self reference");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].coordinate, coordinate);
}

#[test]
fn repository_projects_open_in_editor() {
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let fixtures = tempfile::tempdir().expect("isolated repository fixtures");
    let repository_root = fixtures.path();
    // 本机设置按项目路径隔离；复制夹具避免读取或改写开发者已有的项目设置。

    for relative in [
        "examples/showcase",
        "tests/fixtures/editor-project",
        "tests/fixtures/editor-inheritance",
    ] {
        let source = source_root.join(relative);
        let target = repository_root.join(relative);
        fs::create_dir_all(&target).expect("create isolated project");
        fs::copy(source.join("coflow.yaml"), target.join("coflow.yaml")).expect("copy config");
        for directory in ["schema", "data", "editor-setting"] {
            if source.join(directory).exists() {
                copy_directory(&source.join(directory), &target.join(directory));
            }
        }
    }

    for (project, config) in [
        (
            "showcase",
            repository_root.join("examples/showcase/coflow.yaml"),
        ),
        (
            "editor-project",
            repository_root.join("tests/fixtures/editor-project/coflow.yaml"),
        ),
        (
            "editor-inheritance",
            repository_root.join("tests/fixtures/editor-inheritance/coflow.yaml"),
        ),
    ] {
        let store = SessionStore::new().expect("create editor session store");
        let snapshot = store.load_project(&config).unwrap_or_else(|error| {
            panic!(
                "editor failed to open repository project `{project}` at {}: {error}",
                config.display()
            )
        });

        let expected_root = config.parent().unwrap().canonicalize().unwrap();
        assert_eq!(
            snapshot.project_root,
            coflow_runtime::path_to_slash(&expected_root)
        );
        assert!(
            snapshot
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity != "error"),
            "editor reported errors for repository project `{project}`: {:#?}",
            snapshot.diagnostics
        );

        if project == "editor-project" {
            store
                .get_project_settings(snapshot.session_id)
                .expect("editor fixture settings must parse");
        }
    }
}

#[test]
fn inherited_and_optional_polymorphic_values_edit_end_to_end() {
    let (root, populated) = inheritance_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("load inheritance project");
    let session_id = snapshot.session_id;

    let empty_types = snapshot
        .file_types
        .get("data/empty.cfd")
        .expect("empty types");
    assert!(empty_types.iter().any(|ty| ty.name == "Holder"));
    assert!(!empty_types.iter().any(|ty| ty.name == "ItemReward"));
    assert!(store.create_record_draft(session_id, "ItemReward").is_err());
    let new_holder = record_default(&store, session_id, "Holder");
    store
        .insert_record(
            session_id,
            "data/empty.cfd",
            "new_holder",
            "Holder",
            new_holder,
        )
        .expect("insert table into empty file");

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

    assert_inheritance_edits(&store, session_id, root.path(), &populated);
}

fn assert_inheritance_edits(store: &SessionStore, session_id: u32, root: &Path, populated: &Path) {
    let reloaded = store
        .reload_session(session_id)
        .expect("reload inheritance project");
    assert!(
        reloaded.diagnostics.is_empty(),
        "{:#?}",
        reloaded.diagnostics
    );
    let source = fs::read_to_string(populated).expect("read inheritance source");
    assert!(source.contains("label: \"updated\""), "{source}");
    assert!(source.contains("count: 2"), "{source}");
    assert!(source.contains("tags: [\"\"]"), "{source}");
    assert!(source.contains("note: \"new\""), "{source}");
    let empty_source =
        fs::read_to_string(root.join("data/empty.cfd")).expect("read formerly empty source");
    assert!(
        empty_source.contains("new_holder: Holder"),
        "{empty_source}"
    );
}

#[test]
fn array_editor_mutations_round_trip_through_reload() {
    let (root, data_file) = array_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.path().join("coflow.yaml"))
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
        .rename_record_id(session_id, &original, "featured_tags_v2")
        .expect("rename grouped record")
        .renamed;
    let draft = record_default(&store, session_id, "ArrayExample");
    store
        .insert_record(session_id, file_path, "empty_array", "ArrayExample", draft)
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
    assert!(
        reloaded.diagnostics.is_empty(),
        "{:#?}",
        reloaded.diagnostics
    );
    let records = store
        .get_file_records(session_id, file_path)
        .expect("read records after reload");
    assert_eq!(records.records.len(), 1);
    assert_eq!(records.records[0].coordinate, renamed);
    let source = fs::read_to_string(data_file).expect("read final CFD source");
    // 无显式值的追加项来自元素类型默认值，不能复制现有末项。
    assert!(source.contains("\"recommended\", \"\""), "{source}");
    assert!(
        !source.contains("\"recommended\", \"recommended\""),
        "{source}"
    );
}

#[test]
fn nested_collection_edit_materializes_a_missing_top_level_default() {
    let (root, data_file) = nested_default_collection_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("load nested-default project");
    let coordinate = RecordCoordinate::try_new("Unit", "collection").expect("unit coordinate");

    let outcome = store
        .edit_collection(
            snapshot.session_id,
            &coordinate,
            &[field("stats"), field("resistances")],
            CollectionEdit::DictInsert {
                key: coflow_runtime::CfdDictKey::Enum(coflow_runtime::CfdEnumValue {
                    enum_name: "Element".try_into().expect("enum name"),
                    variant: Some("Fire".try_into().expect("variant name")),
                    value: 0,
                }),
                value: Some(CfdValue::Float(0.5)),
            },
        )
        .expect("insert into collection under omitted default object");
    assert!(matches!(outcome.old_value, Some(CfdValue::Dict(_))));
    assert!(matches!(outcome.new_value, Some(CfdValue::Dict(_))));

    let source = fs::read_to_string(data_file).expect("read materialized source");
    assert!(source.contains("stats: Stats"), "{source}");
    assert!(source.contains("Fire: 0.5"), "{source}");
}

#[test]
fn nested_scalar_edit_materializes_a_missing_top_level_default() {
    let (root, data_file) = nested_default_collection_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("load nested-default project");
    let coordinate = RecordCoordinate::try_new("Unit", "scalar").expect("unit coordinate");

    let outcome = store
        .write_field(
            snapshot.session_id,
            &coordinate,
            &[field("stats"), field("health")],
            &CfdValue::Int(125),
        )
        .expect("write scalar under omitted default object");

    assert_eq!(outcome.old_value, Some(CfdValue::Int(100)));
    assert_eq!(outcome.new_value, Some(CfdValue::Int(125)));
    let source = fs::read_to_string(data_file).expect("read materialized source");
    assert!(source.contains("health: 125"), "{source}");
}

#[test]
fn batch_nested_edits_materialize_missing_top_level_defaults() {
    let (root, data_file) = nested_default_collection_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("load nested-default project");
    let path = vec![field("stats"), field("health")];
    let scalar = RecordCoordinate::try_new("Unit", "scalar").expect("scalar coordinate");
    let collection =
        RecordCoordinate::try_new("Unit", "collection").expect("collection coordinate");

    let outcome = store
        .write_fields(
            snapshot.session_id,
            &[
                BatchWriteFieldInput {
                    coordinate: scalar,
                    field_path: path.clone(),
                    new_value: CfdValue::Int(120),
                },
                BatchWriteFieldInput {
                    coordinate: collection,
                    field_path: path,
                    new_value: CfdValue::Int(130),
                },
            ],
        )
        .expect("batch-write scalars under omitted default objects");

    assert_eq!(outcome.edits.len(), 2);
    assert!(outcome.edits.iter().all(|edit| {
        edit.old_value == Some(CfdValue::Int(100))
            && matches!(edit.new_value, Some(CfdValue::Int(120 | 130)))
    }));
    let source = fs::read_to_string(data_file).expect("read materialized source");
    assert!(source.contains("health: 120"), "{source}");
    assert!(source.contains("health: 130"), "{source}");
}

#[test]
fn deep_default_parents_materialize_for_collection_and_batch_edits() {
    let (root, data_file) = nested_default_collection_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("load nested-default project");
    let deep_collection =
        RecordCoordinate::try_new("Unit", "deep_collection").expect("deep collection coordinate");
    let enum_key = coflow_runtime::CfdDictKey::Enum(coflow_runtime::CfdEnumValue {
        enum_name: "Element".try_into().expect("enum name"),
        variant: Some("Ice".try_into().expect("variant name")),
        value: 1,
    });

    let collection_outcome = store
        .edit_collection(
            snapshot.session_id,
            &deep_collection,
            &[field("stats"), field("combat"), field("resistances")],
            CollectionEdit::DictInsert {
                key: enum_key.clone(),
                value: Some(CfdValue::Float(0.25)),
            },
        )
        .expect("edit collection through an omitted intermediate default");
    assert!(matches!(
        collection_outcome.old_value,
        Some(CfdValue::Dict(_))
    ));
    assert!(matches!(
        collection_outcome.new_value,
        Some(CfdValue::Dict(_))
    ));

    let deep_batch =
        RecordCoordinate::try_new("Unit", "deep_batch").expect("deep batch coordinate");
    let batch_outcome = store
        .write_fields(
            snapshot.session_id,
            &[
                BatchWriteFieldInput {
                    coordinate: deep_batch.clone(),
                    field_path: vec![field("stats"), field("combat"), field("health")],
                    new_value: CfdValue::Int(175),
                },
                BatchWriteFieldInput {
                    coordinate: deep_batch,
                    field_path: vec![field("stats"), field("combat"), field("resistances")],
                    new_value: CfdValue::Dict(vec![(enum_key, CfdValue::Float(0.75))]),
                },
            ],
        )
        .expect("batch edit through the same omitted intermediate default");
    assert_eq!(batch_outcome.edits.len(), 2);
    assert_eq!(batch_outcome.edits[0].old_value, Some(CfdValue::Int(100)));
    assert_eq!(batch_outcome.edits[0].new_value, Some(CfdValue::Int(175)));
    assert!(matches!(
        batch_outcome.edits[1].old_value,
        Some(CfdValue::Dict(_))
    ));
    assert!(matches!(
        batch_outcome.edits[1].new_value,
        Some(CfdValue::Dict(_))
    ));

    let source = fs::read_to_string(data_file).expect("read deeply materialized source");
    assert_eq!(source.matches("label: \"keep\"").count(), 2, "{source}");
    assert!(source.contains("health: 175"), "{source}");
    assert!(source.contains("Ice: 0.25"), "{source}");
    assert!(source.contains("Ice: 0.75"), "{source}");
}

#[test]
fn source_text_edit_saves_invalid_data_with_complete_diagnostics() {
    let (root, data_file) = array_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.path().join("coflow.yaml"))
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
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == "error"),
        "{diagnostics:#?}"
    );
    assert!(
        diagnostics.iter().any(|diagnostic| matches!(
            &diagnostic.target,
            coflow_runtime::DiagnosticTarget::Source { range: Some(range), .. }
                if range.start.line == 1
        )),
        "{diagnostics:#?}"
    );
    // 外部修改与编辑基线不一致时禁止覆盖磁盘内容。
    fs::write(&data_file, format!("{original}\n")).expect("external source write");
    store
        .write_source_text(session_id, file_path, invalid, &original)
        .expect_err("stale source baseline must conflict");
    assert_eq!(
        fs::read_to_string(&data_file).expect("read disk"),
        format!("{original}\n")
    );
    fs::write(&data_file, &original).expect("restore source baseline");
    let bootstrap = store
        .write_source_text(session_id, file_path, invalid, &original)
        .expect("invalid data source remains editable");
    assert!(bootstrap
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == "error"
            && matches!(
                &diagnostic.target,
                coflow_runtime::DiagnosticTarget::Source { .. }
            )));
    assert_eq!(
        fs::read_to_string(&data_file).expect("read changed source"),
        invalid
    );

    let edited = format!("{original}\n");
    let saved = store
        .write_source_text(session_id, file_path, &edited, invalid)
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
fn diff_snapshot_highlighting_supports_deleted_files_without_changing_live_sources() {
    let (root, _) = array_project();
    let store = SessionStore::new().expect("create session store");
    let project = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("load project");
    let source = store
        .read_source_text(project.session_id, "schema.cft")
        .expect("read schema");
    let live = store
        .sync_language_document(project.session_id, "schema.cft", &source, 1)
        .expect("open live schema");
    for path in ["schema.cft", "deleted.cft"] {
        let tokens = store
            .highlight_source_snapshot(project.session_id, path, "table Old { value: int; }\n")
            .expect("highlight CFT snapshot");
        assert!(!tokens.semantic_token_data.is_empty(), "{path}");
    }
    let tokens = store
        .highlight_source_snapshot(
            project.session_id,
            "data/deleted.cfd",
            "old: Old { value: 1 }\n",
        )
        .expect("highlight deleted CFD snapshot");
    assert_ne!(tokens.semantic_token_data, [] as [u32; 0]);
    let current = store
        .sync_language_document(project.session_id, "schema.cft", &source, 2)
        .expect("read live language state");
    assert_eq!(live.semantic_token_data, current.semantic_token_data);
    assert_eq!(
        fs::read_to_string(root.path().join("schema.cft")).expect("read disk"),
        source
    );
    root.close().expect("remove project");
}

#[test]
fn editor_language_features_are_served_by_embedded_lsp() {
    let (root, _) = array_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("load language-service project");
    let file_path = "data/05-arrays.cfd";
    let source = store
        .read_source_text(snapshot.session_id, file_path)
        .expect("read source text");

    let document = store
        .sync_language_document(snapshot.session_id, file_path, &source, 1)
        .expect("synchronize through embedded LSP");
    assert!(
        document.diagnostics.is_empty(),
        "{:#?}",
        document.diagnostics
    );
    assert!(document.syntax_valid);
    assert_ne!(document.semantic_token_data, [] as [u32; 0]);
    assert!(document
        .semantic_token_types
        .iter()
        .any(|kind| kind == "type"));

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
            &LanguagePosition {
                line: 0,
                character: 0,
            },
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
        .is_some_and(|text| text.contains("${1:id}")));

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
fn missing_optional_default_ref_and_enum_states_are_structurally_repairable() {
    let root = repairable_invalid_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("load repairable project");
    let coordinate = RecordCoordinate::try_new("Item", "item").expect("coordinate");
    let records = store
        .get_file_records(snapshot.session_id, "data/items.cfd")
        .expect("read repairable rows");
    let row = records
        .records
        .iter()
        .find(|row| row.coordinate == coordinate)
        .unwrap_or_else(|| {
            panic!(
                "invalid record remains visible; rows={:#?}; diagnostics={:#?}",
                records.records, snapshot.diagnostics
            )
        });
    let cell = |name: &str| {
        let index = row
            .field_index
            .get(name)
            .copied()
            .expect("declared cell index");
        &row.fields[index]
    };

    assert!(cell("name").missing);
    assert!(matches!(cell("name").value, CfdValue::String(_)));
    assert!(cell("note").missing);
    assert!(matches!(cell("note").value, CfdValue::OptionNone));
    assert!(!cell("count").missing);
    assert!(matches!(cell("count").value, CfdValue::Int(7)));
    assert!(!cell("target").missing);
    assert!(matches!(cell("target").value, CfdValue::Ref(_)));
    assert!(cell("rarity").missing);
    assert_eq!(
        cell("rarity")
            .annotation
            .as_ref()
            .and_then(|a| a.enum_type.as_deref()),
        Some("Rarity")
    );

    assert!(snapshot.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "DATA-006"
            && matches!(&diagnostic.target, coflow_runtime::DiagnosticTarget::TableField { field_path, .. } if field_path == "name")
    }));
    assert!(snapshot.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "DATA-008"
            && matches!(&diagnostic.target, coflow_runtime::DiagnosticTarget::TableField { field_path, .. } if field_path == "rarity")
    }));

    store
        .write_field(
            snapshot.session_id,
            &coordinate,
            &[field("name")],
            &CfdValue::String("fixed".to_string()),
        )
        .expect("repair missing scalar");
    store
        .write_field(
            snapshot.session_id,
            &coordinate,
            &[field("note")],
            &CfdValue::OptionNone,
        )
        .expect("explicitly supply the required nullable field");
    store
        .write_field(
            snapshot.session_id,
            &coordinate,
            &[field("target")],
            &CfdValue::record_ref("target").expect("reference"),
        )
        .expect("repair invalid reference");
    let repaired = store
        .write_field(
            snapshot.session_id,
            &coordinate,
            &[field("rarity")],
            &CfdValue::Enum(
                coflow_runtime::CfdEnumValue::try_new("Rarity", Some("Rare"), 1)
                    .expect("enum value"),
            ),
        )
        .expect("repair invalid enum");
    assert!(repaired
        .diagnostics
        .iter()
        .all(|diagnostic| diagnostic.severity != "error"));

    let repaired_records = store
        .get_file_records(snapshot.session_id, "data/items.cfd")
        .expect("read repaired rows");
    let row = repaired_records
        .records
        .iter()
        .find(|row| row.coordinate == coordinate)
        .expect("repaired record");
    assert!(row.fields.iter().all(|cell| !cell.missing));
}

#[test]
fn new_record_can_be_created_with_a_missing_required_reference() {
    let root = repairable_invalid_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("load repairable project");
    let object = CfdObject::try_new("Item", BTreeMap::new()).expect("object");

    let outcome = store
        .insert_record(
            snapshot.session_id,
            "data/items.cfd",
            "new_item",
            "Item",
            CfdValue::Object(Box::new(object)),
        )
        .expect("insert partial record");

    assert!(outcome.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == "error"
            && matches!(&diagnostic.target, coflow_runtime::DiagnosticTarget::TableField { field_path, .. } if field_path == "target")
    }));
    let source = fs::read_to_string(root.path().join("data/items.cfd")).expect("source");
    assert!(source.contains("new_item: Item"), "{source}");
    assert_eq!(
        source.matches("target: &missing").count(),
        1,
        "new record must not receive a fabricated ref: {source}"
    );
}

#[test]
fn cft_string_defaults_are_supported_through_the_editor_backend() {
    let root = string_defaults_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("load defaults project");
    let source = store
        .read_source_text(snapshot.session_id, "schema.cft")
        .expect("read schema");
    let document = store
        .sync_language_document(snapshot.session_id, "schema.cft", &source, 1)
        .expect("synchronize defaults");
    assert!(
        document.diagnostics.is_empty(),
        "{:#?}",
        document.diagnostics
    );
    assert!(document.syntax_valid);
    assert_ne!(document.semantic_token_data, [] as [u32; 0]);
    let value = record_default(&store, snapshot.session_id, "Rule");
    let CfdValue::Object(rule) = value else {
        panic!("expected Rule object")
    };
    assert!(matches!(rule.field("name"), Some(CfdValue::String(value)) if value == "base"));
    assert!(matches!(
        rule.field("label"),
        Some(CfdValue::FormattedString(_))
    ));
}

#[test]
fn cft_source_is_visible_editable_and_validated() {
    let (root, _) = array_project();
    let store = SessionStore::new().expect("create editor session store");
    let snapshot = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("load schema editing project");
    let file_path = "schema.cft";
    let source = store
        .read_source_text(snapshot.session_id, file_path)
        .expect("read configured CFT source");
    assert!(source.contains("table ArrayExample"));

    let invalid = source.replace("[string]", "MissingType");
    let diagnostics = store
        .validate_source_text(snapshot.session_id, file_path, &invalid)
        .expect("validate invalid CFT draft");
    assert!(diagnostics.iter().any(|item| item.severity == "error"));
    store
        .write_source_text(snapshot.session_id, file_path, &invalid, &source)
        .expect_err("invalid CFT remains in the editing draft");

    let unformatted = source.replace("  tags:", "tags:");
    let formatted = store
        .format_language_document(snapshot.session_id, file_path, &unformatted, 1)
        .expect("format CFT through embedded LSP");
    assert!(formatted.text.contains("  tags:"), "{}", formatted.text);
    assert!(!formatted.edits.is_empty());
    let source_with_added_type = format!(
        "{}\n\ntable AddedFromSourceEditor {{\n  value: string;\n}}\n",
        formatted.text.trim_end()
    );
    let saved = store
        .write_source_text(
            snapshot.session_id,
            file_path,
            &source_with_added_type,
            &source,
        )
        .expect("save valid CFT source");
    assert!(saved.revision > snapshot.revision);
    assert!(saved
        .file_types
        .values()
        .flatten()
        .any(|option| { option.name == "AddedFromSourceEditor" }));

    let invalid_syntax = source.replacen("table ArrayExample {", "type ArrayExample", 1);
    let invalid_formatting = store
        .format_language_document(snapshot.session_id, file_path, &invalid_syntax, 2)
        .expect("ignore formatting for invalid CFT");
    assert_eq!(invalid_formatting.text, invalid_syntax);
    assert!(invalid_formatting.edits.is_empty());
}

#[test]
fn editor_check_command_runs_rules_while_build_only_generates_data_types() {
    let (root, data_file) = array_project();
    fs::write(
        root.path().join("schema.cft"),
        "table Item { name: string; check Valid { false : \"editor business issue\"; } }",
    )
    .expect("schema");
    fs::write(data_file, "item: Item { name: \"Item\" }").expect("data");
    let sessions = SessionStore::new().expect("session store");
    let bootstrap = sessions
        .load_project(&root.path().join("coflow.yaml"))
        .expect("editor session");
    assert!(sessions.check_project(bootstrap.session_id).is_err());
    assert!(sessions.build_project(bootstrap.session_id).is_ok());
    root.close().expect("remove project");
}

#[test]
fn cell_templates_and_explicit_reference_constraints_survive_reload() {
    let root = project_directory("cfd-editor-template-owner-");
    fs::create_dir_all(root.path().join("data")).expect("create project");
    fs::write(
        root.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data/\ncodegen:\n  - language: csharp\n    dir: generated\n",
    )
    .expect("config");
    fs::write(root.path().join("schema.cft"), r#"
        data Payload { score: int; label: string = f"{self.score}"; labels: [string] = []; optional: Option<string> = None; }
        table Node { label: string = f"{self.id}"; }
        table Child: Node {}
        table Entry { payload: Payload; target: &Node; }
    "#).expect("schema");
    fs::write(root.path().join("data/main.cfd"), "base: Node {} child: Child {} entry: Entry { payload: Payload { score: 7 }, target: &base }").expect("data");
    let store = SessionStore::new().expect("store");
    let session = store
        .load_project(&root.path().join("coflow.yaml"))
        .expect("load");
    assert!(session.diagnostics.is_empty(), "{:?}", session.diagnostics);
    let coordinate = RecordCoordinate::try_new("Entry", "entry").expect("coordinate");
    for (name, text) in [
        ("label", r#"f"value {self.score}""#),
        (
            "labels",
            r#"[f"first {self.score}" | f"second {self.score}"]"#,
        ),
        ("optional", r#"Some(f"maybe {self.score}")"#),
    ] {
        let path = [field("payload"), field(name)];
        let value = store
            .parse_cell_text(session.session_id, &coordinate, &path, text)
            .expect("parse nested template");
        store
            .write_field(session.session_id, &coordinate, &path, &value)
            .expect("write template");
    }
    assert!(store
        .parse_cell_text(
            session.session_id,
            &coordinate,
            &[field("target")],
            "&Child::base"
        )
        .is_err());
    let typed = store
        .parse_cell_text(
            session.session_id,
            &coordinate,
            &[field("target")],
            "&Child::child",
        )
        .expect("correct subtype reference");
    store
        .write_field(session.session_id, &coordinate, &[field("target")], &typed)
        .expect("write typed reference");
    let snapshot = store.reload_session(session.session_id).expect("reload");
    assert!(
        snapshot.diagnostics.is_empty(),
        "{:?}",
        snapshot.diagnostics
    );
    let rows = store
        .get_file_records(session.session_id, "data/main.cfd")
        .expect("rows");
    let row = rows
        .records
        .iter()
        .find(|row| row.coordinate == coordinate)
        .expect("entry");
    let CfdValue::Object(payload) = &row
        .fields
        .iter()
        .find(|cell| cell.name == "payload")
        .expect("payload")
        .value
    else {
        panic!("object");
    };
    assert!(
        matches!(payload.field("label"), Some(CfdValue::FormattedString(value)) if value.rendered == "value 7")
    );
    let Some(CfdValue::Array(labels)) = payload.field("labels") else {
        panic!("labels");
    };
    assert!(matches!(&labels[0], CfdValue::FormattedString(value) if value.rendered == "first 7"));
    assert!(
        matches!(payload.field("optional"), Some(CfdValue::OptionSome(value)) if matches!(&**value, CfdValue::FormattedString(text) if text.rendered == "maybe 7"))
    );
    root.close().expect("remove fixture");
}

fn copy_directory(source: &Path, target: &Path) {
    fs::create_dir_all(target).expect("create fixture directory");
    for entry in fs::read_dir(source).expect("read fixture directory") {
        let entry = entry.expect("fixture entry");
        let output = target.join(entry.file_name());
        if entry.file_type().expect("fixture type").is_dir() {
            copy_directory(&entry.path(), &output);
        } else {
            fs::copy(entry.path(), output).expect("copy fixture file");
        }
    }
}

#[test]
fn compound_editor_mutation_does_not_publish_its_first_step_when_second_step_fails() {
    let root = project_directory("editor-atomic-mutation-");
    fs::write(
        root.path().join("coflow.yaml"),
        "schema: schema.cft\ndata: data.cfd\ncodegen:\n  - language: csharp\n    dir: generated\n",
    )
    .unwrap();
    fs::write(
        root.path().join("schema.cft"),
        "table Item { value: int = 0; }\n",
    )
    .unwrap();
    let initial = "one: Item { value: 1 }\n";
    fs::write(root.path().join("data.cfd"), initial).unwrap();
    let store = SessionStore::new().unwrap();
    let bootstrap = store
        .load_project(&root.path().join("coflow.yaml"))
        .unwrap();
    let input = cfd_editor_core::editor::CreateLinkedRecordInput {
        file: "data.cfd".into(),
        id: "two".into(),
        actual_type: "Item".into(),
        fields: CfdValue::Object(Box::new(
            CfdObject::try_new("Item", BTreeMap::new()).unwrap(),
        )),
        source_file: "data.cfd".into(),
        source: RecordCoordinate::try_new("Item", "one").unwrap(),
        field_path: vec![CfdPathSegment::Field("value".into())],
        reference: CfdValue::String("invalid integer".into()),
        append: false,
    };
    assert!(store
        .create_linked_record(bootstrap.session_id, input)
        .is_err());
    assert_eq!(
        fs::read_to_string(root.path().join("data.cfd")).unwrap(),
        initial
    );
    assert_eq!(
        store
            .get_file_records(bootstrap.session_id, "data.cfd")
            .unwrap()
            .revision,
        bootstrap.revision
    );
}

#[test]
fn linked_record_creation_and_inverse_batch_publish_one_generation_each() {
    let root = cyclic_reference_project();
    let store = SessionStore::new().unwrap();
    let bootstrap = store
        .load_project(&root.path().join("coflow.yaml"))
        .unwrap();
    let coordinate = RecordCoordinate::try_new("Quest", "advanced").unwrap();
    let created = RecordCoordinate::try_new("Quest", "basic").unwrap();
    let path = vec![CfdPathSegment::Field("prerequisite".into())];
    let outcome = store
        .create_linked_record(
            bootstrap.session_id,
            cfd_editor_core::editor::CreateLinkedRecordInput {
                file: "data/quests.cfd".into(),
                id: "basic".into(),
                actual_type: "Quest".into(),
                fields: CfdValue::Object(Box::new(
                    CfdObject::try_new("Quest", BTreeMap::new()).unwrap(),
                )),
                source_file: "data/quests.cfd".into(),
                source: coordinate.clone(),
                field_path: path.clone(),
                reference: CfdValue::OptionSome(Box::new(CfdValue::Ref(created.id.clone()))),
                append: false,
            },
        )
        .unwrap();
    assert_eq!(outcome.revision, bootstrap.revision + 1);
    assert_eq!(
        store
            .get_file_records(bootstrap.session_id, "data/quests.cfd")
            .unwrap()
            .records
            .len(),
        2
    );
    let undone = store
        .apply_mutation_batch(
            bootstrap.session_id,
            vec![
                coflow_runtime::MutationOp::SetField {
                    record: coordinate,
                    file: None,
                    path,
                    value: coflow_runtime::MutationValue::Cfd(outcome.old_value),
                },
                coflow_runtime::MutationOp::DeleteRecord {
                    record: created,
                    file: None,
                },
            ],
        )
        .unwrap();
    assert_eq!(undone.revision, outcome.revision + 1);
    assert_eq!(
        store
            .get_file_records(bootstrap.session_id, "data/quests.cfd")
            .unwrap()
            .records
            .len(),
        1
    );
}
