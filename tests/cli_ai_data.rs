#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::unwrap_used
)]

mod common;
use common::*;
use serde_json::json;

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
