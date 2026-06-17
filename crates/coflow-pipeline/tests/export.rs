#![allow(
    clippy::cast_possible_truncation,
    clippy::expect_used,
    clippy::format_push_string,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

mod common;
use common::*;

#[test]
fn export_project_data_writes_json_tables() {
    let project = Project::open_schema_only(Some(workspace_path("examples/rpg").as_path()))
        .expect("open project");
    let out_dir = temp_project_dir("coflow-pipeline-json-export");
    let _cleanup = TempDirCleanup(out_dir.clone());

    let outcome = export_project_data(
        &project,
        DataFormat::Json,
        ExportOptions {
            out_dir: Some(out_dir.as_path()),
        },
    )
    .expect("export data");

    let PipelineOutcome::Success(report) = outcome else {
        panic!("expected export success");
    };
    assert_eq!(report.format, DataFormat::Json);
    assert_eq!(report.dir, out_dir);
    assert!(out_dir.join("Item.json").exists());
    assert!(out_dir.join("DropTable.json").exists());
}

#[test]
fn export_project_data_removes_stale_generated_data_files() {
    let root = temp_project_dir("coflow-pipeline-export-removes-stale-data");
    let _cleanup = TempDirCleanup(root.clone());
    write_single_item_project(
        &root,
        OutputsConfig {
            data: Some(output_config("json", "generated/data", None)),
            code: None,
        },
    )
    .expect("write project");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");
    let out_dir = root.join("generated").join("data");
    std::fs::create_dir_all(&out_dir).expect("create output dir");
    std::fs::write(out_dir.join("RemovedTable.json"), "{}").expect("write stale json");
    std::fs::write(out_dir.join("RemovedTable.msgpack"), []).expect("write stale msgpack");
    std::fs::write(out_dir.join("README.txt"), "remove").expect("write sidecar");

    let outcome = export_project_data(&project, DataFormat::Json, ExportOptions::default())
        .expect("export data");

    let PipelineOutcome::Success(report) = outcome else {
        panic!("expected export success");
    };
    assert_eq!(report.format, DataFormat::Json);
    assert!(out_dir.join("Item.json").exists());
    assert!(!out_dir.join("RemovedTable.json").exists());
    assert!(!out_dir.join("RemovedTable.msgpack").exists());
    assert!(!out_dir.join("README.txt").exists());
    assert!(!out_dir.join("coflow.data.manifest.json").exists());
}

#[test]
fn export_project_data_takes_over_existing_generated_data_dir() {
    let root = temp_project_dir("coflow-pipeline-export-takes-over-data");
    let _cleanup = TempDirCleanup(root.clone());
    write_single_item_project(
        &root,
        OutputsConfig {
            data: Some(output_config("json", "generated/data", None)),
            code: None,
        },
    )
    .expect("write project");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");
    let out_dir = root.join("generated").join("data");
    std::fs::create_dir_all(&out_dir).expect("create output dir");
    std::fs::write(out_dir.join("manual.json"), "{}").expect("write existing json");

    let outcome = export_project_data(&project, DataFormat::Json, ExportOptions::default())
        .expect("export data");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
    assert!(!out_dir.join("manual.json").exists());
    assert!(out_dir.join("Item.json").exists());
}

#[test]
fn export_project_data_requires_matching_configured_format() {
    let (project, _cleanup) = schema_only_project_with_outputs(
        "coflow-pipeline-export-wrong-format",
        OutputsConfig {
            data: Some(output_config("json", "generated/data", None)),
            code: None,
        },
    );

    let outcome = export_project_data(&project, DataFormat::Messagepack, ExportOptions::default())
        .expect("messagepack export diagnostics");

    assert_diagnostic_message_contains(
        outcome,
        "outputs.data.type is `json`; required `messagepack`",
    );
}

#[test]
fn export_project_data_requires_excel_sources() {
    let root = temp_project_dir("coflow-pipeline-export-missing-source");
    let _cleanup = TempDirCleanup(root.clone());
    write_project_with_missing_excel_source(&root, false);
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = export_project_data(&project, DataFormat::Json, ExportOptions { out_dir: None })
        .expect("missing source diagnostics");

    assert_diagnostic_message_contains(
        outcome,
        "sources[0].file `data/missing.xlsx` does not exist",
    );
}

#[test]
fn export_project_data_rejects_project_root_output_dir_override() {
    let root = temp_project_dir("coflow-pipeline-export-root-output");
    let _cleanup = TempDirCleanup(root.clone());
    write_single_item_project(
        &root,
        OutputsConfig {
            data: Some(output_config("json", "generated/data", None)),
            code: None,
        },
    )
    .expect("write project");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = export_project_data(
        &project,
        DataFormat::Json,
        ExportOptions {
            out_dir: Some(root.as_path()),
        },
    )
    .expect("unsafe root output should be reported as diagnostics");

    assert_diagnostic_message_contains(outcome, "project root");
    assert!(root.join("coflow.yaml").exists());
    assert!(root.join("schema").join("main.cft").exists());
    assert!(root.join("data").join("configs.xlsx").exists());
}

#[test]
fn export_project_data_rejects_output_dir_containing_schema() {
    let root = temp_project_dir("coflow-pipeline-export-schema-output");
    let _cleanup = TempDirCleanup(root.clone());
    write_single_item_project(
        &root,
        OutputsConfig {
            data: Some(output_config("json", "schema", None)),
            code: None,
        },
    )
    .expect("write project");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = export_project_data(&project, DataFormat::Json, ExportOptions::default())
        .expect("unsafe schema output should be reported as diagnostics");

    assert_diagnostic_message_contains(outcome, "schema path");
    assert!(root.join("schema").join("main.cft").exists());
    assert!(!root.join("schema").join("Item.json").exists());
}
