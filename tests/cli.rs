#![allow(
    clippy::expect_used,
    clippy::needless_raw_string_hashes,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::too_many_lines,
    clippy::unwrap_used
)]

use serde_json::Value;
use std::io::{Read, Write};
use std::process::{ChildStdout, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const TEST_SEM_ENUM: u64 = 2;
const TEST_SEM_ENUM_MEMBER: u64 = 3;

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
    assert!(game_config.contains("namespace Game.Config;"));
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
        "type Item { id: string; }\n",
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
        "type Item { id: string; }\n",
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
fn schema_only_commands_do_not_require_excel_sources() {
    let root = temp_project_dir("schema-only-missing-source");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { @id id: string; value: int; }\n",
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
fn data_commands_require_excel_sources() {
    let root = temp_project_dir("data-missing-source");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        "type Item { @id id: string; value: int; }\n",
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
    let out_dir = root_dir.join("export");
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
            "export",
            "messagepack",
            project_dir.to_str().expect("utf8 temp path"),
            "--out",
            out_dir.to_str().expect("utf8 temp path"),
        ])
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
fn export_messagepack_validates_declared_output_type() {
    let out_dir = std::env::temp_dir().join(format!(
        "coflow-messagepack-validation-test-{}",
        std::process::id()
    ));
    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir).expect("clean old output dir");
    }

    let output = coflow()
        .args([
            "export",
            "messagepack",
            "examples/rpg",
            "--out",
            out_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("required `messagepack` for `coflow export messagepack`"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    if out_dir.exists() {
        std::fs::remove_dir_all(out_dir).expect("clean output dir");
    }
}

#[test]
fn codegen_csharp_writes_newtonsoft_json_loader() {
    let out_dir =
        std::env::temp_dir().join(format!("coflow-csharp-codegen-test-{}", std::process::id()));
    if out_dir.exists() {
        std::fs::remove_dir_all(&out_dir).expect("clean old output dir");
    }

    let output = coflow()
        .args([
            "codegen",
            "csharp",
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
        String::from_utf8_lossy(&output.stdout).contains("C# code generated to"),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let game_config =
        std::fs::read_to_string(out_dir.join("GameConfig.cs")).expect("GameConfig.cs");
    assert!(game_config.contains("using Newtonsoft.Json.Linq;"));
    assert!(game_config.contains("DuplicatePropertyNameHandling.Error"));
    assert!(game_config.contains("LoadRewardPolymorphic"));
    assert!(game_config.contains("ResolveRewardRefs(value.Rewards[i]"));

    let item_reward =
        std::fs::read_to_string(out_dir.join("ItemReward.cs")).expect("ItemReward.cs");
    assert!(item_reward.contains("public string ItemId { get; init; }"));
    assert!(item_reward.contains("public Item Item { get; internal set; }"));

    std::fs::remove_dir_all(out_dir).expect("clean output dir");
}

#[test]
fn codegen_csharp_uses_messagepack_loader_when_data_output_is_messagepack() {
    let suffix = unique_suffix();
    let root_dir = std::env::temp_dir().join(format!("coflow-csharp-messagepack-test-{suffix}"));
    let project_dir = root_dir.join("rpg");
    let out_dir = root_dir.join("csharp");
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
            "codegen",
            "csharp",
            project_dir.to_str().expect("utf8 temp path"),
            "--out",
            out_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let game_config =
        std::fs::read_to_string(out_dir.join("GameConfig.cs")).expect("GameConfig.cs");
    assert!(game_config.contains("using MessagePack;"));
    assert!(game_config.contains("Item.msgpack"));
    assert!(!game_config.contains("Newtonsoft.Json"));

    std::fs::remove_dir_all(root_dir).expect("clean temp dir");
}

#[test]
fn codegen_csharp_requires_data_output_config() {
    let suffix = unique_suffix();
    let root_dir = std::env::temp_dir().join(format!("coflow-csharp-missing-data-test-{suffix}"));
    let project_dir = root_dir.join("project");
    let out_dir = root_dir.join("csharp");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old temp dir");
    }
    std::fs::create_dir_all(project_dir.join("schema")).expect("create project dirs");
    std::fs::write(
        project_dir.join("schema").join("main.cft"),
        "type Item { value: int; }\n",
    )
    .expect("write schema");
    std::fs::write(
        project_dir.join("coflow.yaml"),
        "schema: schema/\noutputs:\n  code:\n    type: csharp\n    dir: generated/csharp\n",
    )
    .expect("write config");

    let output = coflow()
        .args([
            "codegen",
            "csharp",
            project_dir.to_str().expect("utf8 temp path"),
            "--out",
            out_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(
            "coflow.yaml missing outputs.data; required `type: json` or `type: messagepack` for `coflow codegen csharp`"
        ),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::remove_dir_all(root_dir).expect("clean temp dir");
}

#[test]
fn codegen_csharp_rejects_unsupported_data_output_type() {
    let suffix = unique_suffix();
    let root_dir =
        std::env::temp_dir().join(format!("coflow-csharp-unsupported-data-test-{suffix}"));
    let project_dir = root_dir.join("project");
    let out_dir = root_dir.join("csharp");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old temp dir");
    }
    std::fs::create_dir_all(project_dir.join("schema")).expect("create project dirs");
    std::fs::write(
        project_dir.join("schema").join("main.cft"),
        "type Item { value: int; }\n",
    )
    .expect("write schema");
    std::fs::write(
        project_dir.join("coflow.yaml"),
        "schema: schema/\noutputs:\n  data:\n    type: yaml\n    dir: generated/data\n  code:\n    type: csharp\n    dir: generated/csharp\n",
    )
    .expect("write config");

    let output = coflow()
        .args([
            "codegen",
            "csharp",
            project_dir.to_str().expect("utf8 temp path"),
            "--out",
            out_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("outputs.data.type is `yaml`; expected `json` or `messagepack`"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::remove_dir_all(root_dir).expect("clean temp dir");
}

#[test]
fn generated_csharp_compiles_and_loads_exported_json() {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    );
    let root_dir = std::env::temp_dir().join(format!("coflow-csharp-e2e-test-{suffix}"));
    let export_dir = root_dir.join("export");
    let csharp_dir = root_dir.join("csharp");
    let dotnet_dir = root_dir.join("dotnet");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old output dir");
    }
    std::fs::create_dir_all(&root_dir).expect("create temp root");

    let export_output = coflow()
        .args([
            "export",
            "json",
            "examples/rpg",
            "--out",
            export_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow export");
    assert!(
        export_output.status.success(),
        "export failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&export_output.stdout),
        String::from_utf8_lossy(&export_output.stderr)
    );

    let codegen_output = coflow()
        .args([
            "codegen",
            "csharp",
            "examples/rpg",
            "--namespace",
            "Game.Config",
            "--out",
            csharp_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow codegen");
    assert!(
        codegen_output.status.success(),
        "codegen failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&codegen_output.stdout),
        String::from_utf8_lossy(&codegen_output.stderr)
    );

    let new_output = Command::new("dotnet")
        .args([
            "new",
            "console",
            "--framework",
            "net8.0",
            "--output",
            dotnet_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run dotnet new");
    assert!(
        new_output.status.success(),
        "dotnet new failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&new_output.stdout),
        String::from_utf8_lossy(&new_output.stderr)
    );

    let add_package_output = Command::new("dotnet")
        .current_dir(&dotnet_dir)
        .args(["add", "package", "Newtonsoft.Json", "--version", "13.0.3"])
        .output()
        .expect("run dotnet add package");
    assert!(
        add_package_output.status.success(),
        "dotnet add package failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&add_package_output.stdout),
        String::from_utf8_lossy(&add_package_output.stderr)
    );

    for entry in std::fs::read_dir(&csharp_dir).expect("read generated C# dir") {
        let entry = entry.expect("generated C# entry");
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "cs") {
            std::fs::copy(
                &path,
                dotnet_dir.join(path.file_name().expect("generated C# file name")),
            )
            .expect("copy generated C# file");
        }
    }

    std::fs::write(
        dotnet_dir.join("Program.cs"),
        r#"using Game.Config;

var config = GameConfig.Load(args[0]);
if (config.Items.Count == 0)
{
    throw new Exception("expected items");
}
Console.WriteLine("loaded");
"#,
    )
    .expect("write Program.cs");

    let build_output = Command::new("dotnet")
        .current_dir(&dotnet_dir)
        .arg("build")
        .output()
        .expect("run dotnet build");
    assert!(
        build_output.status.success(),
        "dotnet build failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build_output.stdout),
        String::from_utf8_lossy(&build_output.stderr)
    );

    let run_output = Command::new("dotnet")
        .current_dir(&dotnet_dir)
        .args(["run", "--", export_dir.to_str().expect("utf8 temp path")])
        .output()
        .expect("run dotnet app");
    assert!(
        run_output.status.success(),
        "dotnet run failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run_output.stdout),
        String::from_utf8_lossy(&run_output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run_output.stdout).contains("loaded"),
        "dotnet run stdout: {}",
        String::from_utf8_lossy(&run_output.stdout)
    );

    std::fs::remove_dir_all(root_dir).expect("clean output dir");
}

#[test]
fn generated_csharp_compiles_and_loads_exported_messagepack() {
    let suffix = unique_suffix();
    let root_dir = std::env::temp_dir().join(format!("coflow-csharp-messagepack-e2e-{suffix}"));
    let project_dir = root_dir.join("rpg");
    let export_dir = root_dir.join("export");
    let csharp_dir = root_dir.join("csharp");
    let dotnet_dir = root_dir.join("dotnet");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old temp dir");
    }
    std::fs::create_dir_all(&root_dir).expect("create temp root");
    let _cleanup = TempDirCleanup(root_dir);

    copy_dir_recursive(std::path::Path::new("examples/rpg"), &project_dir)
        .expect("copy example project");
    let config_path = project_dir.join("coflow.yaml");
    let config = std::fs::read_to_string(&config_path).expect("read coflow.yaml");
    std::fs::write(
        &config_path,
        config.replacen("type: json", "type: messagepack", 1),
    )
    .expect("write coflow.yaml");

    let export_output = coflow()
        .args([
            "export",
            "messagepack",
            project_dir.to_str().expect("utf8 temp path"),
            "--out",
            export_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow export");
    assert!(
        export_output.status.success(),
        "export failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&export_output.stdout),
        String::from_utf8_lossy(&export_output.stderr)
    );

    let codegen_output = coflow()
        .args([
            "codegen",
            "csharp",
            project_dir.to_str().expect("utf8 temp path"),
            "--namespace",
            "Game.Config",
            "--out",
            csharp_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run coflow codegen");
    assert!(
        codegen_output.status.success(),
        "codegen failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&codegen_output.stdout),
        String::from_utf8_lossy(&codegen_output.stderr)
    );

    let new_output = Command::new("dotnet")
        .args([
            "new",
            "console",
            "--framework",
            "net8.0",
            "--output",
            dotnet_dir.to_str().expect("utf8 temp path"),
        ])
        .output()
        .expect("run dotnet new");
    assert!(
        new_output.status.success(),
        "dotnet new failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&new_output.stdout),
        String::from_utf8_lossy(&new_output.stderr)
    );

    let add_package_output = Command::new("dotnet")
        .current_dir(&dotnet_dir)
        .args(["add", "package", "MessagePack"])
        .output()
        .expect("run dotnet add package");
    assert!(
        add_package_output.status.success(),
        "dotnet add package failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&add_package_output.stdout),
        String::from_utf8_lossy(&add_package_output.stderr)
    );

    for entry in std::fs::read_dir(&csharp_dir).expect("read generated C# dir") {
        let entry = entry.expect("generated C# entry");
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "cs") {
            std::fs::copy(
                &path,
                dotnet_dir.join(path.file_name().expect("generated C# file name")),
            )
            .expect("copy generated C# file");
        }
    }

    std::fs::write(
        dotnet_dir.join("Program.cs"),
        r#"using Game.Config;

var config = GameConfig.Load(args[0]);
if (config.Items.Count == 0)
{
    throw new Exception("expected items");
}
Console.WriteLine("loaded");
"#,
    )
    .expect("write Program.cs");

    let build_output = Command::new("dotnet")
        .current_dir(&dotnet_dir)
        .arg("build")
        .output()
        .expect("run dotnet build");
    assert!(
        build_output.status.success(),
        "dotnet build failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build_output.stdout),
        String::from_utf8_lossy(&build_output.stderr)
    );

    let run_output = Command::new("dotnet")
        .current_dir(&dotnet_dir)
        .args(["run", "--", export_dir.to_str().expect("utf8 temp path")])
        .output()
        .expect("run dotnet app");
    assert!(
        run_output.status.success(),
        "dotnet run failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&run_output.stdout),
        String::from_utf8_lossy(&run_output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run_output.stdout).contains("loaded"),
        "dotnet run stdout: {}",
        String::from_utf8_lossy(&run_output.stdout)
    );
}

#[test]
fn cft_lsp_publishes_project_diagnostics_for_open_document() {
    let mut child = coflow()
        .args(["cft", "lsp", "examples/rpg"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let initialize = read_lsp_response(&mut stdout, 1);
    assert_eq!(initialize["id"], 1);
    assert!(initialize["result"]["capabilities"]["textDocumentSync"].is_object());

    let schema_path = std::fs::canonicalize("examples/rpg/schema/rpg.cft").expect("schema path");
    let uri = file_uri(&schema_path);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "cft",
                    "version": 1,
                    "text": "type Broken { missing: Missing; }"
                }
            }
        }),
    );
    let publish = read_lsp(&mut stdout);
    assert_eq!(publish["method"], "textDocument/publishDiagnostics");
    assert_eq!(publish["params"]["uri"], uri);
    let diagnostics = publish["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "CFT-SCHEMA-006"),
        "diagnostics: {diagnostics:?}"
    );

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "shutdown",
            "params": null
        }),
    );
    let shutdown = read_lsp_response(&mut stdout, 2);
    assert_eq!(shutdown["id"], 2);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        }),
    );
    assert_child_exits(&mut child);
}

#[test]
fn cft_lsp_prefers_open_document_uri_for_project_diagnostics() {
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    );
    let project_dir = std::env::temp_dir().join(format!("coflow lsp uri alias test {suffix}"));
    let schema_dir = project_dir.join("schema");
    if project_dir.exists() {
        std::fs::remove_dir_all(&project_dir).expect("clean old temp dir");
    }
    std::fs::create_dir_all(&schema_dir).expect("create schema dir");
    std::fs::write(project_dir.join("coflow.yaml"), "schema: schema/\n").expect("write config");
    let schema_path = schema_dir.join("main.cft");
    std::fs::write(&schema_path, "type Item { id: string; }\n").expect("write schema");

    let mut child = coflow()
        .args(["cft", "lsp", project_dir.to_str().expect("utf8 temp path")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let initialize = read_lsp_response(&mut stdout, 1);
    assert_eq!(initialize["id"], 1);

    let schema_path = std::fs::canonicalize(&schema_path).expect("schema path");
    let uri = file_uri(&schema_path);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "cft",
                    "version": 1,
                    "text": "type Broken { missing: Missing; }"
                }
            }
        }),
    );

    let publish = read_lsp(&mut stdout);
    assert_eq!(publish["method"], "textDocument/publishDiagnostics");
    assert_eq!(publish["params"]["uri"], uri);
    let diagnostics = publish["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "CFT-SCHEMA-006"),
        "diagnostics: {diagnostics:?}"
    );

    shutdown_lsp(stdin, &mut stdout, &mut child, 2);
    std::fs::remove_dir_all(project_dir).expect("clean temp dir");
}

#[test]
fn cft_lsp_definitions_survive_unrelated_schema_diagnostics() {
    let suffix = unique_suffix();
    let project_dir = std::env::temp_dir().join(format!("coflow-lsp-definition-test-{suffix}"));
    let _cleanup = TempDirCleanup(project_dir.clone());
    let schema_dir = project_dir.join("schema");
    std::fs::create_dir_all(&schema_dir).expect("create schema dir");
    std::fs::write(project_dir.join("coflow.yaml"), "schema: schema/\n").expect("write config");
    let source_path = schema_dir.join("source.cft");
    let target_path = schema_dir.join("target.cft");
    let broken_path = schema_dir.join("broken.cft");
    let source = "type UsesTarget { target: Target; }\n";
    let target = "type Target { id: string; }\n";
    std::fs::write(&source_path, source).expect("write source schema");
    std::fs::write(&target_path, target).expect("write target schema");
    std::fs::write(&broken_path, "type Broken { missing: Missing; }\n")
        .expect("write broken schema");

    let mut child = coflow()
        .args(["cft", "lsp", project_dir.to_str().expect("utf8 temp path")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let initialize = read_lsp_response(&mut stdout, 1);
    assert_eq!(initialize["id"], 1);

    let source_path = std::fs::canonicalize(&source_path).expect("source path");
    let source_uri = file_uri(&source_path);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": source_uri,
                    "languageId": "cft",
                    "version": 1,
                    "text": source
                }
            }
        }),
    );
    let publish = read_lsp(&mut stdout);
    assert_eq!(publish["method"], "textDocument/publishDiagnostics");

    let definitions = request_definition_at(
        &mut stdin,
        &mut stdout,
        2,
        &source_uri,
        source,
        position_after(source, "target: Target"),
    );
    let definitions = definitions.as_array().expect("definition array");
    assert!(
        definitions.iter().any(|location| {
            location["uri"]
                .as_str()
                .is_some_and(|uri| uri.ends_with("target.cft"))
        }),
        "definitions: {definitions:?}"
    );

    shutdown_lsp(stdin, &mut stdout, &mut child, 3);
}

#[test]
fn cft_lsp_enum_variant_definitions_survive_unrelated_schema_diagnostics() {
    let suffix = unique_suffix();
    let project_dir =
        std::env::temp_dir().join(format!("coflow-lsp-enum-definition-test-{suffix}"));
    let _cleanup = TempDirCleanup(project_dir.clone());
    let schema_dir = project_dir.join("schema");
    std::fs::create_dir_all(&schema_dir).expect("create schema dir");
    std::fs::write(project_dir.join("coflow.yaml"), "schema: schema/\n").expect("write config");
    let source_path = schema_dir.join("source.cft");
    let broken_path = schema_dir.join("broken.cft");
    let source = r#"enum ExampleRarity {
  Common = 0,
  Rare = 10,
}

type UsesEnum {
  rarity: ExampleRarity = ExampleRarity.Common;
  check {
    rarity >= ExampleRarity.Common;
  }
}
"#;
    std::fs::write(&source_path, source).expect("write source schema");
    std::fs::write(&broken_path, "type Broken { missing: Missing; }\n")
        .expect("write broken schema");

    let mut child = coflow()
        .args(["cft", "lsp", project_dir.to_str().expect("utf8 temp path")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let initialize = read_lsp_response(&mut stdout, 1);
    assert_eq!(initialize["id"], 1);

    let source_path = std::fs::canonicalize(&source_path).expect("source path");
    let source_uri = file_uri(&source_path);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": source_uri,
                    "languageId": "cft",
                    "version": 1,
                    "text": source
                }
            }
        }),
    );
    let publish = read_lsp(&mut stdout);
    assert_eq!(publish["method"], "textDocument/publishDiagnostics");

    let definitions = request_definition_at(
        &mut stdin,
        &mut stdout,
        2,
        &source_uri,
        source,
        position_after(source, "rarity >= ExampleRarity.Common"),
    );
    let definitions = definitions.as_array().expect("definition array");
    assert!(
        definitions.iter().any(|location| {
            location["uri"] == source_uri
                && location["range"]
                    == serde_json::json!({
                        "start": { "line": 1, "character": 2 },
                        "end": { "line": 1, "character": 8 }
                    })
        }),
        "definitions: {definitions:?}"
    );

    shutdown_lsp(stdin, &mut stdout, &mut child, 3);
}

#[test]
fn cft_lsp_semantic_tokens_classify_check_enum_values() {
    let suffix = unique_suffix();
    let project_dir = std::env::temp_dir().join(format!("coflow-lsp-semantic-test-{suffix}"));
    let _cleanup = TempDirCleanup(project_dir.clone());
    let schema_dir = project_dir.join("schema");
    std::fs::create_dir_all(&schema_dir).expect("create schema dir");
    std::fs::write(project_dir.join("coflow.yaml"), "schema: schema/\n").expect("write config");
    let source_path = schema_dir.join("source.cft");
    let source = r#"enum ExampleRarity {
  Common = 0,
}

enum ExampleDamageType {
  Physical = 0,
}

@flag
enum ExamplePermission {
  Read = 1,
}

type UsesEnum {
  rarity: ExampleRarity = ExampleRarity.Common;
  damage_type: ExampleDamageType = ExampleDamageType.Physical;
  permissions: ExamplePermission = ExamplePermission.Read;
  check {
    rarity >= ExampleRarity.Common;
    damage_type != ExampleDamageType.Physical;
    (permissions & ExamplePermission.Read) != ExamplePermission(0);
  }
}
"#;
    std::fs::write(&source_path, source).expect("write source schema");

    let mut child = coflow()
        .args(["cft", "lsp", project_dir.to_str().expect("utf8 temp path")])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let initialize = read_lsp_response(&mut stdout, 1);
    assert_eq!(initialize["id"], 1);

    let source_path = std::fs::canonicalize(&source_path).expect("source path");
    let source_uri = file_uri(&source_path);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": source_uri,
                    "languageId": "cft",
                    "version": 1,
                    "text": source
                }
            }
        }),
    );
    let publish = read_lsp(&mut stdout);
    assert_eq!(publish["method"], "textDocument/publishDiagnostics");

    let tokens = request_semantic_tokens(&mut stdin, &mut stdout, 2, &source_uri);
    assert_semantic_token_at(
        &tokens,
        source,
        position_after(source, "rarity >= ExampleRarity"),
        TEST_SEM_ENUM,
    );
    assert_semantic_token_at(
        &tokens,
        source,
        position_after(source, "rarity >= ExampleRarity.Common"),
        TEST_SEM_ENUM_MEMBER,
    );
    assert_semantic_token_at(
        &tokens,
        source,
        position_after(source, "damage_type != ExampleDamageType"),
        TEST_SEM_ENUM,
    );
    assert_semantic_token_at(
        &tokens,
        source,
        position_after(source, "damage_type != ExampleDamageType.Physical"),
        TEST_SEM_ENUM_MEMBER,
    );
    assert_semantic_token_at(
        &tokens,
        source,
        position_after(source, "permissions & ExamplePermission.Read"),
        TEST_SEM_ENUM_MEMBER,
    );
    assert_semantic_token_at(
        &tokens,
        source,
        position_after(source, "!= ExamplePermission"),
        TEST_SEM_ENUM,
    );

    shutdown_lsp(stdin, &mut stdout, &mut child, 3);
}

#[test]
fn cft_lsp_definitions_resolve_example_cross_file_enum_references() {
    let mut child = coflow()
        .args(["cft", "lsp", "examples/cft"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let initialize = read_lsp_response(&mut stdout, 1);
    assert_eq!(initialize["id"], 1);

    let schema_path =
        std::fs::canonicalize("examples/cft/03_types_fields_defaults.cft").expect("schema path");
    let uri = file_uri(&schema_path);
    let source = std::fs::read_to_string(&schema_path).expect("schema source");
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "cft",
                    "version": 1,
                    "text": source
                }
            }
        }),
    );
    let publish = read_lsp(&mut stdout);
    assert_eq!(publish["method"], "textDocument/publishDiagnostics");

    let enum_type_definition = request_definition_at(
        &mut stdin,
        &mut stdout,
        2,
        &uri,
        &source,
        position_after(&source, "rarity: ExampleRarity"),
    );
    assert_definition_uri_matches_path(
        &enum_type_definition,
        "examples/cft/02_enums_and_flags.cft",
    );

    let enum_variant_definition = request_definition_at(
        &mut stdin,
        &mut stdout,
        3,
        &uri,
        &source,
        position_after(&source, "ExampleRarity.Common"),
    );
    assert_definition_uri_matches_path(
        &enum_variant_definition,
        "examples/cft/02_enums_and_flags.cft",
    );

    let enum_type_definition_from_middle = request_definition_at(
        &mut stdin,
        &mut stdout,
        4,
        &uri,
        &source,
        position_inside(&source, "rarity: ExampleRarity", "ExampleRarity", 4),
    );
    assert_definition_uri_matches_path(
        &enum_type_definition_from_middle,
        "examples/cft/02_enums_and_flags.cft",
    );

    let enum_variant_definition_from_middle = request_definition_at(
        &mut stdin,
        &mut stdout,
        5,
        &uri,
        &source,
        position_inside(&source, "ExampleRarity.Common", "Common", 2),
    );
    assert_definition_uri_matches_path(
        &enum_variant_definition_from_middle,
        "examples/cft/02_enums_and_flags.cft",
    );

    shutdown_lsp(stdin, &mut stdout, &mut child, 6);
}

#[test]
fn cft_lsp_serves_editor_language_features() {
    let mut child = coflow()
        .args(["cft", "lsp", "examples/rpg"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn coflow lsp");

    let mut stdin = child.stdin.take().expect("lsp stdin");
    let mut stdout = child.stdout.take().expect("lsp stdout");

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        }),
    );
    let initialize = read_lsp_response(&mut stdout, 1);
    let capabilities = &initialize["result"]["capabilities"];
    assert!(capabilities["completionProvider"].is_object());
    assert_eq!(capabilities["documentFormattingProvider"], true);
    assert!(capabilities["semanticTokensProvider"].is_object());

    let schema_path = std::fs::canonicalize("examples/rpg/schema/rpg.cft").expect("schema path");
    let uri = file_uri(&schema_path);
    let source = std::fs::read_to_string(&schema_path)
        .expect("schema source")
        .replacen(
            "const MAX_LEVEL: int = 100;",
            "const MAX_LEVEL: int = 100; # comment",
            1,
        );
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "cft",
                    "version": 1,
                    "text": source
                }
            }
        }),
    );
    let publish = read_lsp(&mut stdout);
    assert_eq!(publish["method"], "textDocument/publishDiagnostics");

    let top_level_completion = request_completion_at(&mut stdin, &mut stdout, 2, &uri, &source, 0);
    assert_has_completion(&top_level_completion, "type");
    assert_missing_completion(&top_level_completion, "Monster");
    assert_missing_completion(&top_level_completion, "len");

    let type_ref_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        3,
        &uri,
        &source,
        position_after(&source, "stats: "),
    );
    assert_has_completion(&type_ref_completion, "Monster");
    assert_has_completion(&type_ref_completion, "int");
    assert_missing_completion(&type_ref_completion, "len");

    let field_default_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        4,
        &uri,
        &source,
        position_after(&source, "rarity: Rarity = "),
    );
    assert_has_completion(&field_default_completion, "Rarity.Common");
    assert_missing_completion(&field_default_completion, "true");
    assert_missing_completion(&field_default_completion, "MAX_LEVEL");
    assert_missing_completion(&field_default_completion, "len");

    let check_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        5,
        &uri,
        &source,
        position_after(&source, "    id != \"\";\n    "),
    );
    assert_has_completion(&check_completion, "len");
    assert_has_completion(&check_completion, "id");
    assert_missing_completion(&check_completion, "Monster");

    let ref_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        6,
        &uri,
        &source,
        position_after(&source, "@ref("),
    );
    assert_has_completion(&ref_completion, "Monster");
    assert_missing_completion(&ref_completion, "int");
    assert_missing_completion(&ref_completion, "Rarity");

    let string_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        7,
        &uri,
        &source,
        position_after(&source, "@display(\"Item"),
    );
    assert_no_completion(&string_completion);

    let enum_dot_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        8,
        &uri,
        &source,
        position_after(&source, "rarity: Rarity = Rarity."),
    );
    assert_has_completion(&enum_dot_completion, "Common");
    assert_missing_completion(&enum_dot_completion, "len");

    let field_dot_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        9,
        &uri,
        &source,
        position_after(&source, "stats."),
    );
    assert_has_completion(&field_dot_completion, "hp");
    assert_missing_completion(&field_dot_completion, "Monster");

    let type_predicate_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        10,
        &uri,
        &source,
        position_after(&source, "reward is "),
    );
    assert_has_completion(&type_predicate_completion, "ItemReward");
    assert_has_completion(&type_predicate_completion, "null");
    assert_missing_completion(&type_predicate_completion, "len");

    let parent_type_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        11,
        &uri,
        &source,
        position_after(&source, "type ItemReward : "),
    );
    assert_has_completion(&parent_type_completion, "Reward");
    assert_missing_completion(&parent_type_completion, "int");

    let abstract_keyword_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        12,
        &uri,
        &source,
        position_after(&source, "abstract "),
    );
    assert_has_completion(&abstract_keyword_completion, "type");
    assert_missing_completion(&abstract_keyword_completion, "enum");

    let comment_completion = request_completion_at(
        &mut stdin,
        &mut stdout,
        13,
        &uri,
        &source,
        position_after(&source, "# comment"),
    );
    assert_no_completion(&comment_completion);

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 14,
            "method": "textDocument/formatting",
            "params": {
                "textDocument": { "uri": uri },
                "options": { "tabSize": 2, "insertSpaces": true }
            }
        }),
    );
    let formatting = read_lsp_response(&mut stdout, 14);
    assert!(
        formatting["result"].is_array(),
        "formatting: {formatting:?}"
    );

    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 15,
            "method": "textDocument/semanticTokens/full",
            "params": {
                "textDocument": { "uri": uri }
            }
        }),
    );
    let semantic = read_lsp_response(&mut stdout, 15);
    assert!(
        semantic["result"]["data"]
            .as_array()
            .is_some_and(|data| !data.is_empty()),
        "semantic: {semantic:?}"
    );

    shutdown_lsp(stdin, &mut stdout, &mut child, 16);
}

fn unique_suffix() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    )
}

fn temp_project_dir(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("coflow-{name}-{}", unique_suffix()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("clean old temp dir");
    }
    root
}

struct TempDirCleanup(std::path::PathBuf);

impl Drop for TempDirCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn request_completion(
    stdin: &mut impl Write,
    stdout: &mut ChildStdout,
    id: u64,
    uri: &str,
    line: u64,
    character: u64,
) -> Value {
    write_lsp(
        stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }),
    );
    read_lsp_response(stdout, id)["result"].clone()
}

fn request_completion_at(
    stdin: &mut impl Write,
    stdout: &mut ChildStdout,
    id: u64,
    uri: &str,
    source: &str,
    byte_offset: usize,
) -> Value {
    let (line, character) = lsp_position(source, byte_offset);
    request_completion(stdin, stdout, id, uri, line, character)
}

fn request_definition(
    stdin: &mut impl Write,
    stdout: &mut ChildStdout,
    id: u64,
    uri: &str,
    line: u64,
    character: u64,
) -> Value {
    write_lsp(
        stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/definition",
            "params": {
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }
        }),
    );
    read_lsp_response(stdout, id)["result"].clone()
}

fn request_definition_at(
    stdin: &mut impl Write,
    stdout: &mut ChildStdout,
    id: u64,
    uri: &str,
    source: &str,
    byte_offset: usize,
) -> Value {
    let (line, character) = lsp_position(source, byte_offset);
    request_definition(stdin, stdout, id, uri, line, character)
}

fn request_semantic_tokens(
    stdin: &mut impl Write,
    stdout: &mut ChildStdout,
    id: u64,
    uri: &str,
) -> Vec<TestSemanticToken> {
    write_lsp(
        stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "textDocument/semanticTokens/full",
            "params": {
                "textDocument": { "uri": uri }
            }
        }),
    );
    let response = read_lsp_response(stdout, id);
    let data = response["result"]["data"]
        .as_array()
        .unwrap_or_else(|| panic!("semantic token data: {response:?}"));
    assert_eq!(data.len() % 5, 0, "semantic token data: {data:?}");

    let mut line = 0_u64;
    let mut character = 0_u64;
    let mut tokens = Vec::new();
    for chunk in data.chunks(5) {
        let delta_line = chunk[0].as_u64().expect("delta line");
        let delta_start = chunk[1].as_u64().expect("delta start");
        line += delta_line;
        if delta_line == 0 {
            character += delta_start;
        } else {
            character = delta_start;
        }
        tokens.push(TestSemanticToken {
            line,
            character,
            length: chunk[2].as_u64().expect("length"),
            token_type: chunk[3].as_u64().expect("token type"),
        });
    }
    tokens
}

#[derive(Debug)]
struct TestSemanticToken {
    line: u64,
    character: u64,
    length: u64,
    token_type: u64,
}

fn assert_semantic_token_at(
    tokens: &[TestSemanticToken],
    source: &str,
    byte_offset: usize,
    token_type: u64,
) {
    let (line, character) = lsp_position(source, byte_offset);
    assert!(
        tokens.iter().any(|token| {
            token.line == line
                && token.character <= character
                && character <= token.character + token.length
                && token.token_type == token_type
        }),
        "expected token type {token_type} at {line}:{character} in {tokens:?}"
    );
}

fn assert_definition_uri_matches_path(definitions: &Value, path: &str) {
    let path = std::fs::canonicalize(path).expect("definition target path");
    let expected = file_uri(&path);
    let definitions = definitions.as_array().expect("definition array");
    assert!(
        definitions
            .iter()
            .any(|location| location["uri"].as_str() == Some(expected.as_str())),
        "expected definition URI `{expected}` in {definitions:?}"
    );
}

fn position_after(source: &str, needle: &str) -> usize {
    find_line_ending_insensitive(source, needle)
        .unwrap_or_else(|| panic!("source should contain `{needle}`"))
}

fn position_inside(source: &str, context: &str, needle: &str, character_offset: usize) -> usize {
    let context_end = position_after(source, context);
    let context_start = context_end - context.len();
    let relative = context
        .find(needle)
        .unwrap_or_else(|| panic!("context `{context}` should contain `{needle}`"));
    context_start + relative + character_offset.min(needle.len())
}

fn find_line_ending_insensitive(source: &str, needle: &str) -> Option<usize> {
    let source_bytes = source.as_bytes();
    for start in source.char_indices().map(|(index, _)| index) {
        let mut source_index = start;
        let mut needle_index = 0;
        while needle_index < needle.len() {
            let needle_char = needle[needle_index..].chars().next()?;
            if needle_char == '\n'
                && source_bytes.get(source_index) == Some(&b'\r')
                && source_bytes.get(source_index + 1) == Some(&b'\n')
            {
                source_index += 2;
                needle_index += 1;
                continue;
            }
            let source_char = source[source_index..].chars().next()?;
            if source_char != needle_char {
                break;
            }
            source_index += source_char.len_utf8();
            needle_index += needle_char.len_utf8();
        }
        if needle_index == needle.len() {
            return Some(source_index);
        }
    }
    None
}

fn lsp_position(source: &str, byte_offset: usize) -> (u64, u64) {
    let target = byte_offset.min(source.len());
    let mut line = 0_u64;
    let mut character = 0_u64;
    for (index, ch) in source.char_indices() {
        if index >= target {
            break;
        }
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u64;
        }
    }
    (line, character)
}

fn assert_has_completion(completion: &Value, label: &str) {
    let items = completion.as_array().expect("completion array");
    assert!(
        items.iter().any(|item| item["label"] == label),
        "expected completion `{label}` in {items:?}"
    );
}

fn assert_missing_completion(completion: &Value, label: &str) {
    let items = completion.as_array().expect("completion array");
    assert!(
        !items.iter().any(|item| item["label"] == label),
        "unexpected completion `{label}` in {items:?}"
    );
}

fn assert_no_completion(completion: &Value) {
    let items = completion.as_array().expect("completion array");
    assert!(items.is_empty(), "expected no completion in {items:?}");
}

fn write_lsp(stdin: &mut impl Write, value: &Value) {
    let body = serde_json::to_vec(value).expect("serialize lsp message");
    write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("write lsp header");
    stdin.write_all(&body).expect("write lsp body");
    stdin.flush().expect("flush lsp");
}

fn shutdown_lsp(
    mut stdin: impl Write,
    stdout: &mut ChildStdout,
    child: &mut std::process::Child,
    id: u64,
) {
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "shutdown",
            "params": null
        }),
    );
    let shutdown = read_lsp_response(stdout, id);
    assert_eq!(shutdown["id"], id);
    write_lsp(
        &mut stdin,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "exit",
            "params": null
        }),
    );
    stdin.flush().expect("flush exit");
    assert_child_exits(child);
}

fn assert_child_exits(child: &mut std::process::Child) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().expect("poll lsp") {
            assert!(status.success(), "lsp exit status: {status}");
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("lsp did not exit after exit notification");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn read_lsp_response(stdout: &mut ChildStdout, id: u64) -> Value {
    loop {
        let message = read_lsp(stdout);
        if message["id"] == id {
            return message;
        }
    }
}

fn read_lsp(stdout: &mut ChildStdout) -> Value {
    let mut header = Vec::new();
    let mut byte = [0; 1];

    while !header.ends_with(b"\r\n\r\n") {
        stdout.read_exact(&mut byte).expect("read lsp header");
        header.push(byte[0]);
    }

    let header = String::from_utf8(header).expect("utf8 header");
    let content_length = header
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length:"))
        .expect("content length")
        .trim()
        .parse::<usize>()
        .expect("parse content length");
    let mut body = vec![0; content_length];
    stdout.read_exact(&mut body).expect("read lsp body");
    serde_json::from_slice(&body).expect("parse lsp body")
}

fn file_uri(path: &std::path::Path) -> String {
    let mut path = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        if let Some(stripped) = path.strip_prefix("//?/") {
            path = stripped.to_string();
        }
    }
    if cfg!(windows) && path.len() >= 2 && path.as_bytes()[1] == b':' {
        path.insert(0, '/');
    }
    format!("file://{path}")
}

fn copy_dir_recursive(source: &std::path::Path, target: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else {
            std::fs::copy(source_path, target_path)?;
        }
    }
    Ok(())
}
