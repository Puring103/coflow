#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use coflow::commands::{check_project, CommandOutcome};
use coflow_api::{
    CfdInputRecord, CfdInputValue, Diagnostic, DiagnosticSet, LoadedSource, ProbeResult,
    ProjectSourceRef, RecordOrigin, RenameRecordRequest, ResolvedSource,
    RewriteRecordReferencesRequest,
    SourceDocument, SourceLoadContext, SourceLocationSpec, SourceProvider,
    SourceProviderDescriptor, SourceWriter, WriteCellRequest, WriteContext, WriteOutcome,
    WriterCapabilities, WriterDescriptor,
};
use coflow_project::Project;
use coflow_runtime::{build_project_session_for_build, RecordCoordinate};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

mod common;

#[test]
fn engine_builds_record_and_source_indexes() {
    let root = common::temp_project_dir("engine-indexes");
    let _cleanup = common::TempDirCleanup(root.clone());
    common::write_invalid_check_project(&root).expect("write project");
    let config = root.join("coflow.yaml");
    let project = Project::open_schema_only(Some(&config)).expect("open project");
    let registry = coflow_builtins::default_provider_registry().expect("default registry");

    let session = build_project_session_for_build(project, &registry).expect("build session");

    assert!(
        session.has_diagnostics(),
        "check diagnostic should be captured"
    );
    assert!(
        session.files.source_files().contains("data/configs.xlsx"),
        "file index should contain loaded xlsx source"
    );
    let record = session
        .records
        .get_by_coordinate("Item", "item_1")
        .expect("record index should contain item_1");
    assert_eq!(record.display_path, "data/configs.xlsx");
    assert_eq!(record.provider_id, "excel");
    let table = session
        .model
        .table("Item")
        .expect("check diagnostics should not discard the loaded model");
    assert_eq!(
        table.records.len(),
        1,
        "engine should retain records when CFT checks fail"
    );
    assert!(
        session
            .files
            .source_for_display("data/configs.xlsx")
            .is_some(),
        "file index should map display path to source id"
    );
}

#[test]
fn command_check_uses_engine_diagnostics() {
    let root = common::temp_project_dir("commands-check-engine");
    let _cleanup = common::TempDirCleanup(root.clone());
    common::write_invalid_check_project(&root).expect("write project");
    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open project");
    let registry = coflow_builtins::default_provider_registry().expect("default registry");

    let outcome = check_project(project, &registry).expect("check command");
    let CommandOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("invalid project should return diagnostics");
    };

    assert!(
        diagnostics
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "CFD-CHECK-007"),
        "check diagnostics should flow through canonical DiagnosticSet"
    );
}

#[test]
fn rename_record_key_updates_cross_source_references() {
    let root = common::temp_project_dir("engine-rename-key-refs");
    let _cleanup = common::TempDirCleanup(root.clone());
    write_rename_reference_project(&root);

    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open project");
    let registry = coflow_builtins::default_provider_registry().expect("default registry");
    let mut session = build_project_session_for_build(project, &registry).expect("build session");
    assert!(
        !session.has_diagnostics(),
        "diagnostics before rename: {:?}",
        session.diagnostics.as_set()
    );

    let outcome = session
        .rename_record_key(&registry, "Item", "sword", "blade")
        .expect("rename key");

    assert_eq!(
        outcome.renamed,
        Some((
            RecordCoordinate::new("Item", "sword"),
            RecordCoordinate::new("Item", "blade")
        ))
    );
    assert!(
        session.records.get_by_coordinate("Item", "blade").is_some(),
        "renamed record should be indexed"
    );
    assert!(
        session.records.get_by_coordinate("Item", "sword").is_none(),
        "old coordinate should be absent"
    );
    assert!(
        !session.has_diagnostics(),
        "diagnostics after rename: {:?}",
        session.diagnostics.as_set()
    );
    let items = std::fs::read_to_string(root.join("data/items.cfd")).expect("read items");
    let bundles = std::fs::read_to_string(root.join("data/bundles.csv")).expect("read bundles");
    let spread = std::fs::read_to_string(root.join("data/spread.cfd")).expect("read spread");
    assert!(items.contains("blade: Item"), "items source:\n{items}");
    assert!(!items.contains("sword: Item"), "items source:\n{items}");
    assert!(
        bundles.contains("starter,&blade,&blade,sword name"),
        "csv refs should be updated:\n{bundles}"
    );
    assert!(
        !bundles.contains("&sword"),
        "csv refs should not keep old key:\n{bundles}"
    );
    assert!(
        spread.contains("...&base_bundle"),
        "unrelated spread should be preserved:\n{spread}"
    );
    assert!(
        items.contains("&blade"),
        "cfd refs should be updated:\n{items}"
    );
    assert!(!items.contains("&sword"), "old refs remain:\n{items}");
}

#[test]
fn rename_record_key_does_not_scan_remote_sources_without_spread_provenance() {
    let root = common::temp_project_dir("engine-rename-remote-refs");
    let _cleanup = common::TempDirCleanup(root.clone());
    write_remote_rewrite_project(&root);
    let tracker = Arc::new(Mutex::new(RemoteRewriteTracker::default()));
    let mut registry = coflow_builtins::default_provider_registry().expect("default registry");
    registry
        .register_source_provider(FakeRemoteLoader)
        .expect("register fake remote loader");
    registry
        .register_source_writer(FakeRemoteWriter {
            tracker: Arc::clone(&tracker),
        })
        .expect("register fake remote writer");

    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open project");
    let mut session = build_project_session_for_build(project, &registry).expect("build session");
    assert!(
        !session.has_diagnostics(),
        "diagnostics before rename: {:?}",
        session.diagnostics.as_set()
    );

    session
        .rename_record_key(&registry, "Item", "sword", "blade")
        .expect("rename key");

    let (renamed, rewrites) = {
        let tracker = tracker.lock().unwrap();
        (tracker.renamed.clone(), tracker.rewrites.clone())
    };
    assert!(
        renamed.is_empty(),
        "target record lives in CFD, so fake remote should not rename it"
    );
    assert_eq!(
        rewrites,
        Vec::<RemoteRewriteCall>::new(),
        "remote direct refs are rewritten structurally; sources without spread provenance are not scanned"
    );
}

#[test]
fn rename_record_key_rolls_back_local_files_when_reference_write_fails() {
    let root = common::temp_project_dir("engine-rename-rollback-local");
    let _cleanup = common::TempDirCleanup(root.clone());
    write_local_rollback_project(&root);
    let mut registry = coflow_builtins::default_provider_registry().expect("default registry");
    registry
        .register_source_provider(FakeLocalFailLoader)
        .expect("register fake local loader");
    registry
        .register_source_writer(FakeLocalFailWriter)
        .expect("register fake local writer");

    let project = Project::open_schema_only(Some(&root.join("coflow.yaml"))).expect("open project");
    let mut session = build_project_session_for_build(project, &registry).expect("build session");
    assert!(
        !session.has_diagnostics(),
        "diagnostics before rename: {:?}",
        session.diagnostics.as_set()
    );

    let err = session
        .rename_record_key(&registry, "Item", "sword", "blade")
        .expect_err("fake local writer should fail");
    assert!(
        err.iter()
            .any(|diagnostic| diagnostic.code == "FAKE-LOCAL-WRITE"),
        "expected fake writer diagnostic: {err:?}"
    );
    assert!(
        !err.iter()
            .any(|diagnostic| diagnostic.code == "WRITE-ROLLBACK"),
        "rollback itself should succeed: {err:?}"
    );

    let items = std::fs::read_to_string(root.join("data/items.cfd")).expect("read items");
    assert!(
        items.contains("sword: Item"),
        "target rename should have been rolled back:\n{items}"
    );
    assert!(
        !items.contains("blade: Item"),
        "rolled back source should not keep new key:\n{items}"
    );
    assert!(
        session.records.get_by_coordinate("Item", "sword").is_some(),
        "failed rename should leave current session on old coordinate"
    );
    assert!(
        session.records.get_by_coordinate("Item", "blade").is_none(),
        "failed rename should not update current session"
    );
}

fn write_rename_reference_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r"
        type Item {
            name: string;
        }

        type Bundle {
            item: &Item;
            backup: &Item;
            path_name: string;
        }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data/items.cfd"),
        r#"sword: Item {
    name: "Sword",
}

base_bundle: Bundle {
    item: &sword,
    backup: &sword,
    path_name: "sword name",
}
"#,
    )
    .expect("write cfd source");
    std::fs::write(
        root.join("data/bundles.csv"),
        "id,item,backup,path_name\nstarter,&sword,&sword,sword name\n",
    )
    .expect("write csv source");
    std::fs::write(
        root.join("data/spread.cfd"),
        r"spread_bundle: Bundle {
    ...&base_bundle,
}
",
    )
    .expect("write spread source");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/main.cft
sources:
  - path: data/items.cfd
  - path: data/bundles.csv
    type: csv
    sheets:
      - sheet: bundles
        type: Bundle
  - path: data/spread.cfd
",
    )
    .expect("write config");
}

fn write_local_rollback_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r"
        type Item {
            name: string;
        }

        type Bundle {
            item: &Item;
        }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data/items.cfd"),
        r#"sword: Item {
    name: "Sword",
}
"#,
    )
    .expect("write items");
    std::fs::write(root.join("data/failing.fake"), "starter,&sword\n")
        .expect("write fake source");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data/items.cfd
  - path: data/failing.fake
    type: fake-local-fail
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
}

fn write_remote_rewrite_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r"
        type Item {
            name: string;
        }

        type Bundle {
            path_name: string;
        }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data/items.cfd"),
        r#"sword: Item {
    name: "Sword",
}
"#,
    )
    .expect("write items");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data/items.cfd
  - type: fake-remote
    url: fake-remote:bundle
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
}

#[derive(Debug, Default, Clone, Copy)]
struct FakeLocalFailLoader;

static FAKE_LOCAL_FAIL_LOADER_DESCRIPTOR: SourceProviderDescriptor = SourceProviderDescriptor {
    id: "fake-local-fail",
    display_name: "Fake local failing source",
    extensions: &[],
    uri_schemes: &[],
    option_keys: &[],
};

impl SourceProvider for FakeLocalFailLoader {
    fn descriptor(&self) -> &'static SourceProviderDescriptor {
        &FAKE_LOCAL_FAIL_LOADER_DESCRIPTOR
    }

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult {
        if source.source_type == Some("fake-local-fail") {
            ProbeResult::certain()
        } else {
            ProbeResult::none()
        }
    }

    fn load(
        &self,
        _ctx: SourceLoadContext<'_>,
        source: &ResolvedSource,
    ) -> Result<LoadedSource, DiagnosticSet> {
        let SourceLocationSpec::Path(path) = &source.location else {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "FAKE-LOCAL-LOAD",
                "TEST",
                "fake local source expects a path",
            )));
        };
        let mut field_columns = BTreeMap::new();
        field_columns.insert(vec!["item".to_string()], 2);
        Ok(LoadedSource {
            records: vec![CfdInputRecord::new(
                "starter",
                "Bundle",
                [("item", CfdInputValue::record_ref("sword"))],
            )
            .with_origin(RecordOrigin::Table {
                document: SourceDocument::Local(path.clone()),
                sheet: "Bundle".to_string(),
                row: 1,
                id_column: 1,
                field_columns,
            })],
        })
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct FakeLocalFailWriter;

static FAKE_LOCAL_FAIL_WRITER_DESCRIPTOR: WriterDescriptor = WriterDescriptor {
    id: "fake-local-fail",
    display_name: "Fake local failing source",
    capabilities: WriterCapabilities {
        provider_id: String::new(),
        can_edit_field: true,
        can_edit_key: false,
        can_insert_record: false,
        can_delete_record: false,
        requires_full_refresh_after_write: true,
        is_remote: false,
    },
};

impl SourceWriter for FakeLocalFailWriter {
    fn descriptor(&self) -> &'static WriterDescriptor {
        &FAKE_LOCAL_FAIL_WRITER_DESCRIPTOR
    }

    fn write_field(
        &self,
        _ctx: WriteContext<'_>,
        _request: &WriteCellRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Err(DiagnosticSet::one(Diagnostic::error(
            "FAKE-LOCAL-WRITE",
            "WRITE",
            "intentional fake local write failure",
        )))
    }
}

#[derive(Debug, Default)]
struct RemoteRewriteTracker {
    renamed: Vec<(String, String)>,
    rewrites: Vec<RemoteRewriteCall>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteRewriteCall {
    source: String,
    old_key: String,
    new_key: String,
    target_count: usize,
}

#[derive(Debug, Default, Clone, Copy)]
struct FakeRemoteLoader;

static FAKE_REMOTE_LOADER_DESCRIPTOR: SourceProviderDescriptor = SourceProviderDescriptor {
    id: "fake-remote",
    display_name: "Fake remote",
    extensions: &[],
    uri_schemes: &["fake-remote"],
    option_keys: &[],
};

impl SourceProvider for FakeRemoteLoader {
    fn descriptor(&self) -> &'static SourceProviderDescriptor {
        &FAKE_REMOTE_LOADER_DESCRIPTOR
    }

    fn probe(&self, source: &ProjectSourceRef<'_>) -> ProbeResult {
        if source.source_type == Some("fake-remote") {
            ProbeResult::certain()
        } else {
            ProbeResult::none()
        }
    }

    fn load(
        &self,
        _ctx: SourceLoadContext<'_>,
        _source: &ResolvedSource,
    ) -> Result<LoadedSource, DiagnosticSet> {
        let mut field_columns = BTreeMap::new();
        field_columns.insert(vec!["path_name".to_string()], 2);
        Ok(LoadedSource {
            records: vec![CfdInputRecord::new(
                "remote_bundle",
                "Bundle",
                [("path_name", CfdInputValue::from("&sword"))],
            )
            .with_origin(RecordOrigin::Table {
                document: SourceDocument::Remote("fake-remote:bundle".to_string()),
                sheet: "Bundle".to_string(),
                row: 2,
                id_column: 1,
                field_columns,
            })],
        })
    }
}

#[derive(Debug, Clone)]
struct FakeRemoteWriter {
    tracker: Arc<Mutex<RemoteRewriteTracker>>,
}

static FAKE_REMOTE_WRITER_DESCRIPTOR: WriterDescriptor = WriterDescriptor {
    id: "fake-remote",
    display_name: "Fake remote",
    capabilities: WriterCapabilities {
        provider_id: String::new(),
        can_edit_field: true,
        can_edit_key: true,
        can_insert_record: true,
        can_delete_record: true,
        requires_full_refresh_after_write: true,
        is_remote: true,
    },
};

impl SourceWriter for FakeRemoteWriter {
    fn descriptor(&self) -> &'static WriterDescriptor {
        &FAKE_REMOTE_WRITER_DESCRIPTOR
    }

    fn write_field(
        &self,
        _ctx: WriteContext<'_>,
        _request: &WriteCellRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        Ok(WriteOutcome::default())
    }

    fn rename_record(
        &self,
        _ctx: WriteContext<'_>,
        request: &RenameRecordRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        self.tracker
            .lock()
            .unwrap()
            .renamed
            .push((request.old_key.to_string(), request.new_key.to_string()));
        Ok(WriteOutcome::default())
    }

    fn rewrite_record_references(
        &self,
        _ctx: WriteContext<'_>,
        request: &RewriteRecordReferencesRequest<'_>,
    ) -> Result<WriteOutcome, DiagnosticSet> {
        let SourceLocationSpec::Uri(uri) = &request.source.location else {
            return Err(DiagnosticSet::one(Diagnostic::error(
                "FAKE-REMOTE",
                "TEST",
                "fake remote rewrite should receive uri source",
            )));
        };
        self.tracker
            .lock()
            .unwrap()
            .rewrites
            .push(RemoteRewriteCall {
                source: uri.clone(),
                old_key: request.old_key.to_string(),
                new_key: request.new_key.to_string(),
                target_count: request.targets.len(),
            });
        Ok(WriteOutcome::default())
    }
}
