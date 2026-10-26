# Project configuration

`coflow.yaml` selects CFT type files, CFD data, dimension override directories and code generation targets. Create a project with `coflow init <directory>`. Commands accept `--project <directory or coflow.yaml>`; when omitted, discovery searches the current directory and its parents.

## Complete configuration

```yaml
schema:
  - schema/
  - shared/common.cft
data:
  - data/
  - overlays/base.cfd
dimensions:
  language:
    display_name: Localization
    dir: dimensions/language/
  platform:
    display_name: Platform
    dir: dimensions/platform/
codegen:
  - language: csharp
    dir: generated/csharp
    namespace: Game.Config
```

Include only inputs you use. `schema` is the only required top-level field. Omitting `data`, `dimensions` or `codegen` means no base data inputs, dimension directory configuration or generation targets, respectively.

## Fields

| Field | Type | Requirement / default | Configuration |
| --- | --- | --- | --- |
| `schema` | Path string or list | Required | `.cft` file or directory; recursively reads `.cft` |
| `data` | Path string or list | Optional, empty list | `.cfd` file or directory; recursively reads `.cfd` |
| `dimensions` | Map from dimension name to settings | Optional, empty map | Display names and override directories for CFT dimensions |
| `dimensions.<name>.display_name` | String | Optional | Editor display name; defaults to a built-in name or the dimension name |
| `dimensions.<name>.dir` | Directory path | Optional | Recursively reads override CFD; destination for new editor override files |
| `codegen` | Target object or list | Optional, empty list | Use a list for multiple targets |
| `codegen[].language` | String | Required per target | `csharp` for C# |
| `codegen[].dir` | Directory path | Required per target | Dedicated generated source directory |
| `codegen[].namespace` | String | Optional for C#, global namespace | For example `Game.Config`; an empty string also selects the global namespace |

Field names are case-sensitive. Unknown fields, duplicates and incorrect value types produce configuration diagnostics.

## Type inputs: schema

Use a string for one file or directory, or a list for multiple inputs:

```yaml
schema: schema/
```

```yaml
schema:
  - schema/items.cft
  - ../shared-schema/
```

Directories recursively include `.cft` files. Declarations from all inputs share a global type collection and can refer to each other directly. Duplicate declarations produce diagnostics. Use relative `../` paths for shared types outside the project.

## Data inputs: data

```yaml
data:
  - data/
  - events/current.cfd
```

Directories recursively include only `.cfd` files. Explicit file paths must select CFD files; each list item is a path string. Records can reference other files. File order does not establish override priority. Each source is loaded once.

Split records by business module; supply mutually referencing files together when loading in an application. New CFD files in an input directory are included automatically. Inspect actual inputs with `coflow project files --kind data`.

## Dimensions

Names correspond to CFT `@dimension("name")`; `@localized` uses `language`. Bind fields in CFT and supply variants in CFD blocks such as `language[ja] { ... }`. YAML controls organization and display; variants are discovered from CFD override blocks.

```yaml
dimensions:
  language:
    display_name: Localization
    dir: dimensions/language/
```

CFD files in dimension directories are included automatically and need not also appear in `data`. A missing directory is treated as empty and created when the editor first saves an override. With only `display_name`, overrides in existing data inputs can still be read; configure `dir` before creating recommended override files in the editor.

The editor preserves the base file's complete path relative to the project root: language overrides for `data/equipment/items.cfd` are saved to `dimensions/language/data/equipment/items.cfd`. Records, fields and variants for the same base file and dimension are stored together. Existing overrides are written back to their original source.

Dimension directories must not overlap each other and must be separate from schema inputs. See [localization and dimensions](./10-localization.md) for override and fallback behavior.

## Code generation

```yaml
codegen:
  - language: csharp
    dir: ../Game/Generated/Config
    namespace: Game.Config
```

`language` selects the target. C# `namespace` consists of valid C# identifiers separated by dots. Omitting it or using an empty string places types in the global namespace.

A single target can also be an object:

```yaml
codegen:
  language: csharp
  dir: generated/csharp
```

Code generation manages the output directory; keep business extensions outside it. Target directories must not overlap each other or schema, data and dimension inputs. An external output such as `../Game/Generated/Config` can place generated source directly in an application project.

Run `coflow codegen` to generate or update source, `coflow codegen --dry-run` to preview, or `coflow codegen --check` to verify outputs match the current project. `coflow build` generates after all business rules pass. See [C#](./07-codegen/01-csharp.md) for loading.

## Paths and validation

All relative paths resolve against the directory containing `coflow.yaml`, regardless of the calling directory. Inputs and outputs may be outside the project; use relative paths, including `../` for external locations. Existing absolute paths are converted to relative paths when read and written back on the next save. Cross-drive paths that cannot be made relative produce an error.

```sh
coflow project inspect --project . --json
coflow project files --project . --json
coflow check --project . --rules
```

These commands inspect configuration, verify actual inputs, and validate declarations, data and rules. Configuration errors include source positions or configuration paths. See [diagnostics](./09-diagnostics/01-diagnostics.md).
