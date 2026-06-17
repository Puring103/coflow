#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

mod common;
use common::*;

#[test]
fn cft_check_uses_project_config_and_json_output() {
    let output = coflow()
        .args(["cft", "check", "examples/rpg", "--json"])
        .output()
        .expect("run coflow");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        r#"{"diagnostics":[]}"#
    );
}

#[test]
fn full_project_check_loads_example_excel() {
    let output = coflow()
        .args(["check", "examples/rpg"])
        .output()
        .expect("run coflow");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Project check passed"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn full_project_check_failure_uses_check_diagnostics_in_human_output() {
    let root = temp_project_dir("check-failure-human");
    let _cleanup = TempDirCleanup(root.clone());
    write_invalid_check_project(&root).expect("write invalid check project");

    let output = coflow()
        .args(["check", root.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow check");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[CFD-CHECK-001] [CHECK]"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("file    data/configs.xlsx"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("sheet   Item"), "stderr: {stderr}");
    assert!(stderr.contains("cell    B2"), "stderr: {stderr}");
    assert!(
        stderr.contains("message\n  check condition evaluated to false"),
        "stderr: {stderr}"
    );
    assert!(
        !stderr.contains(root.to_string_lossy().as_ref()),
        "stderr should use project-relative paths: {stderr}"
    );
}

#[test]
fn full_project_check_failure_uses_check_diagnostics_in_json_output() {
    let root = temp_project_dir("check-failure-json");
    let _cleanup = TempDirCleanup(root.clone());
    write_invalid_check_project(&root).expect("write invalid check project");

    let output = coflow()
        .args(["check", root.to_str().expect("utf8 temp path"), "--json"])
        .output()
        .expect("run coflow check");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("diagnostics json");
    let diagnostics = json["diagnostics"].as_array().expect("diagnostics array");
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic["code"], "CFD-CHECK-001");
    assert_eq!(diagnostic["stage"], "CHECK");
    assert!(
        diagnostic["path"]
            .as_str()
            .is_some_and(|path| path.ends_with("configs.xlsx")),
        "diagnostic: {diagnostic:?}"
    );
    assert_eq!(diagnostic["sheet"], "Item");
    assert_eq!(diagnostic["cell"], "B2");
}

#[test]
fn config_validation_rejects_unknown_fields_and_invalid_outputs() {
    let suffix = unique_suffix();
    let root_dir = std::env::temp_dir().join(format!("coflow-config-validation-test-{suffix}"));
    let project_dir = root_dir.join("project");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old temp dir");
    }
    std::fs::create_dir_all(project_dir.join("schema")).expect("create schema dir");
    std::fs::write(
        project_dir.join("schema").join("main.cft"),
        "type Item { key: string; }\n",
    )
    .expect("write schema");
    std::fs::write(
        project_dir.join("coflow.yaml"),
        "schema: schema/\nunknown: true\noutputs:\n  data:\n    type: yaml\n    dir: generated/data\n",
    )
    .expect("write config");

    let output = coflow()
        .args(["check", project_dir.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow check");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unknown field"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::write(
        project_dir.join("coflow.yaml"),
        "schema: schema/\noutputs:\n  data:\n    type: yaml\n    dir: generated/data\n  code:\n    type: python\n    dir: generated/code\n",
    )
    .expect("write config");
    let output = coflow()
        .args(["check", project_dir.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow check");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("outputs.data.type is `yaml`; expected `json` or `messagepack`"),
        "stderr: {stderr}"
    );

    std::fs::remove_dir_all(root_dir).expect("clean temp dir");
}

#[test]
fn config_validation_collects_multiple_project_diagnostics() {
    let root = temp_project_dir("config-multiple-diagnostics");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { key: string; }\n",
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/
sources:
  - file: data/missing.xlsx
    dir: data
outputs:
  data:
    type: yaml
    dir: ""
    namespace: Bad.Data
  code:
    type: python
    dir: ""
    namespace: ""
"#,
    )
    .expect("write config");

    let output = coflow()
        .args(["check", root.to_str().expect("utf8 temp path"), "--json"])
        .output()
        .expect("run coflow check");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(stdout.trim()).expect("diagnostics json");
    let diagnostics = json["diagnostics"].as_array().expect("diagnostics array");
    for expected in [
        "sources[0] must set exactly one of `file` or `dir`",
        "outputs.data.type is `yaml`; expected `json` or `messagepack`",
        "outputs.data.dir is empty",
        "outputs.data.namespace is only valid for code outputs",
        "outputs.code.type is `python`; expected `csharp`",
        "outputs.code.dir is empty",
        "outputs.code.namespace is empty",
    ] {
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic["message"].as_str() == Some(expected)),
            "missing `{expected}` in diagnostics: {diagnostics:?}"
        );
    }
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic["stage"].as_str() == Some("PROJECT")),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn schema_path_validation_collects_multiple_missing_paths() {
    let root = temp_project_dir("schema-multiple-missing-paths");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(&root).expect("create project dir");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema:
  - missing-a.cft
  - missing-b/
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");

    let output = coflow()
        .args([
            "cft",
            "check",
            root.to_str().expect("utf8 temp path"),
            "--json",
        ])
        .output()
        .expect("run coflow cft check");

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: Value = serde_json::from_str(stdout.trim()).expect("diagnostics json");
    let diagnostics = json["diagnostics"].as_array().expect("diagnostics array");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["message"].as_str()
                == Some("schema[0] path `missing-a.cft` does not exist")),
        "diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["message"].as_str()
                == Some("schema[1] path `missing-b/` does not exist")),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn config_validation_rejects_invalid_sources_and_sheets() {
    let suffix = unique_suffix();
    let root_dir =
        std::env::temp_dir().join(format!("coflow-config-source-validation-test-{suffix}"));
    let project_dir = root_dir.join("project");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old temp dir");
    }
    std::fs::create_dir_all(project_dir.join("schema")).expect("create schema dir");
    std::fs::write(
        project_dir.join("schema").join("main.cft"),
        "type Item { key: string; }\n",
    )
    .expect("write schema");
    std::fs::write(
        project_dir.join("coflow.yaml"),
        "schema: schema/\nsources:\n  - file: data/missing.xlsx\n    sheets: []\n",
    )
    .expect("write config");

    let output = coflow()
        .args(["check", project_dir.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow check");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("sources[0].file `data/missing.xlsx` does not exist"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::create_dir_all(project_dir.join("data")).expect("create data dir");
    std::fs::write(project_dir.join("data").join("missing.xlsx"), "").expect("write placeholder");
    std::fs::write(
        project_dir.join("coflow.yaml"),
        "schema: schema/\nsources:\n  - file: data/missing.xlsx\n    sheets:\n      - sheet: \"\"\n        columns:\n          A: id\n",
    )
    .expect("write config");

    let output = coflow()
        .args(["check", project_dir.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow check");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("sources[0].sheets[0].sheet is empty"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::remove_dir_all(root_dir).expect("clean temp dir");
}

#[test]
fn config_validation_rejects_duplicate_column_keys() {
    let root = temp_project_dir("duplicate-column-keys");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { key: string; }\n",
    )
    .expect("write schema");
    std::fs::write(root.join("data").join("items.xlsx"), "").expect("write placeholder");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/items.xlsx
    sheets:
      - sheet: Items
        columns:
          A: id
          A: name
",
    )
    .expect("write config");

    let output = coflow()
        .args(["check", root.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow check");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("duplicate columns key `A`"),
        "stderr: {stderr}"
    );
}

#[test]
fn schema_only_commands_do_not_require_excel_sources() {
    let root = temp_project_dir("schema-only-missing-source");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { value: int; }\n",
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
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
",
    )
    .expect("write config");

    let cft_check = coflow()
        .args(["cft", "check", root.to_str().expect("utf8 path")])
        .output()
        .expect("run cft check");
    assert!(
        cft_check.status.success(),
        "cft check should not require xlsx\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&cft_check.stdout),
        String::from_utf8_lossy(&cft_check.stderr)
    );

    let codegen_dir = root.join("generated").join("csharp");
    let codegen = coflow()
        .args([
            "codegen",
            "csharp",
            root.to_str().expect("utf8 path"),
            "--out",
            codegen_dir.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("run codegen");
    assert!(
        codegen.status.success(),
        "codegen should not require xlsx\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&codegen.stdout),
        String::from_utf8_lossy(&codegen.stderr)
    );
    assert!(codegen_dir.join("GameConfig.cs").exists());
}

#[test]
fn init_existing_config_has_no_side_effects() {
    let root = temp_project_dir("init-existing-config");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(&root).expect("create temp root");
    std::fs::write(root.join("coflow.yaml"), "schema: schema/\n").expect("write config");

    let output = coflow()
        .args(["init", root.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow init");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("coflow.yaml") && stderr.contains("already exists"),
        "stderr: {stderr}"
    );
    assert!(!root.join("schema").exists());
    assert!(!root.join("data").exists());
    assert!(!root.join("generated").exists());
}

#[test]
fn data_commands_require_excel_sources() {
    let root = temp_project_dir("data-missing-source");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { value: int; }\n",
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
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
",
    )
    .expect("write config");

    for args in [
        vec!["check", root.to_str().expect("utf8 path")],
        vec!["build", root.to_str().expect("utf8 path")],
        vec!["export", "json", root.to_str().expect("utf8 path")],
    ] {
        let output = coflow().args(args).output().expect("run data command");
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("sources[0].file `data/missing.xlsx` does not exist"),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn excel_cell_diagnostics_include_sheet_and_a1_cell_in_human_output() {
    let root = temp_project_dir("excel-diagnostic-location");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { level: int; }\n",
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/items.xlsx
    sheets:
      - sheet: Items
        type: Item
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");

    let xlsx_path = root.join("data").join("items.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Items")
        .expect("set sheet name");
    sheet.write_string(0, 0, "id").expect("write id header");
    sheet
        .write_string(0, 1, "level")
        .expect("write level header");
    sheet.write_string(1, 0, "item_1").expect("write id");
    sheet
        .write_string(1, 1, "not_int")
        .expect("write bad level");
    workbook.save(&xlsx_path).expect("write xlsx");

    let output = coflow()
        .args(["check", root.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow check");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[CELL-TypeMismatch] [CELL]"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("----------------------------------------"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("file    data/items.xlsx"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("sheet   Items"), "stderr: {stderr}");
    assert!(stderr.contains("cell    B2"), "stderr: {stderr}");
    assert!(
        stderr.contains("message\n  failed to parse `Item.level` cell: expected int"),
        "stderr: {stderr}"
    );
    assert!(
        !stderr.contains(root.to_string_lossy().as_ref()),
        "stderr should use project-relative paths: {stderr}"
    );
}

#[test]
fn excel_cell_diagnostics_collect_multiple_bad_cells() {
    let root = temp_project_dir("excel-multiple-bad-cells");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { level: int; }\n",
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/items.xlsx
    sheets:
      - sheet: Items
        type: Item
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");

    let xlsx_path = root.join("data").join("items.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Items")
        .expect("set sheet name");
    sheet.write_string(0, 0, "id").expect("write id header");
    sheet
        .write_string(0, 1, "level")
        .expect("write level header");
    sheet.write_string(1, 0, "item_1").expect("write id 1");
    sheet
        .write_string(1, 1, "bad_1")
        .expect("write bad level 1");
    sheet.write_string(2, 0, "item_2").expect("write id 2");
    sheet
        .write_string(2, 1, "bad_2")
        .expect("write bad level 2");
    workbook.save(&xlsx_path).expect("write xlsx");

    let output = coflow()
        .args(["check", root.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow check");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cell    B2"), "stderr: {stderr}");
    assert!(stderr.contains("cell    B3"), "stderr: {stderr}");
    assert_eq!(stderr.matches("[CELL-TypeMismatch] [CELL]").count(), 2);
}

#[test]
fn excel_missing_sheet_diagnostics_include_sheet_in_human_output() {
    let root = temp_project_dir("excel-missing-sheet-location");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(root.join("schema").join("main.cft"), "type Item {}\n").expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/items.xlsx
    sheets:
      - sheet: Missing
        type: Item
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");

    let xlsx_path = root.join("data").join("items.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new();
    workbook
        .add_worksheet()
        .set_name("Other")
        .expect("set sheet name");
    workbook.save(&xlsx_path).expect("write xlsx");

    let output = coflow()
        .args(["check", root.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow check");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("[EXCEL-SHEET] [EXCEL]"), "stderr: {stderr}");
    assert!(
        stderr.contains("file    data/items.xlsx"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("sheet   Missing"), "stderr: {stderr}");
    assert!(
        stderr.contains("message\n  workbook `data/items.xlsx` is missing sheet `Missing`"),
        "stderr: {stderr}"
    );
    assert!(
        !stderr.contains(root.to_string_lossy().as_ref()),
        "stderr should use project-relative paths: {stderr}"
    );
}

#[test]
fn excel_diagnostics_collect_multiple_missing_sheets() {
    let root = temp_project_dir("excel-multiple-missing-sheets");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(root.join("schema").join("main.cft"), "type Item {}\n").expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/items.xlsx
    sheets:
      - sheet: MissingA
        type: Item
      - sheet: MissingB
        type: Item
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");

    let xlsx_path = root.join("data").join("items.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new();
    workbook
        .add_worksheet()
        .set_name("Other")
        .expect("set sheet name");
    workbook.save(&xlsx_path).expect("write xlsx");

    let output = coflow()
        .args(["check", root.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow check");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("sheet   MissingA"), "stderr: {stderr}");
    assert!(stderr.contains("sheet   MissingB"), "stderr: {stderr}");
    assert_eq!(stderr.matches("[EXCEL-SHEET] [EXCEL]").count(), 2);
}

#[test]
fn excel_diagnostics_collect_multiple_unknown_columns() {
    let root = temp_project_dir("excel-multiple-unknown-columns");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(root.join("schema").join("main.cft"), "type Item {}\n").expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - file: data/items.xlsx
    sheets:
      - sheet: Items
        type: Item
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");

    let xlsx_path = root.join("data").join("items.xlsx");
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let sheet = workbook
        .add_worksheet()
        .set_name("Items")
        .expect("set sheet name");
    sheet.write_string(0, 0, "id").expect("write id header");
    sheet
        .write_string(0, 1, "missing_a")
        .expect("write missing header a");
    sheet
        .write_string(0, 2, "missing_b")
        .expect("write missing header b");
    sheet.write_string(1, 0, "item_1").expect("write id");
    workbook.save(&xlsx_path).expect("write xlsx");

    let output = coflow()
        .args(["check", root.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow check");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("column `missing_a` maps to unknown field `missing_a` on type `Item`"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("column `missing_b` maps to unknown field `missing_b` on type `Item`"),
        "stderr: {stderr}"
    );
    assert_eq!(stderr.matches("[EXCEL-COLUMN] [EXCEL]").count(), 2);
}

#[test]
fn cft_diagnostics_use_readable_relative_human_output() {
    let root = temp_project_dir("cft-diagnostic-format");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { value: Missing; }\n",
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema/\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");

    let output = coflow()
        .args(["cft", "check", root.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow cft check");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("----------------------------------------"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("[CFT-SCHEMA-"), "stderr: {stderr}");
    assert!(
        stderr.contains("file    schema/main.cft"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("line    "), "stderr: {stderr}");
    assert!(stderr.contains("column  "), "stderr: {stderr}");
    assert!(
        stderr.contains("message\n  unknown field type `Missing`"),
        "stderr: {stderr}"
    );
}

#[test]
fn top_level_cli_errors_use_readable_human_output() {
    let output = coflow()
        .args(["check", "definitely-missing-project"])
        .output()
        .expect("run coflow check");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("----------------------------------------"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("[CLI-ERROR] [CLI]"), "stderr: {stderr}");
    assert!(
        stderr
            .contains("message\n  config or directory `definitely-missing-project` does not exist"),
        "stderr: {stderr}"
    );
}

#[test]
fn cft_lsp_legacy_entry_is_not_available() {
    let output = coflow()
        .args(["cft", "lsp", "--help"])
        .output()
        .expect("run coflow cft lsp");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unrecognized subcommand"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("lsp"), "stderr: {stderr}");
}

#[test]
fn project_scoped_cli_errors_use_relative_paths_in_message() {
    let root = temp_project_dir("project-error-relative-path");
    let _cleanup = TempDirCleanup(root.clone());
    copy_dir_recursive(std::path::Path::new("examples/rpg"), &root).expect("copy example project");
    let output_path = root.join("generated").join("data");
    let copied_generated = root.join("generated");
    if copied_generated.exists() {
        std::fs::remove_dir_all(&copied_generated).expect("remove copied generated outputs");
    }
    std::fs::create_dir_all(root.join("generated")).expect("create generated dir");
    std::fs::write(&output_path, "not a directory").expect("create blocking file");

    let output = coflow()
        .args(["build", root.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow build");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("----------------------------------------"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("[ARTIFACT-001] [ARTIFACT]"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains(
            "message\n  output dir `generated/data` already exists and is not a directory"
        ),
        "stderr: {stderr}"
    );
    assert!(
        !stderr.contains(root.to_string_lossy().as_ref()),
        "stderr should use project-relative paths: {stderr}"
    );
}
