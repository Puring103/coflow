#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

mod common;
use common::*;

#[test]
fn pipeline_diagnostic_codes_cover_project_artifact_and_codegen_boundaries() {
    let (missing_data, _missing_data_cleanup) = schema_only_project_with_outputs(
        "coflow-pipeline-error-coverage-project",
        OutputsConfig {
            data: None,
            code: Some(output_config(
                "csharp",
                "generated/csharp",
                Some("Game.Config"),
            )),
        },
    );
    let project_diagnostics = build_project(&missing_data, BuildOptions::default())
        .expect("missing data output should return diagnostics");
    assert_single_plain_diagnostic(project_diagnostics, "PROJECT-001", "PROJECT");

    let root = temp_project_dir("coflow-pipeline-error-coverage-artifact");
    let _artifact_cleanup = TempDirCleanup(root.clone());
    write_single_item_project(
        &root,
        OutputsConfig {
            data: Some(output_config("json", "generated/data", None)),
            code: None,
        },
    )
    .expect("write project");
    let data_output = root.join("generated").join("data");
    std::fs::create_dir_all(root.join("generated")).expect("create generated dir");
    std::fs::write(&data_output, "not a dir").expect("write blocking file");
    let artifact_project = Project::open_schema_only(Some(root.as_path())).expect("open project");
    let artifact_diagnostics =
        build_project(&artifact_project, BuildOptions::default()).expect("artifact diagnostics");
    assert_single_plain_diagnostic(artifact_diagnostics, "ARTIFACT-001", "ARTIFACT");

    let (codegen_project, _codegen_cleanup) = schema_only_project_with_outputs(
        "coflow-pipeline-error-coverage-codegen",
        OutputsConfig {
            data: Some(output_config("json", "generated/data", None)),
            code: Some(output_config(
                "csharp",
                "generated/csharp",
                Some("Game.1Bad"),
            )),
        },
    );
    let codegen_diagnostics = generate_project_code(
        &codegen_project,
        CodegenTarget::Csharp,
        CodegenOptions::default(),
    )
    .expect("codegen diagnostics");
    assert_single_plain_diagnostic(codegen_diagnostics, "CODEGEN-CSHARP-001", "CODEGEN");

    let (valid_project, _valid_cleanup) = schema_only_project_with_outputs(
        "coflow-pipeline-error-coverage-valid",
        OutputsConfig {
            data: Some(output_config("json", "generated/data", None)),
            code: Some(output_config(
                "csharp",
                "generated/csharp",
                Some("Game.Config"),
            )),
        },
    );
    let valid = generate_project_code(
        &valid_project,
        CodegenTarget::Csharp,
        CodegenOptions::default(),
    )
    .expect("valid codegen should not error");
    assert!(matches!(valid, PipelineOutcome::Success(_)));
}

fn assert_single_plain_diagnostic<T>(outcome: PipelineOutcome<T>, code: &str, stage: &str) {
    let PipelineOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("expected {code} diagnostics");
    };
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.code == code
            && diagnostic.stage == stage
            && diagnostic.path.is_empty()
            && diagnostic.start_line == 0
            && diagnostic.end_character == 1),
        "diagnostics: {diagnostics:?}"
    );
}
