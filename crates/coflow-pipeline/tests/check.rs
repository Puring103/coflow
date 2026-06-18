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
fn check_project_defaults_excel_sheets_when_source_omits_sheet_config() {
    let root = temp_project_dir("coflow-pipeline-default-excel-sheets");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            type Item {
                name: string;
                check { name != ""; }
            }
        "#,
    )
    .expect("write schema");
    let workbook_path = root.join("data").join("configs.xlsx");
    write_item_workbook(&workbook_path, None).expect("write workbook");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/configs.xlsx
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
}

#[test]
fn check_project_loads_directory_sources_and_resolves_excel_cfd_refs_both_ways() {
    let root = temp_project_dir("coflow-pipeline-mixed-directory-source");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            type Item {
                name: string;
                linked_stage: Stage? = null;

                check {
                    name != "";
                    when linked_stage != null {
                        linked_stage.reward_item.id == id;
                    }
                }
            }

            type Stage {
                name: string;
                reward_item: Item;

                check {
                    name != "";
                    reward_item.id != "";
                }
            }
        "#,
    )
    .expect("write schema");
    write_item_workbook(
        &root.join("data").join("configs.xlsx"),
        Some("@Stage.forest"),
    )
    .expect("write workbook");
    std::fs::write(
        root.join("data").join("stages.cfd"),
        r#"
            forest: Stage {
                name: "Forest",
                reward_item: @Item.potion,
            }
        "#,
    )
    .expect("write cfd source");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - dir: data
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
}

#[test]
fn check_project_treats_source_extensions_case_sensitively() {
    let root = temp_project_dir("coflow-pipeline-source-extension-case");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data").join("dir")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            type Item {
                name: string;
            }
        "#,
    )
    .expect("write schema");
    write_item_workbook(&root.join("data").join("dir").join("CONFIGS.XLSX"), None)
        .expect("write uppercase workbook");
    std::fs::write(
        root.join("data").join("dir").join("IGNORED.CFD"),
        r#"
            ignored: Item {
                name: "Ignored",
            }
        "#,
    )
    .expect("write uppercase cfd source");
    std::fs::write(
        root.join("data").join("IGNORED.CFD"),
        r#"
            ignored_explicit: Item {
                name: "Ignored explicit",
            }
        "#,
    )
    .expect("write explicit uppercase cfd source");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"
            item_1: Item {
                name: "Item",
            }
        "#,
    )
    .expect("write lowercase cfd source");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - dir: data/dir
  - file: data/items.cfd
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));

    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/IGNORED.CFD
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config with explicit uppercase source");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");
    let outcome = check_project(&project).expect("check project");

    assert_diagnostic_message_contains(
        outcome,
        "source file `data/IGNORED.CFD` has unsupported extension",
    );
}

#[test]
fn check_project_accepts_file_field_that_points_to_a_directory_source() {
    let root = temp_project_dir("coflow-pipeline-file-directory-source");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { name: string; }\n",
    )
    .expect("write schema");
    write_item_workbook(&root.join("data").join("configs.xlsx"), None).expect("write workbook");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
}

#[test]
fn check_project_ignores_unsupported_files_inside_directory_sources() {
    let root = temp_project_dir("coflow-pipeline-directory-ignores-unsupported");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { name: string; }\n",
    )
    .expect("write schema");
    write_item_workbook(&root.join("data").join("configs.xlsx"), None).expect("write workbook");
    std::fs::write(root.join("data").join("notes.txt"), "ignored\n")
        .expect("write unsupported file");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - dir: data
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
}

#[test]
fn check_project_reports_source_with_both_file_and_dir() {
    let root = temp_project_dir("coflow-pipeline-source-both-file-and-dir");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(root.join("schema").join("main.cft"), "type Item {}\n").expect("write schema");
    write_item_workbook(&root.join("data").join("configs.xlsx"), None).expect("write workbook");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/configs.xlsx
    dir: data
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert_diagnostic_message_contains(
        outcome,
        "sources[0] must set exactly one of `file`, `dir`, or `lark_sheet`",
    );
}

#[test]
fn check_project_reports_explicit_file_with_unsupported_extension() {
    let root = temp_project_dir("coflow-pipeline-unsupported-file-source");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(root.join("schema").join("main.cft"), "type Item {}\n").expect("write schema");
    std::fs::write(root.join("data").join("notes.txt"), "not a data file\n")
        .expect("write unsupported source");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/notes.txt
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert_diagnostic_message_contains(
        outcome,
        "source file `data/notes.txt` has unsupported extension",
    );
}

#[test]
fn check_project_allows_mixed_directory_source_with_excel_sheets_config() {
    let root = temp_project_dir("coflow-pipeline-cfd-sheets-directory-source");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { name: string; }\n",
    )
    .expect("write schema");
    write_item_workbook(&root.join("data").join("configs.xlsx"), None).expect("write workbook");
    std::fs::write(
        root.join("data").join("extra.cfd"),
        r#"
            extra: Item {
                name: "Extra",
            }
        "#,
    )
    .expect("write cfd source");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - dir: data
    sheets:
      - sheet: Item
        columns:
          id: id
          name: name
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");
    let project = Project::open_schema_only(Some(root.as_path())).expect("open project");

    let outcome = check_project(&project).expect("check project");

    assert!(matches!(outcome, PipelineOutcome::Success(_)));
}
