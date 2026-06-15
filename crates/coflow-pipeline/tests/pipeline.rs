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

use coflow_data_model::CfdErrorCode;
use coflow_pipeline::{
    build_project, check_project, export_project_data, generate_project_code, BuildOptions,
    CodegenOptions, CodegenTarget, DataFormat, ExportOptions, PipelineOutcome,
};
use coflow_project::{
    OutputConfig, OutputsConfig, Project, ProjectConfig, SchemaConfig, SheetConfig, SourceConfig,
};
use rust_xlsxwriter::Workbook;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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

    let outcome = check_project(&project).expect("missing source diagnostics");

    assert_diagnostic_message_contains(
        outcome,
        "sources[0].file `data/missing.xlsx` does not exist",
    );
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
            @keyAsEnum("GeneId")
            type GeneConfig {}
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
fn generate_project_code_reports_missing_or_incompatible_outputs() {
    let (missing_code, _missing_code_cleanup) = schema_only_project_with_outputs(
        "coflow-pipeline-codegen-missing-code-output",
        OutputsConfig {
            data: Some(output_config("json", "generated/data", None)),
            code: None,
        },
    );
    let outcome = generate_project_code(
        &missing_code,
        CodegenTarget::Csharp,
        CodegenOptions::default(),
    )
    .expect("missing code output diagnostics");
    assert_diagnostic_message_contains(outcome, "missing outputs.code");

    let (missing_data, _missing_data_cleanup) = schema_only_project_with_outputs(
        "coflow-pipeline-codegen-missing-data-output",
        OutputsConfig {
            data: None,
            code: Some(output_config("csharp", "generated/csharp", None)),
        },
    );
    let outcome = generate_project_code(
        &missing_data,
        CodegenTarget::Csharp,
        CodegenOptions::default(),
    )
    .expect("missing data output diagnostics");
    assert_diagnostic_message_contains(outcome, "missing outputs.data");
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
fn codegen_reports_malformed_enum_lockfile_before_overwriting_it() {
    let root = temp_project_dir("coflow-pipeline-codegen-bad-lockfile");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("generated").join("csharp")).expect("create code dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            @keyAsEnum("GeneId")
            type GeneConfig {}
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
    std::fs::write(
        root.join("generated")
            .join("csharp")
            .join("coflow.enum.lock.json"),
        "{bad json",
    )
    .expect("write malformed lockfile");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let err = generate_project_code(&project, CodegenTarget::Csharp, CodegenOptions::default())
        .expect_err("malformed enum lockfile should fail");

    assert!(err.contains("failed to parse"));
    assert!(err.contains("coflow.enum.lock.json"));
}

#[test]
fn codegen_preflight_reports_multiple_diagnostics_before_lockfile_or_writes() {
    let root = temp_project_dir("coflow-pipeline-codegen-preflight");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("generated").join("csharp")).expect("create code dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            type FooBar { value: int; }
            @keyAsEnum("GeneId")
            type Foo_Bar {
                foo_bar: int;
                fooBar: int;
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
    namespace: Game.1Bad
",
    )
    .expect("write config");
    std::fs::write(
        root.join("generated")
            .join("csharp")
            .join("coflow.enum.lock.json"),
        "{bad json",
    )
    .expect("write malformed lockfile");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = generate_project_code(&project, CodegenTarget::Csharp, CodegenOptions::default())
        .expect("codegen preflight diagnostics should not read malformed lockfile");

    let PipelineOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("expected codegen diagnostics");
    };
    let messages = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("invalid C# namespace `Game.1Bad`")),
        "messages: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("generated C# file name `FooBar.cs` collides")),
        "messages: {messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|message| message.contains("generated C# member name `FooBar` collides")),
        "messages: {messages:?}"
    );
    assert!(!root
        .join("generated")
        .join("csharp")
        .join("GameConfig.cs")
        .exists());
    assert_eq!(
        std::fs::read_to_string(
            root.join("generated")
                .join("csharp")
                .join("coflow.enum.lock.json")
        )
        .expect("lockfile should remain readable"),
        "{bad json"
    );
}

#[test]
fn check_project_excel_open_diagnostic_contains_file_path() {
    let root = temp_project_dir("coflow-pipeline-excel-open-path");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { value: int; }\n",
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
fn check_project_returns_check_diagnostics_after_successful_load() {
    let root = temp_project_dir("coflow-pipeline-check-diagnostics");
    let _cleanup = TempDirCleanup(root.clone());
    write_invalid_check_project(&root).expect("write project");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    let PipelineOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("expected check diagnostics");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "CFD-CHECK-001"
                && diagnostic.stage == "CHECK"
                && diagnostic.path.ends_with("configs.xlsx")
                && diagnostic.sheet.as_deref() == Some("Item")
                && diagnostic.cell.as_deref() == Some("B2")),
        "diagnostics: {diagnostics:?}"
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

#[test]
fn generate_project_code_uses_messagepack_data_output_config() {
    let root = temp_project_dir("coflow-pipeline-codegen-messagepack");
    let _cleanup = TempDirCleanup(root.clone());
    write_single_item_project(
        &root,
        OutputsConfig {
            data: Some(output_config("messagepack", "generated/data", None)),
            code: Some(output_config(
                "csharp",
                "generated/csharp",
                Some("Game.Config"),
            )),
        },
    )
    .expect("write project");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = generate_project_code(&project, CodegenTarget::Csharp, CodegenOptions::default())
        .expect("messagepack codegen");

    let PipelineOutcome::Success(report) = outcome else {
        panic!("expected codegen success");
    };
    assert!(report.dir.ends_with(Path::new("generated").join("csharp")));
    let game_config =
        std::fs::read_to_string(report.dir.join("GameConfig.cs")).expect("GameConfig.cs");
    assert!(game_config.contains("MessagePack"));
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

#[test]
fn codegen_writes_empty_key_as_enum_lockfile_when_only_declared_ids_exist() {
    let (declared_only, _declared_only_cleanup) = schema_only_project_with_outputs(
        "coflow-pipeline-key-as-enum-declared-only",
        OutputsConfig {
            data: Some(output_config("json", "generated/data", None)),
            code: Some(output_config(
                "csharp",
                "generated/csharp",
                Some("Game.Config"),
            )),
        },
    );
    let outcome = generate_project_code(
        &declared_only,
        CodegenTarget::Csharp,
        CodegenOptions::default(),
    )
    .expect("declared enum without loaded ids should still write empty lockfile");
    assert!(matches!(outcome, PipelineOutcome::Success(_)));
    let lockfile = std::fs::read_to_string(
        declared_only
            .root_dir
            .join("generated")
            .join("csharp")
            .join("coflow.enum.lock.json"),
    )
    .expect("enum lockfile");
    assert!(lockfile.contains("\"GeneId\": {}"));
}

fn write_project_with_missing_excel_source(root: &Path, include_code_output: bool) {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { value: int; }\n",
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

fn write_single_item_project(
    root: &Path,
    outputs: OutputsConfig,
) -> Result<(), rust_xlsxwriter::XlsxError> {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { value: int; }\n",
    )
    .expect("write schema");
    let workbook_path = root.join("data").join("configs.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("Item")?;
    sheet.write_string(0, 0, "id")?;
    sheet.write_string(0, 1, "value")?;
    sheet.write_string(1, 0, "item_1")?;
    sheet.write_number(1, 1, 1.0)?;
    workbook.save(&workbook_path)?;

    let mut config = String::from(
        r"schema: schema/
sources:
  - file: data/configs.xlsx
    sheets:
      - sheet: Item
        columns:
          id: id
          value: value
outputs:
",
    );
    if let Some(data) = outputs.data {
        config.push_str(&format!(
            "  data:\n    type: {}\n    dir: {}\n",
            data.output_type,
            data.dir.display()
        ));
    }
    if let Some(code) = outputs.code {
        config.push_str(&format!(
            "  code:\n    type: {}\n    dir: {}\n",
            code.output_type,
            code.dir.display()
        ));
        if let Some(namespace) = code.namespace {
            config.push_str(&format!("    namespace: {namespace}\n"));
        }
    }
    std::fs::write(root.join("coflow.yaml"), config).expect("write config");
    Ok(())
}

fn write_invalid_check_project(root: &Path) -> Result<(), rust_xlsxwriter::XlsxError> {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            type Item {
                level: int;
                check { level > 0; }
            }
        "#,
    )
    .expect("write schema");
    let workbook_path = root.join("data").join("configs.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("Item")?;
    sheet.write_string(0, 0, "id")?;
    sheet.write_string(0, 1, "level")?;
    sheet.write_string(1, 0, "item_1")?;
    sheet.write_number(1, 1, 0.0)?;
    workbook.save(&workbook_path)?;
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/configs.xlsx
    sheets:
      - sheet: Item
        columns:
          id: id
          level: level
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    Ok(())
}

fn schema_only_project_with_outputs(
    name: &str,
    outputs: OutputsConfig,
) -> (Project, TempDirCleanup) {
    let root = temp_project_dir(name);
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            @keyAsEnum("GeneId")
            type GeneConfig {}
        "#,
    )
    .expect("write schema");
    let project = Project {
        config_path: root.join("coflow.yaml"),
        root_dir: root.clone(),
        config: ProjectConfig {
            schema: SchemaConfig::One(PathBuf::from("schema")),
            sources: Vec::new(),
            outputs,
        },
    };
    (project, TempDirCleanup(root))
}

fn project_with_unvalidated_outputs(
    name: &str,
    outputs: OutputsConfig,
) -> (Project, TempDirCleanup) {
    let root = temp_project_dir(name);
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(root.join("schema").join("main.cft"), "type Item {}\n").expect("write schema");
    let workbook_path = root.join("data").join("configs.xlsx");
    let mut workbook = Workbook::new();
    let sheet = workbook.add_worksheet();
    sheet.set_name("Item").expect("set sheet name");
    sheet.write_string(0, 0, "id").expect("write header");
    sheet.write_string(1, 0, "item_1").expect("write row");
    workbook.save(&workbook_path).expect("save workbook");
    let project = Project {
        config_path: root.join("coflow.yaml"),
        root_dir: root.clone(),
        config: ProjectConfig {
            schema: SchemaConfig::One(PathBuf::from("schema")),
            sources: vec![SourceConfig {
                file: PathBuf::from("data/configs.xlsx"),
                sheets: vec![SheetConfig {
                    sheet: "Item".to_string(),
                    type_name: None,
                    columns: BTreeMap::from([("id".to_string(), "id".to_string())]),
                }],
            }],
            outputs,
        },
    };
    (project, TempDirCleanup(root))
}

fn output_config(output_type: &str, dir: &str, namespace: Option<&str>) -> OutputConfig {
    OutputConfig {
        output_type: output_type.to_string(),
        dir: PathBuf::from(dir),
        namespace: namespace.map(str::to_string),
    }
}

fn assert_diagnostic_message_contains<T>(outcome: PipelineOutcome<T>, expected: &str) {
    let PipelineOutcome::Diagnostics(diagnostics) = outcome else {
        panic!("expected diagnostics containing `{expected}`");
    };
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains(expected)),
        "missing `{expected}` in diagnostics: {diagnostics:?}"
    );
}

fn write_key_as_enum_project(
    root: &Path,
    gene_ids: &[&str],
) -> Result<(), rust_xlsxwriter::XlsxError> {
    assert!(!gene_ids.is_empty(), "test requires at least one gene id");
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
    remains.write_string(1, 1, format!("@GeneConfig.{}", gene_ids[0]))?;

    workbook.save(workbook_path)
}

fn write_renamable_key_as_enum_project(
    root: &Path,
    enum_name: &str,
) -> Result<(), rust_xlsxwriter::XlsxError> {
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        format!(
            r#"
            @keyAsEnum("{enum_name}")
            type GeneConfig {{}}
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

fn temp_project_dir(name: &str) -> PathBuf {
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

fn workspace_path(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join(path)
}

fn write_key_as_enum_workbook(path: &Path) -> Result<(), rust_xlsxwriter::XlsxError> {
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
    remains.write_string(1, 1, "@GeneConfig.Gene_Spore")?;

    workbook.save(path)
}

struct TempDirCleanup(PathBuf);

impl Drop for TempDirCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
