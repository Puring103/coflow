use std::process::Command;

fn coflow() -> Command {
    Command::new(env!("CARGO_BIN_EXE_coflow"))
}

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
fn export_json_validates_declared_output_type() {
    let out_dir =
        std::env::temp_dir().join(format!("coflow-json-export-test-{}", std::process::id()));
    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir).expect("clean old output dir");
    }

    let output = coflow()
        .args([
            "export",
            "json",
            "examples/rpg",
            "--out",
            out_dir.to_str().expect("utf8 temp path"),
        ])
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
    assert!(drop_table.contains(r#""monster_id": "goblin_warrior""#));
    std::fs::remove_dir_all(out_dir).expect("clean output dir");
}

#[test]
fn codegen_csharp_validates_declared_output_type() {
    let output = coflow()
        .args(["codegen", "csharp", "examples/rpg"])
        .output()
        .expect("run coflow");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("C# code generation is not implemented yet"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
