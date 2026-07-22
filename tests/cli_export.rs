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
fn export_writes_configured_json_target() {
    let root_dir = temp_project_dir("json-export");
    let project_dir = root_dir.join("rpg");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old output dir");
    }
    copy_dir_recursive(std::path::Path::new("examples/rpg"), &project_dir)
        .expect("copy example project");
    let out_dir = project_dir.join("generated/data");

    let output = coflow()
        .args(["export", project_dir.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("JSON data exported to"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let drop_table = std::fs::read_to_string(out_dir.join("DropTable.json"))
        .expect("DropTable.json should be written");
    assert!(drop_table.contains(r#""$type": "ItemReward""#));
    assert!(drop_table.contains(r#""monster": "goblin_warrior""#));
    std::fs::remove_dir_all(root_dir).expect("clean output dir");
}

#[test]
fn export_messagepack_writes_msgpack_tables() {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    );
    let root_dir = std::env::temp_dir().join(format!("coflow-messagepack-export-test-{suffix}"));
    let project_dir = root_dir.join("rpg");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old temp dir");
    }
    copy_dir_recursive(std::path::Path::new("examples/rpg"), &project_dir)
        .expect("copy example project");
    let config_path = project_dir.join("coflow.yaml");
    let config = std::fs::read_to_string(&config_path).expect("read coflow.yaml");
    std::fs::write(
        &config_path,
        config.replacen("type: json", "type: messagepack", 1),
    )
    .expect("write coflow.yaml");
    let out_dir = project_dir.join("generated/data");

    let output = coflow()
        .args(["export", project_dir.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("MessagePack data exported to"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(out_dir.join("Item.msgpack").exists());
    assert!(out_dir.join("DropTable.msgpack").exists());

    std::fs::remove_dir_all(root_dir).expect("clean temp dir");
}

#[test]
fn export_writes_all_configured_data_targets() {
    let root_dir = temp_project_dir("multi-target-export");
    let project_dir = root_dir.join("project");
    write_acyclic_csharp_project(&project_dir, "json");
    std::fs::write(
        project_dir.join("coflow.yaml"),
        r#"schema: schema.cft
sources:
  - path: data
outputs:
  - data:
      type: json
      dir: generated/json
  - data:
      type: messagepack
      dir: generated/messagepack
"#,
    )
    .expect("write multi-target config");

    let output = coflow()
        .args(["export", project_dir.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(project_dir.join("generated/json/Item.json").exists());
    assert!(project_dir
        .join("generated/messagepack/Item.msgpack")
        .exists());

    std::fs::remove_dir_all(root_dir).expect("clean output dir");
}
