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

CFT describes game data shape, defaults, record-key references, and validation rules.

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
@keyAsEnum("ItemId")
type Item {
  name: string;
  rarity: Rarity = Rarity.Common;
  tags: [string] = [];
  attributes: {string: int} = {};
}
```

Inheritance and polymorphic values:

```cft
abstract type Reward {
  check { id != ""; }
}

sealed type ItemReward : Reward {
  item: Item;
  count: int = 1;
}
```

Checks:

```cft
type Monster {
  level: int;
  drops: [Reward] = [];

  check {
    level >= 1 && level <= MAX_LEVEL;
    unique(drops);
  }
}
```

Common annotations:

- `@keyAsEnum("Name")` generates a C# enum from loaded record keys.
- `@display("text")` emits user-facing descriptions where supported.
- `@deprecated` marks generated C# symbols as obsolete.
- `@struct` generates a C# struct for sealed value-like types.
- `@expand` lets Excel columns expand into a nested object field.

Excel sheets must include an `id` column. That column is the record key and is
not a CFT field. Object references in cells are explicit, for example `@sword_01`
or `@drop_01.rewards[0]`; bare strings remain strings.

Common built-ins in checks include `len`, `contains`, `unique`, `min`, `max`, `sum`, `keys`, `values`, and `matches`.

## Runtime Dependencies

Generated JSON C# loaders use `Newtonsoft.Json`. Generated MessagePack C# loaders use MessagePack-CSharp and explicit `MessagePackReader` code paths designed for normal .NET and Unity/IL2CPP-style environments.

## Internal Crate Boundaries

- `coflow-project` owns project configuration, path resolution, and CFT schema compilation.
- `coflow-pipeline` owns project execution for check, build, export, and codegen commands.
- The CLI crate owns command-line parsing and terminal output.

## More Docs

- [CLI reference](docs/cli.md)
- [Diagnostics and error output](docs/diagnostics.md)
- Detailed specs live in `docs/spec`.
