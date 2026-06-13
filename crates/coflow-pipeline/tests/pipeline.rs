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
use rust_xlsxwriter::Workbook;

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
    assert!(game_config
        .replace("\r\n", "\n")
        .contains("namespace Game.Config\n{"));
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
fn generate_project_code_writes_key_as_enum_lockfile_for_declared_enums() {
    let root = temp_project_dir("coflow-pipeline-codegen-key-as-enum-lockfile");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            type GeneConfig {
                @IdAsEnum("GeneId")
                @id
                id: string;
            }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");
    let out_dir = root.join("out-code");

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
    let lockfile =
        std::fs::read_to_string(out_dir.join("coflow.enum.lock.json")).expect("enum lockfile");
    assert!(lockfile.contains("\"GeneId\": {}"));
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

#[test]
fn build_project_generates_key_as_enum_from_loaded_ids() {
    let root = temp_project_dir("coflow-pipeline-key-as-enum");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            type GeneConfig {
                @IdAsEnum("GeneId")
                @id
                id: string;
            }
            type BioRemainsConfig {
                @id id: string;
                @ref(GeneConfig)
                gene_id: string?;
            }
        "#,
    )
    .expect("write schema");
    let workbook_path = root.join("data").join("configs.xlsx");
    write_key_as_enum_workbook(&workbook_path).expect("write workbook");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/configs.xlsx
    sheets:
      - sheet: GeneConfig
        columns:
          id: id
      - sheet: BioRemainsConfig
        columns:
          id: id
          gene_id: gene_id
outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = build_project(
        &project,
        BuildOptions {
            data_out_dir: Some(root.join("out-data").as_path()),
            code_out_dir: Some(root.join("out-code").as_path()),
            namespace: Some("Game.Config"),
        },
    )
    .expect("build project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
    let out_code = root.join("out-code");
    let gene_id = std::fs::read_to_string(out_code.join("GeneId.cs")).expect("GeneId.cs");
    assert!(gene_id.contains("public enum GeneId"));
    assert!(gene_id.contains("Gene_Spore = 0"));
    assert!(gene_id.contains("Gene_Mating = 1"));
    assert!(!out_code.join("GeneConfigId.cs").exists());
    let gene = std::fs::read_to_string(out_code.join("GeneConfig.cs")).expect("GeneConfig.cs");
    assert!(gene.contains("public GeneId Id { get; set; }"));
    let remains =
        std::fs::read_to_string(out_code.join("BioRemainsConfig.cs")).expect("BioRemainsConfig.cs");
    assert!(remains.contains("public GeneId? GeneId { get; set; }"));
}

#[test]
fn build_project_writes_key_as_enum_lockfile() {
    let root = temp_project_dir("coflow-pipeline-key-as-enum-lockfile");
    let _cleanup = TempDirCleanup(root.clone());
    write_key_as_enum_project(&root, &["Gene_Spore", "Gene_Mating"]).expect("write project");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");
    let data_dir = root.join("out-data");
    let code_dir = root.join("out-code");

    let outcome = build_project(
        &project,
        BuildOptions {
            data_out_dir: Some(data_dir.as_path()),
            code_out_dir: Some(code_dir.as_path()),
            namespace: Some("Game.Config"),
        },
    )
    .expect("build project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
    let lockfile =
        std::fs::read_to_string(code_dir.join("coflow.enum.lock.json")).expect("enum lockfile");
    assert!(lockfile.contains("\"GeneId\""));
    assert!(lockfile.contains("\"Gene_Spore\": 0"));
    assert!(lockfile.contains("\"Gene_Mating\": 1"));
    let gene_id = std::fs::read_to_string(code_dir.join("GeneId.cs")).expect("GeneId.cs");
    assert!(gene_id.contains("Gene_Spore = 0"));
    assert!(gene_id.contains("Gene_Mating = 1"));
}

#[test]
fn build_project_preserves_key_as_enum_lockfile_values_and_appends_new_ids() {
    let root = temp_project_dir("coflow-pipeline-key-as-enum-lockfile-stable");
    let _cleanup = TempDirCleanup(root.clone());
    write_key_as_enum_project(&root, &["Gene_Spore", "Gene_Mating"])
        .expect("write initial project");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");
    let data_dir = root.join("out-data");
    let code_dir = root.join("out-code");

    build_project(
        &project,
        BuildOptions {
            data_out_dir: Some(data_dir.as_path()),
            code_out_dir: Some(code_dir.as_path()),
            namespace: Some("Game.Config"),
        },
    )
    .expect("initial build");

    write_key_as_enum_project(&root, &["Gene_Mating", "Gene_New", "Gene_Spore"])
        .expect("rewrite project with reordered ids");
    let project = Project::open_schema_only(Some(root.as_path())).expect("reopen project");
    build_project(
        &project,
        BuildOptions {
            data_out_dir: Some(data_dir.as_path()),
            code_out_dir: Some(code_dir.as_path()),
            namespace: Some("Game.Config"),
        },
    )
    .expect("second build");

    let gene_id = std::fs::read_to_string(code_dir.join("GeneId.cs")).expect("GeneId.cs");
    assert!(gene_id.contains("Gene_Spore = 0"));
    assert!(gene_id.contains("Gene_Mating = 1"));
    assert!(gene_id.contains("Gene_New = 2"));
    let lockfile =
        std::fs::read_to_string(code_dir.join("coflow.enum.lock.json")).expect("enum lockfile");
    assert!(lockfile.contains("\"Gene_Spore\": 0"));
    assert!(lockfile.contains("\"Gene_Mating\": 1"));
    assert!(lockfile.contains("\"Gene_New\": 2"));
}

#[test]
fn build_project_removes_stale_generated_csharp_files_after_key_as_enum_rename() {
    let root = temp_project_dir("coflow-pipeline-stale-csharp-cleanup");
    let _cleanup = TempDirCleanup(root.clone());
    write_renamable_key_as_enum_project(&root, "OldGeneId").expect("write initial project");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");
    let data_dir = root.join("out-data");
    let code_dir = root.join("out-code");

    build_project(
        &project,
        BuildOptions {
            data_out_dir: Some(data_dir.as_path()),
            code_out_dir: Some(code_dir.as_path()),
            namespace: Some("Game.Config"),
        },
    )
    .expect("initial build");
    assert!(code_dir.join("OldGeneId.cs").exists());

    write_renamable_key_as_enum_project(&root, "NewGeneId").expect("rewrite project");
    let project = Project::open_schema_only(Some(root.as_path())).expect("reopen project");
    build_project(
        &project,
        BuildOptions {
            data_out_dir: Some(data_dir.as_path()),
            code_out_dir: Some(code_dir.as_path()),
            namespace: Some("Game.Config"),
        },
    )
    .expect("second build");

    assert!(code_dir.join("NewGeneId.cs").exists());
    assert!(!code_dir.join("OldGeneId.cs").exists());
    let lockfile =
        std::fs::read_to_string(code_dir.join("coflow.enum.lock.json")).expect("enum lockfile");
    assert!(lockfile.contains("\"NewGeneId\""));
    assert!(!lockfile.contains("\"OldGeneId\""));
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

fn write_key_as_enum_project(
    root: &std::path::Path,
    gene_ids: &[&str],
) -> Result<(), rust_xlsxwriter::XlsxError> {
    assert!(!gene_ids.is_empty(), "test requires at least one gene id");
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            type GeneConfig {
                @IdAsEnum("GeneId")
                @id
                id: string;
            }
            type BioRemainsConfig {
                @id id: string;
                @ref(GeneConfig)
                gene_id: string?;
            }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/configs.xlsx
    sheets:
      - sheet: GeneConfig
        columns:
          id: id
      - sheet: BioRemainsConfig
        columns:
          id: id
          gene_id: gene_id
outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
",
    )
    .expect("write config");

    let workbook_path = root.join("data").join("configs.xlsx");
    if workbook_path.exists() {
        std::fs::remove_file(&workbook_path).expect("remove old workbook");
    }

    let mut workbook = Workbook::new();
    let genes = workbook.add_worksheet();
    genes.set_name("GeneConfig")?;
    genes.write_string(0, 0, "id")?;
    for (index, id) in gene_ids.iter().enumerate() {
        genes.write_string((index + 1) as u32, 0, *id)?;
    }

    let remains = workbook.add_worksheet();
    remains.set_name("BioRemainsConfig")?;
    remains.write_string(0, 0, "id")?;
    remains.write_string(0, 1, "gene_id")?;
    remains.write_string(1, 0, "remains_1")?;
    remains.write_string(1, 1, gene_ids[0])?;

    workbook.save(workbook_path)
}

fn write_renamable_key_as_enum_project(
    root: &std::path::Path,
    enum_name: &str,
) -> Result<(), rust_xlsxwriter::XlsxError> {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        format!(
            r#"
            type GeneConfig {{
                @IdAsEnum("{enum_name}")
                @id
                id: string;
            }}
        "#
        ),
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/configs.xlsx
    sheets:
      - sheet: GeneConfig
        columns:
          id: id
outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
",
    )
    .expect("write config");

    let workbook_path = root.join("data").join("configs.xlsx");
    if workbook_path.exists() {
        std::fs::remove_file(&workbook_path).expect("remove old workbook");
    }
    let mut workbook = Workbook::new();
    let genes = workbook.add_worksheet();
    genes.set_name("GeneConfig")?;
    genes.write_string(0, 0, "id")?;
    genes.write_string(1, 0, "Gene_Spore")?;
    workbook.save(workbook_path)
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

fn write_key_as_enum_workbook(path: &std::path::Path) -> Result<(), rust_xlsxwriter::XlsxError> {
    let mut workbook = Workbook::new();
    let genes = workbook.add_worksheet();
    genes.set_name("GeneConfig")?;
    genes.write_string(0, 0, "id")?;
    genes.write_string(1, 0, "Gene_Spore")?;
    genes.write_string(2, 0, "Gene_Mating")?;

    let remains = workbook.add_worksheet();
    remains.set_name("BioRemainsConfig")?;
    remains.write_string(0, 0, "id")?;
    remains.write_string(0, 1, "gene_id")?;
    remains.write_string(1, 0, "remains_1")?;
    remains.write_string(1, 1, "Gene_Spore")?;

    workbook.save(path)
}

struct TempDirCleanup(std::path::PathBuf);

impl Drop for TempDirCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
