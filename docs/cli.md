# Coflow CLI

This document describes the user-facing `coflow` command line interface. Run
commands from the repository with `cargo run -- <command>`, or replace
`cargo run --` with the installed `coflow` executable.

## Common Project Argument

Most commands accept an optional `CONFIG_OR_DIR` argument:

- If omitted, Coflow looks for `coflow.yaml` or `coflow.yml` in the current
  directory.
- If it is a directory, Coflow reads `coflow.yaml` or `coflow.yml` inside it.
- If it is a file, Coflow reads that file as the project config.

Project-relative paths in `coflow.yaml` are resolved from the directory that
contains the config file.

## Exit Status

- `0`: the command completed successfully.
- non-zero: the command failed, or produced diagnostics.

A command that prints structured diagnostics exits non-zero even when the
diagnostics were recoverable enough to continue collecting more errors. Coflow
does not write build/export/codegen artifacts when diagnostics are present.

## Project Config

A minimal project config:

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
    namespace: Game.Config
```

`schema` may be one file, one directory, or a list of files/directories. Schema
directories are searched recursively for `.cft` files.

`sources` configures Excel input. Each source has a workbook `file` and one or
more `sheets`. A sheet can specify:

- `sheet`: Excel worksheet name.
- `type`: optional CFT type name. If omitted, the sheet name is used.
- `columns`: optional mapping from Excel header text to CFT field names.

`outputs.data.type` is `json` or `messagepack`.

`outputs.code.type` currently supports `csharp`. `outputs.code.namespace` is
used by C# codegen and can be overridden by command line options.

## Commands

### `coflow init [DIR]`

Creates a minimal Coflow project in `DIR`, or in the current directory when
`DIR` is omitted.

```powershell
cargo run -- init my-config
```

The command refuses to overwrite an existing `coflow.yaml`.

### `coflow cft check [CONFIG_OR_DIR] [--json] [--stdin-path PATH]`

Compiles the configured CFT schema files and prints schema diagnostics.

```powershell
cargo run -- cft check examples/rpg
cargo run -- cft check examples/rpg --json
```

This command does not require Excel files to exist. It validates schema paths,
output config shape, and source config shape, then compiles schema files.

`--stdin-path PATH` treats standard input as the contents of a schema file with
the given path. This is primarily useful for editor integrations.

`--json` emits:

```json
{"diagnostics":[]}
```

or the same object with one or more diagnostic entries.

### `coflow cft lsp [CONFIG_OR_DIR]`

Starts the CFT language server for editor integrations.

```powershell
cargo run -- cft lsp examples/rpg
```

The language server uses schema-only project loading. It does not require Excel
files to exist.

### `coflow check [CONFIG_OR_DIR] [--json]`

Runs the validation pipeline without writing artifacts:

1. Project config preflight.
2. Schema discovery and compilation.
3. Excel loading.
4. Data model build.
5. Reference resolution.
6. CFT `check {}` execution.

```powershell
cargo run -- check examples/rpg
cargo run -- check examples/rpg --json
```

Use this command before committing data changes. It reports all diagnostics it
can collect without depending on invalid intermediate state.

### `coflow build [CONFIG_OR_DIR] [--data-out DIR] [--code-out DIR] [--namespace NAME]`

Runs validation, exports data, and optionally generates configured code.

```powershell
cargo run -- build examples/rpg
cargo run -- build examples/rpg --data-out out/data --code-out out/csharp --namespace Game.Config
```

`build` writes data artifacts only after project, schema, Excel, data model, and
check diagnostics are clean. If `outputs.code` is configured, it also runs C#
codegen after codegen preflight succeeds.

Overrides:

- `--data-out DIR`: overrides `outputs.data.dir`.
- `--code-out DIR`: overrides `outputs.code.dir`.
- `--namespace NAME`: overrides `outputs.code.namespace`.

If any diagnostic is produced, `build` exits non-zero and does not write data or
code artifacts for the failing run.

### `coflow export json [CONFIG_OR_DIR] [--out DIR]`

Exports JSON data. The project config must declare:

```yaml
outputs:
  data:
    type: json
```

```powershell
cargo run -- export json examples/rpg
cargo run -- export json examples/rpg --out generated/data
```

Output files are named `<TypeName>.json`.

### `coflow export messagepack [CONFIG_OR_DIR] [--out DIR]`

Exports MessagePack data. The project config must declare:

```yaml
outputs:
  data:
    type: messagepack
```

```powershell
cargo run -- export messagepack examples/rpg
cargo run -- export messagepack examples/rpg --out generated/data
```

Output files are named `<TypeName>.msgpack`. They contain bare MessagePack
arrays with the same schema shape as JSON export.

### `coflow codegen csharp [CONFIG_OR_DIR] [--out DIR] [--namespace NAME]`

Generates C# runtime loading code without loading Excel data.

```powershell
cargo run -- codegen csharp examples/rpg
cargo run -- codegen csharp examples/rpg --out generated/csharp --namespace Game.Config
```

The project config must declare:

```yaml
outputs:
  data:
    type: json # or messagepack
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
```

`outputs.data.type` decides which loader is generated:

| Data output type | Generated loader |
| --- | --- |
| `json` | Newtonsoft.Json loader |
| `messagepack` | MessagePack-CSharp loader |

`codegen csharp` does not require Excel sources to exist. For `@keyAsEnum`, it
generates declared enum files but cannot add data-driven variants because it
does not load data. `coflow build` can add those variants because it has loaded
the data model.

Codegen runs a preflight before touching the lockfile, cleaning old `.cs` files,
or writing new files. Naming errors therefore produce diagnostics without
modifying existing generated output.

## Command Matrix

| Command | Requires schema | Requires Excel files | Builds data model | Runs checks | Writes artifacts |
| --- | --- | --- | --- | --- | --- |
| `init` | no | no | no | no | creates project files |
| `cft check` | yes | no | no | no | no |
| `cft lsp` | yes | no | no | no | no |
| `check` | yes | yes | yes | yes | no |
| `build` | yes | yes | yes | yes | data and optional code |
| `export json` | yes | yes | yes | yes | JSON data |
| `export messagepack` | yes | yes | yes | yes | MessagePack data |
| `codegen csharp` | yes | no | no | no | C# code |

See [diagnostics.md](diagnostics.md) for output formats, error codes, and
non-blocking diagnostic collection rules.
