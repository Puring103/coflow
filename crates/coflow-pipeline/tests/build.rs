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
fn build_project_reports_missing_data_output_and_wrong_code_output_type() {
    let (missing_data, _missing_data_cleanup) = schema_only_project_with_outputs(
        "coflow-pipeline-build-missing-data-output",
        OutputsConfig {
            data: None,
            code: Some(output_config(
                "csharp",
                "generated/csharp",
                Some("Game.Config"),
            )),
        },
    );
    let outcome =
        build_project(&missing_data, BuildOptions::default()).expect("missing data diagnostics");
    assert_diagnostic_message_contains(outcome, "missing outputs.data");

    let (wrong_code_type, _wrong_code_type_cleanup) = project_with_unvalidated_outputs(
        "coflow-pipeline-build-wrong-code-output-type",
        OutputsConfig {
            data: Some(output_config("json", "generated/data", None)),
            code: Some(output_config("java", "generated/java", Some("Game.Config"))),
        },
    );
    let outcome =
        build_project(&wrong_code_type, BuildOptions::default()).expect("wrong code diagnostics");
    assert_diagnostic_message_contains(outcome, "outputs.code.type is `java`; expected `csharp`");
}

#[test]
fn build_project_exports_data_and_code() {
    let project = Project::open_schema_only(Some(workspace_path("examples/rpg").as_path()))
        .expect("open project");
    let data_dir = temp_project_dir("coflow-pipeline-build-data");
    let code_dir = temp_project_dir("coflow-pipeline-build-code");
    let _data_cleanup = TempDirCleanup(data_dir.clone());
    let _code_cleanup = TempDirCleanup(code_dir.clone());

    let outcome = build_project(
        &project,
        BuildOptions {
            data_out_dir: Some(data_dir.as_path()),
            code_out_dir: Some(code_dir.as_path()),
            namespace: Some("Game.Config"),
        },
    )
    .expect("build project");

    let PipelineOutcome::Success(report) = outcome else {
        panic!("expected build success");
    };
    assert_eq!(report.data.format, DataFormat::Json);
    assert_eq!(report.data.dir, data_dir);
    assert!(report.code.is_some());
    assert!(data_dir.join("Item.json").exists());
    assert!(code_dir.join("GameConfig.cs").exists());
}

#[test]
fn rpg_example_covers_validation_heavy_game_config_tables() {
    let project = Project::open_schema_only(Some(workspace_path("examples/rpg").as_path()))
        .expect("open project");
    let data_dir = temp_project_dir("coflow-pipeline-rpg-complete-data");
    let code_dir = temp_project_dir("coflow-pipeline-rpg-complete-code");
    let _data_cleanup = TempDirCleanup(data_dir.clone());
    let _code_cleanup = TempDirCleanup(code_dir.clone());

    let outcome = build_project(
        &project,
        BuildOptions {
            data_out_dir: Some(data_dir.as_path()),
            code_out_dir: Some(code_dir.as_path()),
            namespace: Some("Game.Config"),
        },
    )
    .expect("build RPG example");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
    for table in [
        "Item",
        "Equipment",
        "Skill",
        "Buff",
        "Monster",
        "DropTable",
        "Stage",
        "Quest",
        "Shop",
        "Text",
    ] {
        assert!(
            data_dir.join(format!("{table}.json")).exists(),
            "missing exported table {table}"
        );
        assert!(
            code_dir.join(format!("{table}.cs")).exists(),
            "missing generated C# type {table}"
        );
    }
}

#[test]
fn build_project_with_data_only_output_does_not_generate_code() {
    let root = temp_project_dir("coflow-pipeline-build-data-only");
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
    let data_dir = root.join("out-data");

    let outcome = build_project(
        &project,
        BuildOptions {
            data_out_dir: Some(data_dir.as_path()),
            code_out_dir: None,
            namespace: None,
        },
    )
    .expect("build project");

    let PipelineOutcome::Success(report) = outcome else {
        panic!("expected build success");
    };
    assert!(report.code.is_none());
    assert!(data_dir.join("Item.json").exists());
    assert!(!root.join("generated").join("csharp").exists());
}

#[test]
fn build_project_reports_data_output_path_that_is_existing_file() {
    let root = temp_project_dir("coflow-pipeline-build-data-output-file");
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
    let data_out_file = root.join("not-a-dir");
    std::fs::write(&data_out_file, "already a file").expect("write output collision");

    let outcome = build_project(
        &project,
        BuildOptions {
            data_out_dir: Some(data_out_file.as_path()),
            code_out_dir: None,
            namespace: None,
        },
    )
    .expect("file output path should return artifact diagnostics");

    let PipelineOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("expected artifact diagnostics");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "ARTIFACT-001"
                && diagnostic.message.contains("not-a-dir")),
        "diagnostics: {diagnostics:?}"
    );
    assert!(!data_out_file.join("Item.json").exists());
}

#[test]
fn build_project_does_not_write_data_when_codegen_preflight_reports_diagnostics() {
    let root = temp_project_dir("coflow-pipeline-build-codegen-preflight-gates-data");
    let _cleanup = TempDirCleanup(root.clone());
    write_single_item_project(
        &root,
        OutputsConfig {
            data: Some(output_config("json", "generated/data", None)),
            code: Some(output_config(
                "csharp",
                "generated/csharp",
                Some("Game.1Bad"),
            )),
        },
    )
    .expect("write project");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");
    let data_dir = root.join("out-data");
    let code_dir = root.join("out-code");

    let outcome = build_project(
        &project,
        BuildOptions {
            data_out_dir: Some(data_dir.as_path()),
            code_out_dir: Some(code_dir.as_path()),
            namespace: None,
        },
    )
    .expect("build project should return codegen diagnostics");

    let PipelineOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("expected codegen diagnostics");
    };
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic
            .message
            .contains("invalid C# namespace `Game.1Bad`")),
        "diagnostics: {diagnostics:?}"
    );
    assert!(!data_dir.join("Item.json").exists());
    assert!(!code_dir.join("GameConfig.cs").exists());
}
