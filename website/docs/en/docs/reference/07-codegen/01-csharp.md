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
var settings = data.Singleton<Settings>();
var baseName = sword.name;
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

Records and singletons have `string id`. Dimension fields retain their business types; direct access reads the base value. C# keyword names are escaped with `@` in generated source.

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
Item sword = data.Table<Item>().Get(ItemId.sword);
bool found = data.Table<Item>().TryGet(ItemId.shield, out var shield);
```

`Get` throws `KeyNotFoundException` when absent; `TryGet` returns false. String indexing still accepts CFD IDs. The mapping contains current records and preserves existing numbers when records are added or removed.

## Select dimensions per consumer

```csharp
var japanese = sword.For("language", "ja");
var mobileJapanese = japanese.For("platform", "mobile");

var ordinaryName = sword.name;
var translatedName = japanese.name;
bool translated = sword.HasVariant(nameof(sword.name), "ja");

foreach (var variant in sword.Variants("language")) {
    var text = sword.For("language", variant).name;
    var explicitlyProvided = sword.HasVariant(nameof(sword.name), variant);
}
```

`For` returns an independent record view so interfaces may use different variants simultaneously. Chain calls to combine dimensions; selecting the same dimension again replaces its selection. Unknown dimensions are errors. Missing overrides or unknown variants within known dimensions use the base expression.

References, inline data, collections and templates in a view share its selection context. If only name has a Japanese override, a base template `f"Buy {self.name}"` still reads the Japanese name in that view.

`HasVariant(field, variant)` checks explicit assignment. Explicit `None`, empty strings and overrides equal to the base value count as present. The field must be bound to a dimension. Fallback alone returns false.

`Variants(dimension)` returns all known variants in that dimension, including ones not supplied for the current record. Use `HasVariant` for field-specific coverage.

## Options, strings and diagnostics

`Option<T>` exposes `IsSome`, `IsNone` and `Value`; reading Value on None throws. Nested Options retain every layer's state. CFD may omit a field only when CFT declares a default, for example `note: Option<string> = None;`.

Strings support ordinary literals and explicit `f"..."` interpolation. Ordinary braces remain literal; interpolation reads values in the current view.

Initial loading validates base data and each declared dimension variant. A first-time combination of dimensions that exposes a dependency cycle or similar error makes `For` throw `CfdLoadException`; inspect its `Diagnostics`. CFT business rules execute through CLI validation, builds or the editor.

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
