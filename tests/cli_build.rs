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
    let project_dir = root_dir.join("project");
    let data_dir = root_dir.join("data-out");
    let code_dir = root_dir.join("code-out");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old temp dir");
    }
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    write_acyclic_csharp_project(&project_dir, "json");

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
    assert!(data_dir.join("Bundle.json").exists());
    assert!(!data_dir.join("EmptyThing.json").exists());
    let coflow_tables =
        std::fs::read_to_string(code_dir.join("CoflowTables.cs")).expect("CoflowTables.cs");
    assert!(coflow_tables
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
    let project_dir = root_dir.join("project");
    let data_dir = root_dir.join("data-out");
    let code_dir = root_dir.join("code-out");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old temp dir");
    }
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    write_acyclic_csharp_project(&project_dir, "messagepack");

    let output = coflow()
        .args([
            "build",
            project_dir.to_str().expect("utf8 temp path"),
            "--data-out",
            data_dir.to_str().expect("utf8 temp path"),
            "--code-out",
            code_dir.to_str().expect("utf8 temp path"),
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
    assert!(data_dir.join("Bundle.msgpack").exists());
    assert!(code_dir.join("CoflowTables.cs").exists());

    std::fs::remove_dir_all(root_dir).expect("clean temp dir");
}

#[test]
fn build_loads_csv_sources_and_exports_json() {
    let root = temp_project_dir("build-csv-json");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            enum Rarity { Common = 0, Rare = 10, }
            type Item {
                name: string;
                rarity: Rarity = Rarity.Common;
                tags: [string] = [];
            }
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.csv"),
        "物品ID,名称,稀有度,tags\nsword_01,铁剑,Rare,weapon | melee\n",
    )
    .expect("write csv");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - type: csv
    path: data/items.csv
    sheets:
      - sheet: Items
        type: Item
        key: 物品ID
        columns:
          名称: name
          稀有度: rarity
outputs:
  data:
    type: json
    dir: generated/data
",
    )
    .expect("write config");

    let output = coflow()
        .args(["build", root.to_str().expect("utf8 temp path")])
        .output()
        .expect("run coflow build");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let exported = std::fs::read_to_string(root.join("generated").join("data").join("Item.json"))
        .expect("read exported Item.json");
    assert!(exported.contains("sword_01"), "exported: {exported}");
    assert!(exported.contains("铁剑"), "exported: {exported}");
}

#[test]
fn build_rejects_outputs_that_overlap_input_directories_both_directions() {
    let root = temp_project_dir("build-output-input-overlap");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
    std::fs::create_dir_all(root.join("data").join("generated")).expect("create nested source dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { value: int; }\n",
    )
    .expect("write schema");
    std::fs::write(
        root.join("data").join("items.cfd"),
        "item: Item { value: 1 }\n",
    )
    .expect("write data");

    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data
outputs:
  data:
    type: json
    dir: data/generated
",
    )
    .expect("write nested output config");
    let nested = coflow()
        .args(["build", root.to_str().expect("utf8 path")])
        .output()
        .expect("run nested overlap build");
    assert!(!nested.status.success());
    let nested_stderr = String::from_utf8_lossy(&nested.stderr);
    assert!(
        nested_stderr.contains("overlaps data source"),
        "stderr: {nested_stderr}"
    );

    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data/generated
outputs:
  data:
    type: json
    dir: data
",
    )
    .expect("write containing output config");
    let containing = coflow()
        .args(["build", root.to_str().expect("utf8 path")])
        .output()
        .expect("run containing overlap build");
    assert!(!containing.status.success());
    let containing_stderr = String::from_utf8_lossy(&containing.stderr);
    assert!(
        containing_stderr.contains("overlaps data source"),
        "stderr: {containing_stderr}"
    );

    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data/items.cfd
outputs:
  data:
    type: json
    dir: data/generated
",
    )
    .expect("write file source nested output config");
    let file_source_nested = coflow()
        .args(["build", root.to_str().expect("utf8 path")])
        .output()
        .expect("run file source nested overlap build");
    assert!(!file_source_nested.status.success());
    let file_source_nested_stderr = String::from_utf8_lossy(&file_source_nested.stderr);
    assert!(
        file_source_nested_stderr.contains("overlaps data source"),
        "stderr: {file_source_nested_stderr}"
    );
}
