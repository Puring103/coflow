#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

use coflow_pipeline::{
    build_project, check_project, export_project_data, generate_project_code, BuildOptions,
    CodegenOptions, CodegenTarget, DataFormat, ExportOptions, PipelineOutcome,
};
use coflow_project::Project;

#[test]
fn check_project_passes_for_rpg_example() {
    let project = Project::open_schema_only(Some(workspace_path("examples/rpg").as_path()))
        .expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
}

#[test]
fn check_project_returns_schema_diagnostics() {
    let root = temp_project_dir("coflow-pipeline-schema-diagnostics");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("bad.cft"),
        "type Broken { value: Missing; }\n",
    )
    .expect("write schema");
    std::fs::write(root.join("coflow.yaml"), "schema: schema/\nsources: []\n")
        .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    let PipelineOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("expected diagnostics");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("Missing")),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn check_project_validates_excel_sources_before_schema_diagnostics() {
    let root = temp_project_dir("coflow-pipeline-check-source-before-schema");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("bad.cft"),
        "type Broken { value: Missing; }\n",
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/missing.xlsx
    sheets:
      - sheet: Items
        type: Item
        columns:
          A: id
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let err = check_project(&project).expect_err("missing source should be validated first");

    assert!(err.contains("sources[0].file `data/missing.xlsx` does not exist"));
}

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
fn generate_project_code_writes_csharp_files() {
    let project = Project::open_schema_only(Some(workspace_path("examples/rpg").as_path()))
        .expect("open project");
    let out_dir = temp_project_dir("coflow-pipeline-csharp-codegen");
    let _cleanup = TempDirCleanup(out_dir.clone());

    let outcome = generate_project_code(
        &project,
        CodegenTarget::Csharp,
        CodegenOptions {
            out_dir: Some(out_dir.as_path()),
            namespace: Some("Game.Config"),
        },
    )
    .expect("generate csharp");

    let PipelineOutcome::Success(report) = outcome else {
        panic!("expected codegen success");
    };
    assert_eq!(report.target, CodegenTarget::Csharp);
    assert_eq!(report.dir, out_dir);
    let game_config = std::fs::read_to_string(out_dir.join("GameConfig.cs")).expect("GameConfig");
    assert!(game_config.contains("namespace Game.Config;"));
}

#[test]
fn generate_project_code_does_not_require_excel_sources() {
    let root = temp_project_dir("coflow-pipeline-codegen-missing-source");
    let _cleanup = TempDirCleanup(root.clone());
    write_project_with_missing_excel_source(&root, true);
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");
    let out_dir = root.join("generated").join("csharp");

    let outcome = generate_project_code(
        &project,
        CodegenTarget::Csharp,
        CodegenOptions {
            out_dir: Some(out_dir.as_path()),
            namespace: Some("Game.Config"),
        },
    )
    .expect("generate csharp");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
    assert!(out_dir.join("GameConfig.cs").exists());
}

#[test]
fn check_project_excel_open_diagnostic_contains_file_path() {
    let root = temp_project_dir("coflow-pipeline-excel-open-path");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { @id id: string; value: int; }\n",
    )
    .expect("write schema");
    std::fs::write(root.join("data").join("bad.xlsx"), "not an xlsx").expect("write bad xlsx");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/bad.xlsx
    sheets:
      - sheet: Items
        type: Item
        columns:
          A: id
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    let PipelineOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("expected diagnostics");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "EXCEL-OPEN"
                && diagnostic.path.ends_with("bad.xlsx")),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn export_project_data_requires_excel_sources() {
    let root = temp_project_dir("coflow-pipeline-export-missing-source");
    let _cleanup = TempDirCleanup(root.clone());
    write_project_with_missing_excel_source(&root, false);
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let err = export_project_data(&project, DataFormat::Json, ExportOptions { out_dir: None })
        .expect_err("missing source should be a stage validation error");

    assert!(err.contains("sources[0].file `data/missing.xlsx` does not exist"));
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

fn write_project_with_missing_excel_source(root: &std::path::Path, include_code_output: bool) {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { @id id: string; value: int; }\n",
    )
    .expect("write schema");
    let code_output = if include_code_output {
        "  code:\n    type: csharp\n    dir: generated/csharp\n    namespace: Game.Config\n"
    } else {
        ""
    };
    std::fs::write(
        root.join("coflow.yaml"),
        format!(
            r"schema: schema/
sources:
  - file: data/missing.xlsx
    sheets:
      - sheet: Items
        type: Item
        columns:
          A: id
outputs:
  data:
    type: json
    dir: generated/data
{code_output}"
        ),
    )
    .expect("write config");
}

fn temp_project_dir(name: &str) -> std::path::PathBuf {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    );
    let root = std::env::temp_dir().join(format!("{name}-{suffix}"));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean temp dir");
    }
    root
}

fn workspace_path(path: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(path)
}

struct TempDirCleanup(std::path::PathBuf);

impl Drop for TempDirCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
