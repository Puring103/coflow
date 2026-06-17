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
    std::fs::write(
        out_dir.join("coflow.data.manifest.json"),
        r#"["Item.json","RemovedTable.json","RemovedTable.msgpack"]"#,
    )
    .expect("write data manifest");
    std::fs::write(out_dir.join("README.txt"), "keep").expect("write sidecar");

    let outcome = export_project_data(&project, DataFormat::Json, ExportOptions::default())
        .expect("export data");

    let PipelineOutcome::Success(report) = outcome else {
        panic!("expected export success");
    };
    assert_eq!(report.format, DataFormat::Json);
    assert!(out_dir.join("Item.json").exists());
    assert!(!out_dir.join("RemovedTable.json").exists());
    assert!(!out_dir.join("RemovedTable.msgpack").exists());
    assert!(out_dir.join("README.txt").exists());
    let manifest =
        std::fs::read_to_string(out_dir.join("coflow.data.manifest.json")).expect("manifest");
    assert!(manifest.contains("Item.json"));
    assert!(!manifest.contains("RemovedTable"));
}

#[test]
fn export_project_data_rejects_unmanaged_generated_data_files() {
    let root = temp_project_dir("coflow-pipeline-export-rejects-unmanaged-data");
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
    std::fs::write(out_dir.join("manual.json"), "{}").expect("write unmanaged json");

    let outcome = export_project_data(&project, DataFormat::Json, ExportOptions::default())
        .expect("unmanaged generated-looking file should be reported as diagnostics");

    assert_diagnostic_message_contains(outcome, "unmanaged generated artifact");
    assert!(out_dir.join("manual.json").exists());
    assert!(!out_dir.join("Item.json").exists());
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
