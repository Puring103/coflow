# Getting started

This tutorial defines an iron sword's name and price, generates C# types, and loads CFD into an existing C# application to read the item.

## 1. Install

Choose a Windows x64 asset from [GitHub Releases](https://github.com/Puring103/coflow/releases/latest):

- `coflow-tools-windows-x64-setup.exe`: desktop editor and CLI for visual editing and AI collaboration.
- `coflow-cli-windows-x64-setup.exe`: CLI only for terminal and automation workflows.
- VSIX: use “Install from VSIX” in VS Code for CFT/CFD language support.

Both Windows installers install the user skills. Reopen your terminal after installation and verify the CLI with the commands below. The full installer includes the CLI; no separate CLI-only installation is needed. See [Editor guide](./editors.md) for desktop and VS Code usage. Alternatively, install the Rust toolchain and build from source:

```sh
cargo install --git https://github.com/Puring103/coflow.git coflow
coflow --version
coflow --help
```

## 2. Create and configure a project

Choose a location and run:

```sh
coflow init item-config
```

Set `item-config/coflow.yaml` to:

```yaml
schema: schema/
data: data/
codegen:
  - language: csharp
    dir: generated/csharp
    namespace: Game.Config
```

`schema` selects CFT type files, `data` selects CFD data, and `codegen` sets the output directory and namespace. Paths are relative to `item-config`; input directories are read recursively.

## 3. Define an item in CFT

Create `item-config/schema/items.cft`:

```cft
table Item {
  name: string;
  price: int = 0;
  check ValidPrice {
    price >= 0: "Price cannot be negative";
  }
}
```

`table Item` defines records with IDs. `name` is required, `price` defaults to `0`, and `ValidPrice` requires a nonnegative price.

## 4. Write CFD data

Create `item-config/data/items.cfd`:

```cfd
sword: Item {
  name: "Iron sword",
  price: 100,
}
```

`sword` is the record ID and `Item` is the CFT type. Separate fields with commas.

## 5. Generate C# code

```sh
coflow build --project item-config
```

`build` checks CFT, CFD and business rules, then generates C# source in `item-config/generated/csharp` when all checks pass. A negative price produces a validation diagnostic; correct it before rebuilding.

To validate or generate separately:

```sh
coflow check --project item-config --rules
coflow codegen --project item-config
```

`codegen` checks types and data. Use `build` to execute all business rules before delivery.

## 6. Load in C#

Reference `Coflow.Loader` and the generated source under `item-config/generated/csharp` in your application. Use the same release version for the loader and CLI. Your resource system reads CFD text and supplies it to the loader:

```csharp
using Coflow.Loader;
using Game.Config;

// itemsText contains the data/items.cfd text above.
var result = CoflowData.Load(new[] {
    new CfdSource("data/items.cfd", itemsText),
});

// Confirm loading succeeded before reading business data.
if (!result.Success) {
    foreach (var diagnostic in result.Diagnostics)
        System.Console.Error.WriteLine(diagnostic);
    return;
}

var sword = result.Data.Table<Item>()["sword"];
var name = sword.name;   // Iron sword
var price = sword.price; // 100
```

Reload after changing CFD to read updated values. Regenerate and compile source after changing CFT structure. See [C# data access](../reference/07-codegen/01-csharp.md) for type mappings, references and dimensions.
