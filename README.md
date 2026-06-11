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

Generated JSON C# loaders use `Newtonsoft.Json`. Generated MessagePack C# loaders use MessagePack-CSharp and explicit `MessagePackReader` code paths designed for normal .NET and Unity/IL2CPP-style environments.

## More Docs

Detailed specs live in `docs/spec`.
