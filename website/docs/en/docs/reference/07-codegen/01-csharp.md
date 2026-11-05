# C# code generation and data access

C# applications reference `Coflow.Loader` and generated source. Supported platforms include .NET Standard 2.1, .NET 8 and Unity 2022+.

```yaml
schema: schema/
data: data/
codegen:
  - language: csharp
    dir: generated/csharp
    namespace: Game.Config
```

Run `coflow codegen --project .`, then add a matching Loader and all generated files to your C# project. Type, field, and namespace names must be representable as C# identifiers. Unsupported names produce generation diagnostics; rename them in CFT and regenerate.

## Get the Loader

Check `coflow --version`, then download the source for the matching `vX.Y.Z` tag from [Coflow releases](https://github.com/Puring103/coflow/releases), or run:

```sh
git clone --depth 1 --branch vX.Y.Z https://github.com/Puring103/coflow.git dependencies/coflow
dotnet add app/App.csproj reference dependencies/coflow/runtimes/csharp/Coflow.Loader/Coflow.Loader.csproj
```

Replace `X.Y.Z` with your CLI version and the application path with your project. CLI installers do not include Loader source. Include every generated `.cs` file in the application compilation; use `Compile Include` when the generated directory is outside the project.

For Unity 2022+, build a `netstandard2.1` Loader DLL from the same source version, import it with the generated source, and configure assembly references. Keep a single Loader reference when one is already installed.

## Loading and queries

```csharp
using Coflow.Loader;
using Game.Config;

var result = CoflowData.Load(new[] {
    new CfdSource("items.cfd", itemsText),
    new CfdSource("ja.cfd", japaneseText),
});
if (!result.Success) {
    foreach (var diagnostic in result.Diagnostics)
        System.Console.WriteLine(diagnostic);
    return;
}
var data = result.Data;
var items = data.Table<Item>();
var sword = items["sword"];
var settings = data.Singleton<Settings>().Base;
var baseName = sword.Base.name;
var japaneseName = sword.For("language", "ja").name;
```

Supply all mutually referencing CFD contents together. Paths locate diagnostics. Inspect `Diagnostics` on failure; read `Data` only when `Success` is true.

`Table<T>()` returns a read-only record table supporting string indexing, `TryGetValue`, `ContainsKey`, `Count`, `Keys`, `Values` and iteration. `Singleton<T>()` returns a singleton. Parent-type tables include subtype records; base tables and base references share objects. Data types are inline values and have no record table.

## Types and extensions

| CFT | C# |
| --- | --- |
| table / singleton | partial class |
| data | partial class |
| @struct sealed data | partial struct |
| int / long / float / double / bool / string | Corresponding C# types |
| enum | int by default; long with `@int64` |
| `&TableName` | Business object reference |
| Data field | Inline business value |
| `Option<T>` | `Coflow.Loader.Option<T>` |
| `[T]` / `{K: V}` | `IReadOnlyList<T>` / `IReadOnlyDictionary<K, V>` |

Flags generate `[Flags]`, `None = 0` and `Everything` computed from actual flag bits. `@int64` also works with `@flag` and record ID enums.

Generated properties are publicly read-only and directly store completed values. Records and singletons have `string id`. Dimension fields retain their business types; select a base or variant object through its dimension container. C# keyword names are escaped with `@` in generated source.

Generated types preserve abstract, sealed and inheritance constraints. Put business methods and computed properties in separate partial files within the same namespace and assembly:

```csharp
public partial class Item
{
    public bool IsFree => price == 0;
}
```

## Enum record IDs

```cft
enum ItemId {}
@idAsEnum(ItemId)
table Item { name: string; }
```

This generates stable numeric enum values and table query extensions:

```csharp
Dimension<Item> sword = data.Table<Item>().Get(ItemId.sword);
bool found = data.Table<Item>().TryGet(ItemId.shield, out var shield);
```

`Get` throws `KeyNotFoundException` when absent; `TryGet` returns false. String indexing still accepts CFD IDs. The mapping contains current records and preserves existing numbers when records are added or removed.

Keep `coflow.enum.lock.json` in version control. Record IDs in the same enum must have distinct numbers. When merging the lock file, preserve published mappings and resolve number conflicts before generating again.

## Select dimensions per consumer

```csharp
var japanese = sword.For("language", "ja");
var ordinaryName = sword.Base.name;
bool translated = sword.HasVariant(nameof(Item.name), "ja");
var japaneseData = data.For("language", "ja");
Item sameSword = japaneseData.Table<Item>()["sword"];
```

`Table<T>()` contains `Dimension<T>` values; `Singleton<T>()` returns a `Dimension<T>`. Use `.Base` for base data and `.For` for an already built variant. The root's `Base` and `For` return a complete data set with ordinary records and singletons. Both entry points share object identity. Reads and selection never build business data.

Loading builds base data and every declared single-dimension variant. Declare additional combinations when loading:

```csharp
var selection = new DimensionSelection(
    ("language", "ja"), ("platform", "mobile"));
var result = CoflowData.Load(sources, new LoadOptions {
    Combinations = new[] { selection }
});
if (result.Success) {
    Item item = result.Data.Table<Item>()["sword"].For(selection);
}
```

Combination order does not matter. Unknown dimensions, unknown variants and combinations not built during loading throw `ArgumentException`; `TryFor(selection, out value)` returns false. Unknown-variant errors list loaded variants. Missing overrides in a known variant use the base expression, evaluated during loading in the selected context.

`Variants(dimension)` lists loaded variants, including empty override blocks. `HasVariant(field, variant)` checks explicit assignments, including None, empty strings and values equal to the base. Use `nameof(Item.name)` for the field name. A fallback value does not count as an explicit override.

## Options, strings and diagnostics

`Option<T>` exposes `IsSome`, `IsNone` and `Value`; reading Value on None throws. Nested Options retain every layer's state. CFD may omit a field only when CFT declares a default, for example `note: Option<string> = None;`.

Strings support ordinary literals and explicit `f"..."` interpolation. Ordinary braces remain literal; interpolation reads values in the current view.

Loading completes and validates all requested data, including references, collections and template results. Dependency errors in any requested combination are reported in `LoadResult.Diagnostics`; no partial data is returned. CFT business rules execute through CLI validation, builds or the editor.

## Application resources and reloads

Use the same release for generated source and `Coflow.Loader`. Business partials must share the generated types' namespace and assembly. Codegen/build manages generated directories; keep extension files outside them.

Your resource system reads CFD text: desktop applications may use files; Unity may use TextAsset or asset bundles. Pass all mutually referencing base files and needed overrides to Load together. Logical paths locate diagnostics, and input order does not establish priority. Applications use CFD, the loader and generated types; CFT is used at build time for validation and generation.

For Unity, pass resource text directly:

```csharp
var loaded = CoflowData.Load(new[] {
    new CfdSource("data/items.cfd", itemsAsset.text),
    new CfdSource("language/ja.cfd", japaneseAsset.text),
});
```

Here itemsAsset and japaneseAsset are TextAssets obtained by the application. Downloading, asynchronous reading and update timing are application responsibilities.

Reload into a new result and replace the data root only on success:

```csharp
// result is the successfully loaded result above.
var current = result.Data;
var candidate = CoflowData.Load(new[] {
    new CfdSource("items.cfd", updatedItemsText),
    new CfdSource("ja.cfd", updatedJapaneseText),
});
if (candidate.Success) {
    current = candidate.Data;
} else {
    foreach (var diagnostic in candidate.Diagnostics)
        System.Console.WriteLine(diagnostic);
}
```

Keep the previous data on failure. Records and views belong to their own load context; after replacing the root, reacquire records by ID and recreate views from the new root. Multithreaded applications synchronize root replacement themselves.

Loading checks structure, field types, references and template dependencies. Run CFT business rules with CLI `check --rules`, `build` or the editor before delivering data updates. After changing CFT structure, regenerate and compile application source.
