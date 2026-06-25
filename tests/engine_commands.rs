#![allow(clippy::expect_used, clippy::panic)]

use coflow::commands::{check_project, CommandOutcome};
use coflow_engine::build_project_session;
use coflow_project::Project;

mod common;

#[test]
fn engine_builds_record_and_source_indexes() {
    let root = common::temp_project_dir("engine-indexes");
    let _cleanup = common::TempDirCleanup(root.clone());
    common::write_invalid_check_project(&root).expect("write project");
    let config = root.join("coflow.yaml");
    let project = Project::open_schema_only(Some(&config)).expect("open project");
    let registry = coflow_builtins::default_provider_registry().expect("default registry");

    let session = build_project_session(project, &registry).expect("build session");

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
        .get("item_1")
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
