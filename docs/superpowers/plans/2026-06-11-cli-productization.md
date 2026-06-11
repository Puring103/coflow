# CLI Productization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `coflow build`, stricter project configuration validation, CI, and a root README with quick start and CFT syntax overview.

**Architecture:** Keep orchestration in `src/main.rs`, where existing project commands already live. Put reusable configuration validation in `crates/coflow-project` so all project commands reject invalid config consistently. Add CI and README as repository-level artifacts.

**Tech Stack:** Rust, clap, serde/serde_yaml, Cargo test integration tests, GitHub Actions, Markdown.

---

## File Structure

- Modify `crates/coflow-project/src/lib.rs`: add strict serde field handling and project configuration validation called by `Project::open`.
- Modify `src/main.rs`: add `build` command and reuse existing compile/load/export/codegen helpers.
- Modify `tests/cli.rs`: add behavior tests for `build` and config validation.
- Create `.github/workflows/ci.yml`: Rust CI workflow.
- Create `README.md`: project introduction, quick start, CFT syntax overview, and command reference.
- Modify `docs/superpowers/specs/2026-06-11-cli-productization-design.md`: already created design source for this work.

---

## Task 1: Add Failing CLI Tests

**Files:**

- Modify: `tests/cli.rs`

- [ ] **Step 1: Add tests for build and config validation**

Add integration tests near the existing project CLI tests:

```rust
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
    std::fs::write(project_dir.join("schema").join("main.cft"), "type Item { id: string; }\n")
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
    std::fs::write(project_dir.join("schema").join("main.cft"), "type Item { id: string; }\n")
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
        "schema: schema/\nsources:\n  - file: data/missing.xlsx\n    sheets:\n      - sheet: \"\"\n        columns:\n          A: id\n          A: name\n",
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
```

- [ ] **Step 2: Run tests to verify RED**

Run:

```powershell
cargo test --test cli build_exports_data_and_generates_csharp_for_json_project build_exports_messagepack_when_configured config_validation_rejects_unknown_fields_and_invalid_outputs config_validation_rejects_invalid_sources_and_sheets
```

Expected: FAIL because `build` is not a known subcommand and validation rules are not implemented.

---

## Task 2: Implement Config Validation

**Files:**

- Modify: `crates/coflow-project/src/lib.rs`

- [ ] **Step 1: Add strict serde and validation call**

Add `#[serde(deny_unknown_fields)]` to `ProjectConfig`, `SourceConfig`, `SheetConfig`, `OutputsConfig`, and `OutputConfig`.

In `Project::open`, after parsing YAML, call:

```rust
validate_project_config(&root_dir, &config)?;
```

- [ ] **Step 2: Add validation helpers**

Add helper functions in `crates/coflow-project/src/lib.rs`:

```rust
fn validate_project_config(root_dir: &Path, config: &ProjectConfig) -> Result<(), String> {
    validate_schema_config(root_dir, &config.schema)?;
    validate_sources(root_dir, &config.sources)?;
    validate_outputs(&config.outputs)?;
    Ok(())
}

fn validate_schema_config(root_dir: &Path, schema: &SchemaConfig) -> Result<(), String> {
    match schema {
        SchemaConfig::One(path) => validate_schema_path(root_dir, path, "schema"),
        SchemaConfig::Many(paths) => {
            if paths.is_empty() {
                return Err("schema list is empty".to_string());
            }
            for (index, path) in paths.iter().enumerate() {
                validate_schema_path(root_dir, path, &format!("schema[{index}]"))?;
            }
            Ok(())
        }
    }
}

fn validate_schema_path(root_dir: &Path, path: &Path, label: &str) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        return Err(format!("{label} path is empty"));
    }
    let resolved = resolve_project_relative(root_dir, path);
    if !resolved.exists() {
        return Err(format!("{label} path `{}` does not exist", path.display()));
    }
    Ok(())
}

fn validate_sources(root_dir: &Path, sources: &[SourceConfig]) -> Result<(), String> {
    for (source_index, source) in sources.iter().enumerate() {
        let source_label = format!("sources[{source_index}]");
        if source.file.as_os_str().is_empty() {
            return Err(format!("{source_label}.file is empty"));
        }
        if !resolve_project_relative(root_dir, &source.file).is_file() {
            return Err(format!(
                "{source_label}.file `{}` does not exist",
                source.file.display()
            ));
        }
        if source.sheets.is_empty() {
            return Err(format!("{source_label}.sheets is empty"));
        }
        for (sheet_index, sheet) in source.sheets.iter().enumerate() {
            let sheet_label = format!("{source_label}.sheets[{sheet_index}]");
            if sheet.sheet.trim().is_empty() {
                return Err(format!("{sheet_label}.sheet is empty"));
            }
            if let Some(type_name) = &sheet.type_name {
                if type_name.trim().is_empty() {
                    return Err(format!("{sheet_label}.type is empty"));
                }
            }
        }
    }
    Ok(())
}

fn validate_outputs(outputs: &OutputsConfig) -> Result<(), String> {
    if let Some(data) = &outputs.data {
        if !matches!(data.output_type.as_str(), "json" | "messagepack") {
            return Err(format!(
                "outputs.data.type is `{}`; expected `json` or `messagepack`",
                data.output_type
            ));
        }
        validate_output_dir("outputs.data.dir", &data.dir)?;
        if data.namespace.is_some() {
            return Err("outputs.data.namespace is only valid for code outputs".to_string());
        }
    }
    if let Some(code) = &outputs.code {
        if code.output_type != "csharp" {
            return Err(format!(
                "outputs.code.type is `{}`; expected `csharp`",
                code.output_type
            ));
        }
        validate_output_dir("outputs.code.dir", &code.dir)?;
        if let Some(namespace) = &code.namespace {
            if namespace.trim().is_empty() {
                return Err("outputs.code.namespace is empty".to_string());
            }
        }
    }
    Ok(())
}

fn validate_output_dir(label: &str, path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty() {
        Err(format!("{label} is empty"))
    } else {
        Ok(())
    }
}

fn resolve_project_relative(root_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root_dir.join(path)
    }
}
```

- [ ] **Step 3: Run validation tests**

Run:

```powershell
cargo test --test cli config_validation_rejects_unknown_fields_and_invalid_outputs config_validation_rejects_invalid_sources_and_sheets
```

Expected: PASS for config validation tests.

---

## Task 3: Implement Build Command

**Files:**

- Modify: `src/main.rs`

- [ ] **Step 1: Add build CLI type**

Add `Build(BuildArgs)` to `Command` and `BuildArgs`:

```rust
#[derive(Debug, Args)]
struct BuildArgs {
    #[arg(value_name = "CONFIG_OR_DIR")]
    config_or_dir: Option<PathBuf>,
    /// Override outputs.data.dir for this invocation.
    #[arg(long = "data-out", value_name = "DIR")]
    data_out_dir: Option<PathBuf>,
    /// Override outputs.code.dir for this invocation.
    #[arg(long = "code-out", value_name = "DIR")]
    code_out_dir: Option<PathBuf>,
    /// Override outputs.code.namespace for this invocation.
    #[arg(long, value_name = "NAME")]
    namespace: Option<String>,
}
```

Add match arm:

```rust
Command::Build(args) => project_build(&args),
```

- [ ] **Step 2: Extract shared export and codegen writers**

Add helpers:

```rust
fn write_json_tables(
    schema: &CftContainer,
    load_output: &ExcelLoadOutput,
    dir: &Path,
) -> Result<(), String> {
    let tables = export_json_model(schema, &load_output.model)
        .map_err(|err| format!("failed to export JSON model: {err}"))?;
    fs::create_dir_all(dir)
        .map_err(|err| format!("failed to create output dir `{}`: {err}", dir.display()))?;
    for (table, value) in tables {
        let path = dir.join(format!("{table}.json"));
        let file = fs::File::create(&path)
            .map_err(|err| format!("failed to create `{}`: {err}", path.display()))?;
        serde_json::to_writer_pretty(file, &value)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    Ok(())
}

fn write_messagepack_tables(
    schema: &CftContainer,
    load_output: &ExcelLoadOutput,
    dir: &Path,
) -> Result<(), String> {
    let tables = export_messagepack_model(schema, &load_output.model)
        .map_err(|err| format!("failed to export MessagePack model: {err}"))?;
    fs::create_dir_all(dir)
        .map_err(|err| format!("failed to create output dir `{}`: {err}", dir.display()))?;
    for (table, bytes) in tables {
        let path = dir.join(format!("{table}.msgpack"));
        fs::write(&path, bytes)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    Ok(())
}

fn write_csharp_files(
    schema: &CftContainer,
    data_output_type: &str,
    namespace: &str,
    dir: &Path,
) -> Result<(), String> {
    let generate = match data_output_type {
        "json" => generate_csharp_json,
        "messagepack" => generate_csharp_messagepack,
        other => {
            return Err(format!(
                "coflow.yaml outputs.data.type is `{other}`; required `json` or `messagepack` for C# codegen"
            ));
        }
    };
    let options = CsharpCodegenOptions::new(namespace);
    let files =
        generate(schema, &options).map_err(|err| format!("failed to generate C# code: {err}"))?;
    fs::create_dir_all(dir)
        .map_err(|err| format!("failed to create output dir `{}`: {err}", dir.display()))?;
    for file in files {
        let path = dir.join(&file.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create `{}`: {err}", parent.display()))?;
        }
        fs::write(&path, file.contents)
            .map_err(|err| format!("failed to write `{}`: {err}", path.display()))?;
    }
    Ok(())
}
```

Update existing `export_json`, `export_messagepack`, and `codegen_csharp` to call these helpers.

- [ ] **Step 3: Add project_build**

Add:

```rust
fn project_build(args: &BuildArgs) -> Result<bool, String> {
    let project = Project::open(args.config_or_dir.as_deref())?;
    let data_output = project.config.outputs.data.as_ref().ok_or_else(|| {
        "coflow.yaml missing outputs.data; required `type: json` or `type: messagepack` for `coflow build`"
            .to_string()
    })?;
    let Some(schema) = compile_project_schema(&project, false)? else {
        return Ok(false);
    };
    let Some(load_output) = load_project_excel(&project, &schema, false)? else {
        return Ok(false);
    };

    let data_dir = output_dir(&project, data_output, args.data_out_dir.as_deref());
    match data_output.output_type.as_str() {
        "json" => {
            write_json_tables(&schema, &load_output, &data_dir)?;
            println!("JSON data exported to {}", data_dir.display());
        }
        "messagepack" => {
            write_messagepack_tables(&schema, &load_output, &data_dir)?;
            println!("MessagePack data exported to {}", data_dir.display());
        }
        other => {
            return Err(format!(
                "coflow.yaml outputs.data.type is `{other}`; expected `json` or `messagepack`"
            ));
        }
    }

    if let Some(code_output) = project.config.outputs.code.as_ref() {
        if code_output.output_type != "csharp" {
            return Err(format!(
                "coflow.yaml outputs.code.type is `{}`; expected `csharp`",
                code_output.output_type
            ));
        }
        let code_dir = output_dir(&project, code_output, args.code_out_dir.as_deref());
        let namespace = args
            .namespace
            .as_deref()
            .or(code_output.namespace.as_deref())
            .unwrap_or("Game.Config");
        write_csharp_files(&schema, &data_output.output_type, namespace, &code_dir)?;
        println!("C# code generated to {}", code_dir.display());
    }

    println!("Build completed: {}", project.config_path.display());
    Ok(true)
}
```

- [ ] **Step 4: Run build tests**

Run:

```powershell
cargo test --test cli build_exports_data_and_generates_csharp_for_json_project build_exports_messagepack_when_configured
```

Expected: PASS.

---

## Task 4: Add CI Workflow

**Files:**

- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Create CI workflow**

Create:

```yaml
name: CI

on:
  push:
  pull_request:

jobs:
  rust:
    name: Rust
    runs-on: windows-latest

    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - name: Check formatting
        run: cargo fmt --all -- --check

      - name: Clippy
        run: cargo clippy --workspace --all-targets -- -D warnings

      - name: Test
        run: cargo test --workspace
```

- [ ] **Step 2: Locally run equivalent commands**

Run:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: PASS.

---

## Task 5: Add README

**Files:**

- Create: `README.md`

- [ ] **Step 1: Write README**

Create a concise README with these sections:

```markdown
# Coflow

Coflow is a game configuration pipeline for teams that want schema-checked data, Excel authoring, deterministic export artifacts, and generated C# runtime loaders.

## What It Does

- Compiles CFT schema files.
- Loads configured Excel sheets into a typed data model.
- Runs schema `check` expressions.
- Exports JSON or MessagePack data files.
- Generates C# runtime loading code for .NET and Unity-style projects.

## Quick Start

Run the RPG example:

```powershell
cargo run -- check examples/rpg
cargo run -- build examples/rpg
```

Generated files are written to the paths declared in `examples/rpg/coflow.yaml`:

```text
examples/rpg/generated/data
examples/rpg/generated/csharp
```

Run individual stages:

```powershell
cargo run -- cft check examples/rpg
cargo run -- export json examples/rpg
cargo run -- codegen csharp examples/rpg
```

To use MessagePack, set `outputs.data.type: messagepack` in `coflow.yaml`, then run:

```powershell
cargo run -- build examples/rpg
```

## Project Configuration

A Coflow project is configured by `coflow.yaml`:

```yaml
schema: schema/

sources:
  - file: data/rpg.xlsx
    sheets:
      - sheet: Item
        columns:
          Item ID: id
          Name: name

outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Example.Rpg.Config
```

`schema` points to one CFT file, a schema directory, or a list of files/directories. `sources` maps workbook sheets and column headers to CFT fields. `outputs.data.type` is `json` or `messagepack`; `outputs.code.type` currently supports `csharp`.

## Commands

```powershell
cargo run -- init my-config
cargo run -- check examples/rpg
cargo run -- build examples/rpg
cargo run -- export json examples/rpg --out generated/data
cargo run -- export messagepack examples/rpg --out generated/data
cargo run -- codegen csharp examples/rpg --out generated/csharp --namespace Game.Config
cargo run -- cft lsp examples/rpg
```

## CFT Syntax Overview

CFT describes game data shape, defaults, references, indexes, and validation rules.

Constants:

```cft
const MAX_LEVEL: int = 100;
```

Enums:

```cft
@display("Item rarity")
enum Rarity {
  Common = 0,
  Rare = 10,
  Epic = 20,
}
```

Types and fields:

```cft
@display("Item")
type Item {
  @id
  id: string;

  name: string;

  @index
  rarity: Rarity = Rarity.Common;

  tags: [string] = [];
  attributes: {string: int} = {};
}
```

Inheritance and polymorphic values:

```cft
abstract type Reward {
  id: string;
}

sealed type ItemReward : Reward {
  @ref(Item)
  item_id: string;
  count: int = 1;
}
```

Checks:

```cft
type Monster {
  @id
  id: string;
  level: int;
  drops: [Reward] = [];

  check {
    level >= 1 && level <= MAX_LEVEL;
    unique(drops);
  }
}
```

Common annotations:

- `@id` marks the primary key field for a table.
- `@index` generates lookup APIs in generated runtime code.
- `@ref(Type)` stores another table record's ID and resolves it in generated loaders.
- `@display("text")` emits user-facing descriptions where supported.
- `@deprecated` marks generated C# symbols as obsolete.
- `@struct` generates a C# struct for sealed value-like types.

Common built-ins in checks include `len`, `contains`, `unique`, `min`, `max`, `sum`, `keys`, `values`, and `matches`.

## Runtime Dependencies

Generated JSON C# loaders use `Newtonsoft.Json`. Generated MessagePack C# loaders use `MessagePack-CSharp` and explicit `MessagePackReader` code paths designed for normal .NET and Unity/IL2CPP-style environments.

## More Docs

Detailed specs live in `docs/spec`.
```

- [ ] **Step 2: Check README commands**

Run:

```powershell
cargo run -- check examples/rpg
cargo run -- build examples/rpg --data-out target/readme-data --code-out target/readme-csharp
```

Expected: PASS.

---

## Task 6: Final Verification

**Files:**

- No planned source files beyond prior tasks.

- [ ] **Step 1: Format**

Run:

```powershell
cargo fmt --all
```

Expected: no errors.

- [ ] **Step 2: Run verification**

Run:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 3: Inspect git state**

Run:

```powershell
git status --short
```

Expected: only intentional files are changed, plus pre-existing local branch state.
