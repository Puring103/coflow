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
fn build_project_generates_key_as_enum_from_loaded_ids() {
    let root = temp_project_dir("coflow-pipeline-key-as-enum");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            @keyAsEnum("GeneId")
            type GeneConfig {}
            type BioRemainsConfig {
                gene: GeneConfig?;
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
          gene_id: gene
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
    assert!(gene.contains("public GeneId Id { get; internal set; }"));
    let remains =
        std::fs::read_to_string(out_code.join("BioRemainsConfig.cs")).expect("BioRemainsConfig.cs");
    assert!(remains.contains("public GeneConfig? Gene { get; internal set; }"));
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

#[test]
fn build_project_reports_duplicate_key_as_enum_keys_before_codegen() {
    let root = temp_project_dir("coflow-pipeline-key-as-enum-duplicates");
    let _cleanup = TempDirCleanup(root.clone());
    write_key_as_enum_project(&root, &["Gene_Spore", "Gene_Spore", "Gene_Mating"])
        .expect("write project with duplicate keys");
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
    .expect("duplicate keys should be returned as diagnostics");

    let PipelineOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("expected duplicate key diagnostics");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == CfdErrorCode::DuplicateId.as_str()),
        "diagnostics: {diagnostics:?}"
    );
    assert!(!code_dir.exists());
}
