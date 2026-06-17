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
    assert!(game_config.contains("ResolveDialogueNodeRefs(list1[i1]"));

    let item_reward =
        std::fs::read_to_string(out_dir.join("ItemReward.cs")).expect("ItemReward.cs");
    assert!(item_reward.contains("public string Id { get; internal set; }"));
    assert!(!item_reward.contains("public string ItemId { get; set; }"));
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
fn codegen_csharp_preflight_outputs_multiple_diagnostics_without_writing_files() {
    let root = temp_project_dir("codegen-preflight");
    let _cleanup = TempDirCleanup(root.clone());
    std::fs::create_dir_all(root.join("schema")).expect("create schema dir");
    std::fs::create_dir_all(root.join("generated").join("csharp")).expect("create code dir");
    std::fs::write(
        root.join("schema").join("main.cft"),
        r#"
            type FooBar { value: int; }
            @keyAsEnum("GeneId")
            type Foo_Bar {
                foo_bar: int;
                fooBar: int;
            }
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
    assert!(stderr.contains("generated C# file name `FooBar.cs` collides"));
    assert!(stderr.contains("generated C# member name `FooBar` collides"));
    assert!(!stderr.contains("failed to parse"));
    assert_eq!(
        std::fs::read_to_string(&lockfile).expect("lockfile remains"),
        "{bad json"
    );
    assert!(!root
        .join("generated")
        .join("csharp")
        .join("GameConfig.cs")
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
Expect(config.Items.Count == 3, "expected 3 items");
Expect(config.Equipments.Count == 4, "expected 4 equipment rows");
Expect(config.DropTables.Count == 3, "expected 3 drop tables");
Expect(config.Stages.Count == 3, "expected 3 stages");
Expect(config.Quests.Count == 3, "expected 3 quests");
Expect(config.Shops.Count == 3, "expected 3 shops");

Expect(config.FindItem("healing_potion")?.FeaturedStage?.Id == "stage_forest_road", "item to CFD stage ref failed");
Expect(config.FindEquipment("flame_staff")?.FeaturedStage?.Id == "stage_arcane_tower", "equipment to CFD stage ref failed");

var arcaneStage = config.FindStage("stage_arcane_tower") ?? throw new Exception("missing stage_arcane_tower");
Expect(arcaneStage.DropTable.Id == "drop_fire_mage", "stage to CFD drop table ref failed");
Expect(arcaneStage.Spawns[0].Monster.Id == "fire_mage", "stage spawn to Excel monster ref failed");
Expect(arcaneStage.FirstClearReward is SkillUnlockReward { Skill.Id: "fireball" }, "inline polymorphic reward ref failed");

var dragonDrop = config.FindDropTable("drop_ancient_dragon") ?? throw new Exception("missing drop_ancient_dragon");
Expect(dragonDrop.Monster.Id == "ancient_dragon", "drop table to Excel monster ref failed");
Expect(dragonDrop.Rewards[0] is ItemReward { Item.Id: "phoenix_feather" }, "drop table reward item ref failed");
Expect(dragonDrop.Rewards[2] is SkillUnlockReward { Skill.Id: "meteor" }, "drop table reward skill ref failed");

var arcaneShop = config.FindShop("shop_arcane") ?? throw new Exception("missing shop_arcane");
Expect(arcaneShop.StageGate?.Id == "stage_arcane_tower", "shop to CFD stage ref failed");
Expect(arcaneShop.Entries[0].Item.Id == "flame_staff", "shop entry to Excel equipment ref failed");
Expect(arcaneShop.Entries[0].RequiredQuest?.Id == "quest_mage_trial", "shop entry to CFD quest ref failed");

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
