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
