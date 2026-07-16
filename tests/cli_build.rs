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
    let exported = std::fs::read_to_string(root.join("generated/data/Item.json"))
        .expect("read exported Item.json");
    assert!(exported.contains("sword_01"), "exported: {exported}");
    assert!(exported.contains("铁剑"), "exported: {exported}");
}

#[test]
fn build_exports_dimension_variant_tables_to_requested_data_dir() {
    let root = temp_project_dir("build-dimension-export");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data/localization")).expect("create localization dir");
    std::fs::write(
        root.join("schema/main.cft"),
        r#"
            type Item {
                @localized
                name: string;
            }
        "#,
    )
    .expect("write schema");
    std::fs::write(root.join("data/items.csv"), "id,name\npotion,Potion\n")
        .expect("write source data");
    std::fs::write(
        root.join("data/localization/Item_name.csv"),
        "id,default,zh,en\npotion,Old,药水,null\n",
    )
    .expect("write dimension data");
    std::fs::write(
        root.join("coflow.yaml"),
        r#"schema: schema/
sources:
  - path: data/items.csv
    sheets:
      - sheet: items
        type: Item
dimensions:
  language:
    variants: [zh, en]
    out_dir: data/localization
outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/code
    namespace: Game.Config
"#,
    )
    .expect("write config");

    let output = coflow()
        .args(["build", root.to_str().expect("utf8 path")])
        .output()
        .expect("run build");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let variants: Value = serde_json::from_slice(
        &std::fs::read(root.join("generated/data/Item_nameVariants.json"))
            .expect("read dimension export"),
    )
    .expect("parse dimension export");
    assert_eq!(
        variants,
        serde_json::json!([{
            "id": "potion",
            "default": "Potion",
            "zh": "药水",
            "en": null
        }])
    );
    let generated_type = root.join("generated/code/ItemNameVariants.cs");
    assert!(
        generated_type.exists(),
        "dimension C# type should be published to {}",
        generated_type.display()
    );
    let database = std::fs::read_to_string(root.join("generated/code/CoflowTables.cs"))
        .expect("read generated CoflowTables.cs");
    assert!(database.contains(
        "public Table<string, ItemNameVariants> TbItemNameVariants { get; }"
    ));
    assert!(database.contains(
        "ItemNameVariants.LoadRawTable(Path.Combine(dataDir, \"Item_nameVariants.json\"))"
    ));
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

#[test]
fn build_resolves_existing_output_alias_before_scope_check() {
    let root = temp_project_dir("build-output-alias-overlap");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("data")).expect("create data dir");
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
    create_directory_alias(&root.join("output-alias"), &root.join("data"));
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
sources:
  - path: data
outputs:
  data:
    type: json
    dir: output-alias/future
",
    )
    .expect("write alias output config");

    let output = coflow()
        .args(["build", root.to_str().expect("utf8 path")])
        .output()
        .expect("run aliased output build");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("overlaps data source"), "stderr: {stderr}");
    assert!(!root.join("data").join("future").exists());
}

#[test]
fn build_updates_requested_outputs_and_tracks_immutable_generations() {
    let root = temp_project_dir("build-immutable-generations");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(&root).expect("create project root");
    write_acyclic_csharp_project(&root, "json");
    let requested_data = root.join("artifacts/data");
    let requested_code = root.join("artifacts/code");

    let first = coflow()
        .args([
            "build",
            root.to_str().expect("utf8 path"),
            "--data-out",
            requested_data.to_str().expect("utf8 path"),
            "--code-out",
            requested_code.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("run first build");
    assert!(
        first.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    let first_data = active_artifact_dir(&root, "data");
    let first_code = active_artifact_dir(&root, "code");
    let first_item = std::fs::read(first_data.join("Item.json")).expect("read first Item.json");
    let first_tables =
        std::fs::read(first_code.join("CoflowTables.cs")).expect("read first C# generation");
    assert_eq!(
        std::fs::read(requested_data.join("Item.json")).expect("read first requested data"),
        first_item
    );
    assert_eq!(
        std::fs::read(requested_code.join("CoflowTables.cs")).expect("read first requested code"),
        first_tables
    );

    let source_path = root.join("data/records.cfd");
    let source = std::fs::read_to_string(&source_path).expect("read source");
    std::fs::write(&source_path, source.replace("Potion", "Elixir")).expect("update source");
    let second = coflow()
        .args([
            "build",
            root.to_str().expect("utf8 path"),
            "--data-out",
            requested_data.to_str().expect("utf8 path"),
            "--code-out",
            requested_code.to_str().expect("utf8 path"),
        ])
        .output()
        .expect("run second build");
    assert!(
        second.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );

    let second_data = active_artifact_dir(&root, "data");
    let second_code = active_artifact_dir(&root, "code");
    assert_ne!(first_data, second_data);
    assert_ne!(first_code, second_code);
    assert!(std::fs::read_to_string(second_data.join("Item.json"))
        .expect("read second Item.json")
        .contains("Elixir"));
    assert_eq!(
        std::fs::read(first_data.join("Item.json")).expect("re-read first Item.json"),
        first_item
    );
    assert_eq!(
        std::fs::read(first_code.join("CoflowTables.cs")).expect("re-read first C# generation"),
        first_tables
    );
    assert!(std::fs::read_to_string(requested_data.join("Item.json"))
        .expect("read second requested data")
        .contains("Elixir"));
    assert_eq!(
        std::fs::read(requested_code.join("CoflowTables.cs")).expect("read second requested code"),
        std::fs::read(second_code.join("CoflowTables.cs")).expect("read second C# generation")
    );
    let generation_root = root.join(".coflow/artifacts/generations");
    let canonical_generation_root =
        std::fs::canonicalize(&generation_root).expect("canonical generation root");
    for generation in [&first_data, &first_code, &second_data, &second_code] {
        assert!(
            std::fs::canonicalize(generation)
                .expect("canonical generation")
                .starts_with(&canonical_generation_root),
            "generation should be stored under .coflow: {}",
            generation.display()
        );
    }
    let versioned_lock: Value = serde_json::from_slice(
        &std::fs::read(root.join("coflow.enum.lock.json")).expect("read versioned enum lock"),
    )
    .expect("parse versioned enum lock");
    assert_eq!(versioned_lock, active_enum_lock(&root));

    assert_eq!(directory_count(&generation_root), 4);

    let abandoned_staging = root.join(".coflow/artifacts/staging/abandoned");
    std::fs::create_dir_all(&abandoned_staging).expect("create abandoned staging entry");
    std::fs::write(abandoned_staging.join("partial.txt"), "partial")
        .expect("write abandoned staging file");
    let clean = coflow()
        .args(["clean", root.to_str().expect("utf8 path")])
        .output()
        .expect("run clean");
    assert!(
        clean.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&clean.stdout),
        String::from_utf8_lossy(&clean.stderr)
    );
    assert!(String::from_utf8_lossy(&clean.stdout)
        .contains("Cleaned 2 historical generations and 1 staging entries"));
    assert_eq!(directory_count(&generation_root), 2);
    assert!(second_data.is_dir());
    assert!(second_code.is_dir());
    assert!(!first_data.exists());
    assert!(!first_code.exists());
    assert!(!abandoned_staging.exists());
}

fn directory_count(path: &std::path::Path) -> usize {
    std::fs::read_dir(path)
        .expect("read directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .count()
}

#[cfg(unix)]
fn create_directory_alias(alias: &std::path::Path, target: &std::path::Path) {
    std::os::unix::fs::symlink(target, alias).expect("create directory symlink");
}

#[cfg(windows)]
fn create_directory_alias(alias: &std::path::Path, target: &std::path::Path) {
    let output = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(alias)
        .arg(target)
        .output()
        .expect("create directory junction");
    assert!(
        output.status.success(),
        "failed to create junction: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
