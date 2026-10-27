# Coflow

English | [简体中文](README.zh-CN.md)

**An AI-native data configuration workflow. People and AI configure together.**

Coflow connects data definitions, editing, validation and application integration. Define types and business rules in CFT, store configuration in version-friendly CFD text, and generate type-safe code for your application.

[Website](https://puring103.github.io/coflow/) · [Documentation](https://puring103.github.io/coflow/en/docs/) · [Download](https://github.com/Puring103/coflow/releases/latest)

## Features

- **AI accelerates iteration.** A CLI designed for AI, paired with dedicated skills, provides context queries, edit previews, revision-protected batch changes and actionable diagnostics.
- **People stay involved.** Switch between graph, table, record and source views to edit the same data, review differences and resolve diagnostics.
- **A shared data language.** CFT describes types, fields, references, inheritance, enums and defaults so designers, AI and programmers work from the same model.
- **Rules that travel with your data.** Declare type-level and cross-record checks, validate inline objects and dimension variants, and pinpoint failures to records and fields.
- **Version-friendly configuration.** CFD supports nested objects, arrays, dictionaries, cross-file references and explicit `f"..."` string interpolation.
- **Localization and dimensions.** Give fields language, platform or other variants, check coverage and read the right configuration for each use.
- **Type-safe C# integration.** Generate business types and load application-provided CFD with `Coflow.Loader`, including references and independent dimension views.
- **Extensible editing.** Add specialized views and field presentations through plugins. CFT/CFD language services provide diagnostics, completion, navigation and semantic highlighting.
- **Validate before delivery.** Use `check`, `build` and `codegen`; failed validation preserves existing generated code.

## Installation

From [GitHub Releases](https://github.com/Puring103/coflow/releases/latest), choose `coflow-tools-windows-x64-setup.exe` for the desktop editor and CLI, or `coflow-cli-windows-x64-setup.exe` for CLI only. Both installers install user skills. Reopen the terminal after installation. Install the release VSIX using VS Code's “Install from VSIX” command for CFT/CFD language support. See the [editor guide](https://puring103.github.io/coflow/en/docs/guide/editors).

Alternatively, install from source with Rust:

```powershell
cargo install --git https://github.com/Puring103/coflow.git coflow
coflow --help
```

## Quick start

```powershell
coflow check --rules --project examples/showcase
coflow codegen --project examples/showcase
```

A minimal `coflow.yaml`:

```yaml
schema: schema/
data:
  - data/
codegen:
  - language: csharp
    dir: generated/csharp
```

`data` accepts CFD files or directories containing `.cfd` files. The C# target also accepts `namespace`. Follow the [getting started guide](https://puring103.github.io/coflow/en/docs/guide/install) to create a project from scratch.

## AI and automation

```powershell
coflow describe --json
coflow context --type Item --project <project> --json
coflow apply --input changes.json --project <project> --rules --if-revision <revision> --json
```

Use the `revision` returned by a query to protect subsequent edits. `apply --rules` validates structure, data and business rules before publishing changes; add `--dry-run` to review them first. Default JSON keeps counts and failures concise. Use `describe apply --schema input` for the full request schema and `--details` for execution details. Install companion skills with `coflow skill install -g`. See the [CLI reference](https://puring103.github.io/coflow/en/docs/reference/08-cli).

## Loading data in C#

Obtain Loader source from the tag matching `coflow --version`, then reference `runtimes/csharp/Coflow.Loader` and all generated C# files. Follow the [C# integration guide](https://puring103.github.io/coflow/en/docs/reference/07-codegen/01-csharp). The loader supports .NET Standard 2.1 and .NET 8. Your application selects CFD files and supplies their contents:

```csharp
using Coflow.Loader;

var result = CoflowData.Load(new[] {
    new CfdSource("items.cfd", itemsText),
    new CfdSource("ja.cfd", japaneseText),
});
if (result.Success) {
    var sword = result.Data.Table<Item>()["sword"];
    var japaneseName = sword.For("language", "ja").name;
}
```

The result includes success status and diagnostics. Numeric types map to their namesake C# types; optional values use nestable `Option<T>`.

Tables and singletons generate partial classes. Data types generate partial classes by default; `@struct sealed data` generates partial structs. Access records with `Table<T>()` and `Singleton<T>()`; ID enums add `Get(enum)` and `TryGet(enum, out record)` extensions. Record references link generated objects directly.

Direct field access returns base values. `record.For("language", "ja")` returns an independent variant view. Without an override, the base expression is evaluated in the selected dimension context. `record.HasVariant("name", "ja")` checks whether a field has an explicit override.

Application loading uses CFD and generated code. CFT defines types and code generation; the editor and CLI `check --rules` / `build` execute business rules. See [C# data access](https://puring103.github.io/coflow/en/docs/reference/07-codegen/01-csharp).
