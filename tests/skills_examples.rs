#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};

fn call(root: &Path, arguments: &[&str], expected_exit: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_coflow"))
        .current_dir(root)
        .args(arguments)
        .arg("--json")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code(),
        Some(expected_exit),
        "{arguments:?}\n{stdout}\n{stderr}"
    );
    serde_json::from_str(&stdout).expect("CLI must return JSON")
}

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    call(root.path(), &["init", "."], 0);
    call(root.path(), &["skill", "install"], 0);
    // 从真正安装的技能包读取示例，验证分发资源和执行内容一致。
    let example = root
        .path()
        .join(".agents/skills/coflow-schema/examples/minimal-project");
    for relative in ["coflow.yaml", "schema/main.cft", "data/items.cfd"] {
        fs::copy(example.join(relative), root.path().join(relative)).unwrap();
    }
    root
}

fn record(root: &Path, id: &str) -> Value {
    call(root, &["data", "get", "--type", "Item", "--id", id], 0)
}

fn example_path(skill: &str, name: &str) -> String {
    format!(".agents/skills/{skill}/examples/{name}")
}

#[test]
fn installed_minimal_project_validates_and_generates_with_documented_values() {
    let root = fixture();
    let checked = call(root.path(), &["check", "--rules"], 0);
    assert_eq!(checked["result"]["complete"], true);
    assert_eq!(checked["result"]["rules"], "passed");
    let sword = record(root.path(), "sword");
    let fields = &sword["result"]["fields"];
    assert_eq!(fields["price"]["value"], "90");
    assert_eq!(fields["stats"]["fields"]["hp"]["value"], "100");
    assert_eq!(fields["note"]["kind"], "none");
    assert_eq!(fields["next"]["value"]["id"], "shield");
    let settings = call(
        root.path(),
        &["data", "get", "--type", "Settings", "--id", "Settings"],
        0,
    );
    assert_eq!(settings["result"]["fields"]["start"]["id"], "sword");
    call(root.path(), &["build"], 0);
    call(root.path(), &["codegen", "--check"], 0);
    assert!(fs::read_dir(root.path().join("generated/csharp"))
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "cs")));
}

#[test]
fn installed_record_example_previews_then_writes_and_rejects_stale_revision() {
    let root = fixture();
    let input = example_path("coflow-workflow", "record-changes.json");
    let before = record(root.path(), "sword");
    let revision = before["revision"].as_str().unwrap();
    let source_path = root.path().join("data/items.cfd");
    let source = fs::read(&source_path).unwrap();
    let preview = call(
        root.path(),
        &[
            "apply",
            "--input",
            &input,
            "--rules",
            "--if-revision",
            revision,
            "--dry-run",
        ],
        0,
    );
    assert_eq!(preview["result"]["written"], false);
    assert_eq!(fs::read(&source_path).unwrap(), source);
    call(
        root.path(),
        &[
            "apply",
            "--input",
            &input,
            "--rules",
            "--if-revision",
            revision,
        ],
        0,
    );
    let after = record(root.path(), "sword");
    let fields = &after["result"]["fields"];
    assert_eq!(fields["price"]["value"], "100");
    assert_eq!(fields["tags"]["items"][0]["value"], "weapon");
    assert_eq!(fields["labels"]["entries"][0]["key"]["value"], "category");
    assert_eq!(
        fields["labels"]["entries"][0]["value"]["value"],
        "equipment"
    );
    assert_eq!(fields["next"]["value"]["id"], "shield");
    let published = fs::read(&source_path).unwrap();
    let conflict = call(
        root.path(),
        &[
            "apply",
            "--input",
            &input,
            "--rules",
            "--if-revision",
            revision,
        ],
        3,
    );
    assert_eq!(conflict["status"], "conflict");
    assert_eq!(fs::read(&source_path).unwrap(), published);
}

#[test]
fn installed_batch_example_checks_total_and_updates_all_matches() {
    let root = fixture();
    let input = example_path("coflow-workflow", "batch-changes.json");
    let source_path = root.path().join("data/items.cfd");
    let before = fs::read(&source_path).unwrap();
    let mut wrong: Value =
        serde_json::from_slice(&fs::read(root.path().join(&input)).unwrap()).unwrap();
    wrong["ops"][0]["expected_count"] = json!(1);
    fs::write(root.path().join("wrong-count.json"), wrong.to_string()).unwrap();
    call(
        root.path(),
        &["apply", "--input", "wrong-count.json", "--rules"],
        3,
    );
    assert_eq!(fs::read(&source_path).unwrap(), before);
    // 一页只显示一条，写入数量仍以筛选总数为准。
    let selected = call(
        root.path(),
        &[
            "data",
            "list",
            "--type",
            "Item",
            "--where",
            "price < 100",
            "--limit",
            "1",
        ],
        0,
    );
    assert_eq!(selected["result"]["total"], 2);
    assert_eq!(selected["result"]["items"].as_array().unwrap().len(), 1);
    call(
        root.path(),
        &[
            "apply",
            "--input",
            &input,
            "--rules",
            "--if-revision",
            selected["revision"].as_str().unwrap(),
        ],
        0,
    );
    for id in ["sword", "shield"] {
        assert_eq!(
            record(root.path(), id)["result"]["fields"]["price"]["value"],
            "100"
        );
    }
}

fn dimension(root: &Path, id: &str, field: &str) -> Value {
    call(
        root,
        &[
            "dimension",
            "get",
            "--type",
            "Item",
            "--id",
            id,
            "--field",
            field,
            "--dimension",
            "language",
            "--variant",
            "ja",
        ],
        0,
    )
}

#[test]
fn installed_dimension_example_preserves_base_and_template_fallback() {
    let root = fixture();
    let input = example_path("coflow-workflow", "dimension-changes.json");
    let before = dimension(root.path(), "shield", "name");
    assert_eq!(before["result"]["explicit"], false);
    call(
        root.path(),
        &[
            "apply",
            "--input",
            &input,
            "--rules",
            "--if-revision",
            before["revision"].as_str().unwrap(),
        ],
        0,
    );
    let name = dimension(root.path(), "shield", "name");
    assert_eq!(name["result"]["explicit"], true);
    assert_eq!(name["result"]["effective"]["value"], "盾");
    let label = dimension(root.path(), "shield", "label");
    assert_eq!(label["result"]["explicit"], false);
    assert_eq!(label["result"]["rendered"], "盾：80");
    assert_eq!(
        record(root.path(), "shield")["result"]["fields"]["name"]["value"],
        "盾牌"
    );
    assert_eq!(
        record(root.path(), "sword")["result"]["fields"]["price"]["value"],
        "90"
    );
    let region = call(
        root.path(),
        &[
            "dimension",
            "get",
            "--type",
            "Item",
            "--id",
            "sword",
            "--field",
            "price",
            "--dimension",
            "region",
            "--variant",
            "cn",
        ],
        0,
    );
    assert_eq!(region["result"]["effective"]["value"], "85");
}

#[test]
fn installed_schema_example_requires_joint_data_update() {
    let root = fixture();
    let input = example_path("coflow-schema", "schema-changes.json");
    let schema_path = root.path().join("schema/main.cft");
    let before = fs::read(&schema_path).unwrap();
    let candidate = example_path("coflow-schema", "candidates/main.cft");
    // 单独发布新增必填字段会失败，原 Schema 必须保持不变。
    call(
        root.path(),
        &[
            "source",
            "write",
            "--file",
            "schema/main.cft",
            "--input",
            &candidate,
            "--rules",
        ],
        1,
    );
    assert_eq!(fs::read(&schema_path).unwrap(), before);
    let snapshot = record(root.path(), "sword");
    let applied = call(
        root.path(),
        &[
            "apply",
            "--input",
            &input,
            "--rules",
            "--if-revision",
            snapshot["revision"].as_str().unwrap(),
        ],
        0,
    );
    assert_eq!(applied["result"]["written"], true);
    assert_eq!(applied["result"]["validation"]["complete"], true);
    assert_eq!(
        record(root.path(), "sword")["result"]["fields"]["weight"]["value"],
        "3"
    );
    assert_eq!(
        record(root.path(), "shield")["result"]["fields"]["weight"]["value"],
        "5"
    );
    assert_eq!(
        dimension(root.path(), "sword", "name")["result"]["effective"]["value"],
        "剣"
    );
}

#[test]
fn installed_file_candidate_repairs_broken_source() {
    let root = fixture();
    fs::write(root.path().join("data/items.cfd"), "sword: Item { name:").unwrap();
    call(root.path(), &["check", "--rules"], 1);
    let source = call(
        root.path(),
        &["source", "read", "--file", "data/items.cfd"],
        0,
    );
    let input = example_path("coflow-schema", "schema-changes.json");
    call(
        root.path(),
        &[
            "apply",
            "--input",
            &input,
            "--rules",
            "--if-revision",
            source["revision"].as_str().unwrap(),
        ],
        0,
    );
    assert_eq!(
        record(root.path(), "sword")["result"]["fields"]["weight"]["value"],
        "3"
    );
}
