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
fn codegen_csharp_writes_newtonsoft_json_loader() {
    let root = temp_project_dir("csharp-codegen");
    let _cleanup = TempDirCleanup(root.clone());
    write_acyclic_csharp_project(&root, "json");
    let out_dir = root.join("csharp");

    let output = coflow()
        .args([
            "codegen",
            "csharp",
            root.to_str().expect("utf8 temp path"),
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

    let coflow_tables =
        std::fs::read_to_string(out_dir.join("CoflowTables.cs")).expect("CoflowTables.cs");
    assert!(coflow_tables.contains("using Newtonsoft.Json.Linq;"));
    assert!(coflow_tables.contains("public Table<string, Item> TbItem { get; }"));
    assert!(coflow_tables.contains("public TRecord Get(TKey id)"));
    assert!(!coflow_tables.contains("CftLoadException"));
    assert!(!out_dir.join("GameConfig.cs").exists());
    assert!(!out_dir.join("CftLoadException.cs").exists());

    let item = std::fs::read_to_string(out_dir.join("Item.cs")).expect("Item.cs");
    assert!(item.contains("public string Id { get; }"));
    assert!(item.contains("public string DisplayName { get; }"));
    assert!(item.contains("public Reward Reward { get; }"));
    assert!(!item.contains("set;"));
    assert!(item.contains("context.GetReward"));
}

#[test]
fn codegen_csharp_uses_messagepack_loader_when_data_output_is_messagepack() {
    let suffix = unique_suffix();
    let root_dir = std::env::temp_dir().join(format!("coflow-csharp-messagepack-test-{suffix}"));
    let project_dir = root_dir.join("project");
    let out_dir = root_dir.join("csharp");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old temp dir");
    }
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    write_acyclic_csharp_project(&project_dir, "messagepack");

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
    let coflow_tables =
        std::fs::read_to_string(out_dir.join("CoflowTables.cs")).expect("CoflowTables.cs");
    assert!(coflow_tables.contains("using MessagePack;"));
    assert!(coflow_tables.contains("Item.msgpack"));
    assert!(!coflow_tables.contains("Newtonsoft.Json"));

    std::fs::remove_dir_all(root_dir).expect("clean temp dir");
}

#[test]
fn codegen_csharp_preflight_outputs_multiple_diagnostics_without_writing_files() {
    let root = temp_project_dir("codegen-preflight");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("generated").join("csharp")).expect("create code dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            type class { value: int; }
            @idAsEnum(GeneId)
            type Foo_Bar {
                namespace: int;
            }
            enum namespace { Value }
            enum GeneId {}
        "#,
    )
    .expect("write schema");
    std::fs::write(
        root.join("coflow.yaml"),
        r"schema: schema/
outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.1Bad
",
    )
    .expect("write config");
    let lockfile = root.join("coflow.enum.lock.json");
    std::fs::write(&lockfile, "{bad json").expect("write malformed lockfile");

    let output = coflow()
        .args(["codegen", "csharp", root.to_str().expect("utf8 path")])
        .output()
        .expect("run codegen");

    assert!(
        !output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("[CODEGEN-CSHARP-001] [CODEGEN]"));
    assert!(stderr.contains("invalid C# namespace `Game.1Bad`"));
    assert!(stderr.contains("invalid C# type name `class`"));
    assert!(stderr.contains("invalid C# enum name `namespace`"));
    assert!(!stderr.contains("failed to parse"));
    assert_eq!(
        std::fs::read_to_string(&lockfile).expect("lockfile remains"),
        "{bad json"
    );
    assert!(!root
        .join("generated")
        .join("csharp")
        .join("CoflowTables.cs")
        .exists());
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
        String::from_utf8_lossy(&output.stderr).contains("coflow.yaml missing outputs.data"),
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
            .contains("code generator `csharp` does not support data format `yaml`"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::remove_dir_all(root_dir).expect("clean temp dir");
}

#[test]
fn generated_csharp_compiles_and_loads_exported_json() {
    if std::env::var_os("COFLOW_RUN_DOTNET_E2E").is_none() {
        eprintln!("skipping dotnet E2E test; set COFLOW_RUN_DOTNET_E2E=1 to run");
        return;
    }

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
    let project_dir = root_dir.join("project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    write_acyclic_csharp_project(&project_dir, "json");

    let export_output = coflow()
        .args([
            "export",
            "json",
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

var tables = CoflowTables.Load(args[0]);
Expect(tables.TbReward.Count == 1, "expected 1 reward");
Expect(tables.TbItem.Count == 1, "expected 1 item");
Expect(tables.TbBundle.Count == 1, "expected 1 bundle");
Expect(tables.TbEmptyThing.Count == 0, "expected missing empty JSON table to load as empty");

var potion = tables.TbItem.Get("potion");
Expect(potion.DisplayName == "Potion", "item field failed");
Expect(potion.Reward.Id == "reward_small", "item ref failed");
Expect(tables.TbItem.Find("missing") is null, "missing item should be null");
Expect(tables.TbItem.TryGet("potion", out var found) && found == potion, "try get failed");
Expect(tables.TbBundle.Get("starter").Item.Id == "potion", "bundle ref failed");

Console.WriteLine("loaded");

static void Expect(bool condition, string message)
{
    if (!condition)
    {
        throw new Exception(message);
    }
}
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
    if std::env::var_os("COFLOW_RUN_DOTNET_E2E").is_none() {
        eprintln!("skipping dotnet E2E test; set COFLOW_RUN_DOTNET_E2E=1 to run");
        return;
    }

    let suffix = unique_suffix();
    let root_dir = std::env::temp_dir().join(format!("coflow-csharp-messagepack-e2e-{suffix}"));
    let project_dir = root_dir.join("project");
    let export_dir = root_dir.join("export");
    let csharp_dir = root_dir.join("csharp");
    let dotnet_dir = root_dir.join("dotnet");
    if root_dir.exists() {
        std::fs::remove_dir_all(&root_dir).expect("clean old temp dir");
    }
    std::fs::create_dir_all(&root_dir).expect("create temp root");
    let _cleanup = TempDirCleanup(root_dir);

    std::fs::create_dir_all(&project_dir).expect("create project dir");
    write_acyclic_csharp_project(&project_dir, "messagepack");

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
        .args(["add", "package", "MessagePack", "--version", "3.1.4"])
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

var tables = CoflowTables.Load(args[0]);
if (tables.TbItem.Count == 0)
{
    throw new Exception("expected items");
}
if (tables.TbItem.Get("potion").Reward.Id != "reward_small")
{
    throw new Exception("expected resolved reward");
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
