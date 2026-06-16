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
fn build_exports_data_and_generates_csharp_for_json_project() {
    let suffix = unique_suffix();
    let root_dir = std::env::temp_dir().join(format!("coflow-build-json-test-{suffix}"));
    let project_dir = root_dir.join("rpg");
    let data_dir = root_dir.join("data-out");
    let code_dir = root_dir.join("code-out");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old temp dir");
    }
    copy_dir_recursive(std::path::Path::new("examples/rpg"), &project_dir)
        .expect("copy example project");

    let output = coflow()
        .args([
            "build",
            project_dir.to_str().expect("utf8 temp path"),
            "--data-out",
            data_dir.to_str().expect("utf8 temp path"),
            "--code-out",
            code_dir.to_str().expect("utf8 temp path"),
            "--namespace",
            "Game.Config",
        ])
        .output()
        .expect("run coflow build");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(data_dir.join("Item.json").exists());
    assert!(data_dir.join("DropTable.json").exists());
    let game_config =
        std::fs::read_to_string(code_dir.join("GameConfig.cs")).expect("GameConfig.cs");
    assert!(game_config
        .replace("\r\n", "\n")
        .contains("namespace Game.Config\n{"));
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Build completed"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    std::fs::remove_dir_all(root_dir).expect("clean temp dir");
}

#[test]
fn build_exports_messagepack_when_configured() {
    let suffix = unique_suffix();
    let root_dir = std::env::temp_dir().join(format!("coflow-build-messagepack-test-{suffix}"));
    let project_dir = root_dir.join("rpg");
    let data_dir = root_dir.join("data-out");
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

    let output = coflow()
        .args([
            "build",
            project_dir.to_str().expect("utf8 temp path"),
            "--data-out",
            data_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow build");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(data_dir.join("Item.msgpack").exists());
    assert!(data_dir.join("DropTable.msgpack").exists());

    std::fs::remove_dir_all(root_dir).expect("clean temp dir");
}
