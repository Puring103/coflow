#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

mod common;
use common::*;
use serde_json::json;
use std::io::Write;

fn write_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r#"
            @display("Item")
            type Item {
                name: string;
                price: int;
                check { price > 0; }
            }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        r#"sword: Item { name: "Sword", price: 100 }"#,
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
}

fn write_table_project(root: &std::path::Path, fields: &str, source: &str) {
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        format!(
            r"
            type Item {{
{fields}
            }}
        "
        ),
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        format!(
            "schema: schema.cft\nsources:\n  - path: {source}\n    type: csv\n    sheets:\n      - sheet: Item\n        type: Item\noutputs:\n  data:\n    type: json\n    dir: generated/data\n"
        ),
    )
    .expect("write config");
}

fn create_items_csv_table(root: &std::path::Path) {
    write_table_project(
        root,
        "                name: string;\n                price: int;",
        "data/items.csv",
    );

    let output = coflow()
        .args([
            "data",
            "create-file",
            root.to_str().expect("utf8 path"),
            "--file",
            "data/items.csv",
            "--type",
            "Item",
            "--provider",
            "csv",
        ])
        .output()
        .expect("run data create-file");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn apply_data_patch_command(root: &std::path::Path, file_name: &str, patch: &Value) -> Value {
    let patch_path = root.join(file_name);
    std::fs::write(
        &patch_path,
        serde_json::to_string(patch).expect("patch json"),
    )
    .expect("write patch");

    let output = coflow()
        .args([
            "data",
            "patch",
            root.to_str().expect("utf8 path"),
            "--patch",
            patch_path.to_str().expect("utf8 patch path"),
        ])
        .output()
        .expect("run data patch");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("patch json")
}

fn run_stdin_command(args: &[&str], stdin: &str) -> std::process::Output {
    let mut command = coflow();
    command.args(args);
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn stdin command");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("run stdin command")
}

#[test]
fn schema_inspect_outputs_json_by_default_and_includes_item_annotations() {
    let root = temp_project_dir("cli-schema-inspect");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);

    let output = coflow()
        .args(["schema", "inspect", root.to_str().expect("utf8 path")])
        .output()
        .expect("run schema inspect");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("schema inspect json");
    assert!(
        json["types"].as_array().expect("types").iter().any(|ty| {
            ty["name"] == "Item"
                && ty["annotations"]
                    .as_array()
                    .is_some_and(|items| items.iter().any(|a| a["name"] == "display"))
        }),
        "schema inspect output: {json:?}"
    );
}

#[test]
fn schema_write_file_writes_existing_schema_file_from_stdin() {
    let root = temp_project_dir("cli-schema-write-file");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);
    let next_schema = r"
        type Item {
            name: string;
            price: int;
            rarity: string;
        }
    ";

    let mut command = coflow();
    command.args([
        "schema",
        "write-file",
        root.to_str().expect("utf8 path"),
        "--file",
        "schema.cft",
        "--stdin",
    ]);
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn schema write-file");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(next_schema.as_bytes())
        .expect("write schema stdin");
    let output = child.wait_with_output().expect("run schema write-file");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("write json");
    assert_eq!(json["file"], "schema.cft");
    assert_eq!(json["written"], true);
    assert_eq!(json["dry_run"], false);
    assert_eq!(json["changed"], true);
    assert_eq!(json["check_ok"], Value::Null);
    assert_eq!(
        std::fs::read_to_string(root.join("schema.cft")).expect("read schema"),
        next_schema
    );
}

#[test]
fn schema_write_file_dry_run_does_not_write() {
    let root = temp_project_dir("cli-schema-write-file-dry-run");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);
    let before = std::fs::read_to_string(root.join("schema.cft")).expect("read schema");

    let mut command = coflow();
    command.args([
        "schema",
        "write-file",
        root.to_str().expect("utf8 path"),
        "--file",
        "schema.cft",
        "--stdin",
        "--dry-run",
    ]);
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn schema write-file dry-run");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"type Item { name: string; }\n")
        .expect("write schema stdin");
    let output = child
        .wait_with_output()
        .expect("run schema write-file dry-run");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("write json");
    assert_eq!(json["written"], false);
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["changed"], true);
    assert_eq!(
        std::fs::read_to_string(root.join("schema.cft")).expect("read schema"),
        before
    );
}

#[test]
fn schema_write_file_rejects_non_schema_file() {
    let root = temp_project_dir("cli-schema-write-file-reject");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);

    let mut command = coflow();
    command.args([
        "schema",
        "write-file",
        root.to_str().expect("utf8 path"),
        "--file",
        "data/items.cfd",
        "--stdin",
    ]);
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn schema write-file reject");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"type Item { name: string; }\n")
        .expect("write schema stdin");
    let output = child
        .wait_with_output()
        .expect("run schema write-file reject");

    assert!(
        !output.status.success(),
        "non-schema write should fail\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("configured .cft schema file"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn schema_write_file_check_reports_schema_diagnostics() {
    let root = temp_project_dir("cli-schema-write-file-check");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);

    let mut command = coflow();
    command.args([
        "schema",
        "write-file",
        root.to_str().expect("utf8 path"),
        "--file",
        "schema.cft",
        "--stdin",
        "--check",
    ]);
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn schema write-file check");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"type Item { name: MissingType; }\n")
        .expect("write schema stdin");
    let output = child
        .wait_with_output()
        .expect("run schema write-file check");

    assert!(
        !output.status.success(),
        "schema diagnostics should produce non-zero exit\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("write json");
    assert_eq!(json["written"], true);
    assert_eq!(json["check_ok"], false);
    assert!(
        json["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(|diagnostic| diagnostic["code"] == "CFT-SCHEMA-006"),
        "write output: {json:?}"
    );
    assert!(std::fs::read_to_string(root.join("schema.cft"))
        .expect("read schema")
        .contains("MissingType"));
}

#[test]
fn data_get_can_fetch_single_complete_record() {
    let root = temp_project_dir("cli-data-get");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);

    let output = coflow()
        .args([
            "data",
            "get",
            root.to_str().expect("utf8 path"),
            "Item.sword",
        ])
        .output()
        .expect("run data get");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("data get json");
    assert_eq!(json["records"][0]["record"]["key"], "sword");
    assert_eq!(json["records"][0]["file"], "data/items.cfd");
    assert_eq!(json["records"][0]["fields"]["name"]["kind"], "string");
    assert_eq!(json["records"][0]["fields"]["name"]["value"], "Sword");
    assert_eq!(json["records"][0]["fields"]["price"]["kind"], "int");
    assert_eq!(json["records"][0]["fields"]["price"]["value"], "100");
}

#[test]
fn data_get_treats_single_config_file_argument_as_project_path() {
    let root = temp_project_dir("cli-data-get-config-file");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);

    let mut command = coflow();
    let output = command
        .current_dir(&root)
        .args([
            "data",
            "get",
            "coflow.yaml",
            "--type",
            "Item",
            "--keys",
            "sword",
        ])
        .output()
        .expect("run data get");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("data get json");
    assert_eq!(json["records"][0]["record"]["key"], "sword");
    assert_eq!(json["records"][0]["file"], "data/items.cfd");
}

#[test]
fn data_patch_writes_then_returns_check_diagnostics_and_nonzero_exit() {
    let root = temp_project_dir("cli-data-patch");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);
    let patch_path = root.join("patch.json");
    std::fs::write(
        &patch_path,
        serde_json::to_string(&json!({
            "ops": [{
                "op": "insert_record",
                "file": "data/items.cfd",
                "type": "Item",
                "key": "bad_sword",
                "fields": { "name": "Bad Sword", "price": -1 }
            }]
        }))
        .expect("patch json"),
    )
    .expect("write patch");

    let output = coflow()
        .args([
            "data",
            "patch",
            root.to_str().expect("utf8 path"),
            "--patch",
            patch_path.to_str().expect("utf8 patch path"),
        ])
        .output()
        .expect("run data patch");

    assert!(
        !output.status.success(),
        "check diagnostics should produce non-zero exit\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("data patch json");
    assert_eq!(json["write_ok"], true);
    assert_eq!(json["check_ok"], false);
    assert!(json["failed"].as_array().expect("failed").is_empty());
    assert!(
        json["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(|diagnostic| diagnostic["stage"] == "CHECK"),
        "patch output: {json:?}"
    );
    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");
    assert!(text.contains("bad_sword"), "items.cfd:\n{text}");
}

#[test]
fn data_patch_returns_nonzero_when_check_after_write_is_false_but_errors_remain() {
    let root = temp_project_dir("cli-data-patch-check-after-false");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);
    let patch_path = root.join("patch.json");
    std::fs::write(
        &patch_path,
        serde_json::to_string(&json!({
            "check_after_write": false,
            "ops": [{
                "op": "insert_record",
                "file": "data/items.cfd",
                "type": "Item",
                "key": "unchecked_bad_sword",
                "fields": { "name": "Unchecked Bad Sword", "price": -1 }
            }]
        }))
        .expect("patch json"),
    )
    .expect("write patch");

    let output = coflow()
        .args([
            "data",
            "patch",
            root.to_str().expect("utf8 path"),
            "--patch",
            patch_path.to_str().expect("utf8 patch path"),
        ])
        .output()
        .expect("run data patch");

    assert!(
        !output.status.success(),
        "final error diagnostics should produce non-zero exit\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("data patch json");
    assert_eq!(json["write_ok"], true);
    assert_eq!(json["check_ok"], true);
    assert!(
        json["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(|diagnostic| diagnostic["severity"] == "error" && diagnostic["stage"] == "CHECK"),
        "patch output: {json:?}"
    );
}

#[test]
fn data_write_file_writes_configured_cfd_file_from_stdin() {
    let root = temp_project_dir("cli-data-write-file");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);
    let next_cfd = "shield: Item { name: \"Shield\", price: 80 }\n";

    let output = run_stdin_command(
        &[
            "data",
            "write-file",
            root.to_str().expect("utf8 path"),
            "--file",
            "data/items.cfd",
            "--stdin",
        ],
        next_cfd,
    );

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("write json");
    assert_eq!(json["file"], "data/items.cfd");
    assert_eq!(json["written"], true);
    assert_eq!(json["dry_run"], false);
    assert_eq!(json["changed"], true);
    assert_eq!(json["check_ok"], Value::Null);
    assert_eq!(
        std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd"),
        next_cfd
    );
}

#[test]
fn data_write_file_dry_run_does_not_write() {
    let root = temp_project_dir("cli-data-write-file-dry-run");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);
    let before = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");

    let output = run_stdin_command(
        &[
            "data",
            "write-file",
            root.to_str().expect("utf8 path"),
            "--file",
            "data/items.cfd",
            "--stdin",
            "--dry-run",
        ],
        "shield: Item { name: \"Shield\", price: 80 }\n",
    );

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("write json");
    assert_eq!(json["written"], false);
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["changed"], true);
    assert_eq!(
        std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd"),
        before
    );
}

#[test]
fn data_write_file_dry_run_check_reports_check_skipped() {
    let root = temp_project_dir("cli-data-write-file-dry-run-check");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);
    let before = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");

    let output = run_stdin_command(
        &[
            "data",
            "write-file",
            root.to_str().expect("utf8 path"),
            "--file",
            "data/items.cfd",
            "--stdin",
            "--dry-run",
            "--check",
        ],
        "bad_sword: Item { name: \"Bad Sword\", price: -1 }\n",
    );

    assert!(
        output.status.success(),
        "dry-run check is skipped and should not fail\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("write json");
    assert_eq!(json["written"], false);
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["check_ok"], Value::Null);
    assert_eq!(
        json["diagnostics"].as_array().expect("diagnostics").len(),
        0
    );
    assert_eq!(
        std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd"),
        before
    );
}

#[test]
fn data_write_file_rejects_non_cfd_file() {
    let root = temp_project_dir("cli-data-write-file-reject-extension");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);

    let output = run_stdin_command(
        &[
            "data",
            "write-file",
            root.to_str().expect("utf8 path"),
            "--file",
            "data/items.csv",
            "--stdin",
        ],
        "id,name,price\nshield,Shield,80\n",
    );

    assert!(
        !output.status.success(),
        "non-cfd write should fail\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("configured .cfd data file"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn data_write_file_rejects_file_outside_configured_sources() {
    let root = temp_project_dir("cli-data-write-file-reject-source");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);
    std::fs::create_dir_all(root.join("other")).expect("create other dir");
    std::fs::write(root.join("other").join("items.cfd"), "").expect("write other cfd");

    let output = run_stdin_command(
        &[
            "data",
            "write-file",
            root.to_str().expect("utf8 path"),
            "--file",
            "other/items.cfd",
            "--stdin",
        ],
        "shield: Item { name: \"Shield\", price: 80 }\n",
    );

    assert!(
        !output.status.success(),
        "unconfigured source write should fail\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("configured local CFD data source"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn data_write_file_writes_file_under_explicit_cfd_source() {
    let root = temp_project_dir("cli-data-write-file-explicit-cfd");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - type: cfd\n    path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");
    let next_cfd = "shield: Item { name: \"Shield\", price: 80 }\n";

    let output = run_stdin_command(
        &[
            "data",
            "write-file",
            root.to_str().expect("utf8 path"),
            "--file",
            "data/items.cfd",
            "--stdin",
            "--check",
        ],
        next_cfd,
    );

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("write json");
    assert_eq!(json["file"], "data/items.cfd");
    assert_eq!(json["written"], true);
    assert_eq!(json["check_ok"], true);
    assert_eq!(
        std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd"),
        next_cfd
    );
}

#[test]
fn data_write_file_rejects_cfd_file_under_explicit_non_cfd_source() {
    let root = temp_project_dir("cli-data-write-file-reject-provider");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - type: csv\n    path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");

    let output = run_stdin_command(
        &[
            "data",
            "write-file",
            root.to_str().expect("utf8 path"),
            "--file",
            "data/items.cfd",
            "--stdin",
        ],
        "shield: Item { name: \"Shield\", price: 80 }\n",
    );

    assert!(
        !output.status.success(),
        "non-cfd source write should fail\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("configured local CFD data source"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn data_write_file_check_reports_project_diagnostics_after_write() {
    let root = temp_project_dir("cli-data-write-file-check");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);
    let bad_cfd = "bad_sword: Item { name: \"Bad Sword\", price: -1 }\n";

    let output = run_stdin_command(
        &[
            "data",
            "write-file",
            root.to_str().expect("utf8 path"),
            "--file",
            "data/items.cfd",
            "--stdin",
            "--check",
        ],
        bad_cfd,
    );

    assert!(
        !output.status.success(),
        "check diagnostics should produce non-zero exit\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("write json");
    assert_eq!(json["written"], true);
    assert_eq!(json["check_ok"], false);
    assert!(
        json["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(|diagnostic| diagnostic["stage"] == "CHECK"),
        "write output: {json:?}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd"),
        bad_cfd
    );
}

#[test]
fn data_create_file_creates_csv_with_schema_header() {
    let root = temp_project_dir("cli-data-create-file-csv");
    let _cleanup = TempDirCleanup(root.clone());
    write_table_project(
        &root,
        "                name: string;\n                price: int;",
        "data/items.csv",
    );

    let output = coflow()
        .args([
            "data",
            "create-file",
            root.to_str().expect("utf8 path"),
            "--file",
            "data/items.csv",
            "--type",
            "Item",
            "--provider",
            "csv",
        ])
        .output()
        .expect("run data create-file");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("create json");
    assert_eq!(json["file"], "data/items.csv");
    assert_eq!(json["provider"], "csv");
    assert_eq!(json["headers"], json!(["id", "name", "price"]));
    let text = std::fs::read_to_string(root.join("data").join("items.csv")).expect("read csv");
    assert_eq!(text, "id,name,price\n");
}

#[test]
fn data_create_file_then_patch_creates_complete_csv_table() {
    let root = temp_project_dir("cli-data-create-file-complete-csv");
    let _cleanup = TempDirCleanup(root.clone());
    create_items_csv_table(&root);

    let json = apply_data_patch_command(
        &root,
        "insert-potion.json",
        &json!({
            "ops": [{
                "op": "insert_record",
                "file": "data/items.csv",
                "type": "Item",
                "key": "potion",
                "fields": { "name": "Potion", "price": 25 }
            }]
        }),
    );
    assert_eq!(json["write_ok"], true);
    assert_eq!(json["check_ok"], true);
    let text = std::fs::read_to_string(root.join("data").join("items.csv")).expect("read csv");
    assert_eq!(text, "id,name,price\npotion,Potion,25\n");

    let output = coflow()
        .args([
            "data",
            "get",
            root.to_str().expect("utf8 path"),
            "Item.potion",
        ])
        .output()
        .expect("run data get");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("get json");
    assert_eq!(json["records"][0]["record"]["key"], "potion");
    assert_eq!(json["records"][0]["file"], "data/items.csv");
    assert_eq!(json["records"][0]["fields"]["name"]["value"], "Potion");
    assert_eq!(json["records"][0]["fields"]["price"]["value"], "25");
}

#[test]
fn data_create_file_then_patch_updates_csv_record() {
    let root = temp_project_dir("cli-data-create-file-update-csv");
    let _cleanup = TempDirCleanup(root.clone());
    create_items_csv_table(&root);

    let json = apply_data_patch_command(
        &root,
        "update-potion.json",
        &json!({
            "ops": [
                {
                    "op": "insert_record",
                    "file": "data/items.csv",
                    "type": "Item",
                    "key": "potion",
                    "fields": { "name": "Potion", "price": 25 }
                },
                {
                    "op": "set_field",
                    "record": { "type": "Item", "key": "potion" },
                    "file": "data/items.csv",
                    "path": ["price"],
                    "value": 40
                }
            ]
        }),
    );
    assert_eq!(json["write_ok"], true);
    assert_eq!(json["check_ok"], true);
    assert_eq!(json["applied"].as_array().expect("applied").len(), 2);
    let text = std::fs::read_to_string(root.join("data").join("items.csv")).expect("read csv");
    assert_eq!(text, "id,name,price\npotion,Potion,40\n");

    let output = coflow()
        .args([
            "data",
            "get",
            root.to_str().expect("utf8 path"),
            "Item.potion",
        ])
        .output()
        .expect("run data get");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("get json");
    assert_eq!(json["records"][0]["fields"]["price"]["value"], "40");
}

#[test]
fn data_create_file_then_patch_deletes_csv_record() {
    let root = temp_project_dir("cli-data-create-file-delete-csv");
    let _cleanup = TempDirCleanup(root.clone());
    create_items_csv_table(&root);

    let json = apply_data_patch_command(
        &root,
        "delete-potion.json",
        &json!({
            "ops": [
                {
                    "op": "insert_record",
                    "file": "data/items.csv",
                    "type": "Item",
                    "key": "potion",
                    "fields": { "name": "Potion", "price": 25 }
                },
                {
                    "op": "insert_record",
                    "file": "data/items.csv",
                    "type": "Item",
                    "key": "elixir",
                    "fields": { "name": "Elixir", "price": 60 }
                },
                {
                    "op": "delete_record",
                    "record": { "type": "Item", "key": "potion" },
                    "file": "data/items.csv"
                }
            ]
        }),
    );
    assert_eq!(json["write_ok"], true);
    assert_eq!(json["check_ok"], true);
    assert_eq!(json["applied"].as_array().expect("applied").len(), 3);
    let text = std::fs::read_to_string(root.join("data").join("items.csv")).expect("read csv");
    assert_eq!(text, "id,name,price\nelixir,Elixir,60\n");

    let output = coflow()
        .args([
            "data",
            "get",
            root.to_str().expect("utf8 path"),
            "Item.potion",
        ])
        .output()
        .expect("run data get deleted");
    assert!(
        !output.status.success(),
        "deleted record lookup should fail\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("get deleted json");
    assert!(
        json["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(|diagnostic| diagnostic["code"] == "DATA-NOT-FOUND"),
        "get output: {json:?}"
    );

    let output = coflow()
        .args([
            "data",
            "get",
            root.to_str().expect("utf8 path"),
            "Item.elixir",
        ])
        .output()
        .expect("run data get remaining");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("get remaining json");
    assert_eq!(json["records"][0]["record"]["key"], "elixir");
    assert_eq!(json["records"][0]["fields"]["price"]["value"], "60");
}

#[test]
fn data_sync_header_adds_and_removes_csv_columns_while_preserving_rows() {
    let root = temp_project_dir("cli-data-sync-header-csv");
    let _cleanup = TempDirCleanup(root.clone());
    write_table_project(
        &root,
        "                name: string;\n                rarity: string;",
        "data/items.csv",
    );
    std::fs::write(
        root.join("data").join("items.csv"),
        "id,name,price\nsword,Sword,100\n",
    )
    .expect("write csv");

    let output = coflow()
        .args([
            "data",
            "sync-header",
            root.to_str().expect("utf8 path"),
            "--file",
            "data/items.csv",
            "--type",
            "Item",
        ])
        .output()
        .expect("run data sync-header");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("sync json");
    assert_eq!(json["headers"], json!(["id", "name", "rarity"]));
    assert_eq!(json["added"], json!(["rarity"]));
    assert_eq!(json["removed"], json!(["price"]));
    let text = std::fs::read_to_string(root.join("data").join("items.csv")).expect("read csv");
    assert_eq!(text, "id,name,rarity\nsword,Sword,\n");
}

#[test]
fn data_create_file_creates_empty_cfd_file() {
    let root = temp_project_dir("cli-data-create-file-cfd");
    let _cleanup = TempDirCleanup(root.clone());
    write_project(&root);

    let output = coflow()
        .args([
            "data",
            "create-file",
            root.to_str().expect("utf8 path"),
            "--file",
            "data/new_items.cfd",
            "--provider",
            "cfd",
        ])
        .output()
        .expect("run data create-file cfd");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("create json");
    assert_eq!(json["file"], "data/new_items.cfd");
    assert_eq!(json["provider"], "cfd");
    assert!(root.join("data").join("new_items.cfd").exists());
    let text = std::fs::read_to_string(root.join("data").join("new_items.cfd")).expect("read cfd");
    assert_eq!(text, "");
}

#[test]
fn data_sync_header_updates_cfd_record_columns_without_creating_a_header() {
    let root = temp_project_dir("cli-data-sync-header-cfd");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema.cft"),
        r"
            type Item {
                name: string;
                rarity: string?;
            }
        ",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        "sword: Item {\n  name: \"Sword\",\n  price: 100,\n}\n",
    )
    .expect("write cfd");
    std::fs::write(
        root.join("coflow.yaml"),
        "schema: schema.cft\nsources:\n  - path: data\noutputs:\n  data:\n    type: json\n    dir: generated/data\n",
    )
    .expect("write config");

    let output = coflow()
        .args([
            "data",
            "sync-header",
            root.to_str().expect("utf8 path"),
            "--file",
            "data/items.cfd",
            "--type",
            "Item",
        ])
        .output()
        .expect("run data sync-header cfd");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).expect("sync json");
    assert_eq!(json["headers"], json!(["id", "name", "rarity"]));
    assert_eq!(json["added"], json!(["rarity"]));
    assert_eq!(json["removed"], json!(["price"]));
    let text = std::fs::read_to_string(root.join("data").join("items.cfd")).expect("read cfd");
    assert!(!text.lines().any(|line| line == "id,name,rarity"));
    assert!(text.contains("name: \"Sword\""), "items.cfd:\n{text}");
    assert!(text.contains("rarity: null"), "items.cfd:\n{text}");
    assert!(!text.contains("price:"), "items.cfd:\n{text}");
}
