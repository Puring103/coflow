# MessagePack Support Design

## Goal

Add first-class MessagePack runtime data support to Coflow while keeping JSON support intact. The feature includes MessagePack export, generated C# runtime loading for regular .NET and Unity/IL2CPP/AOT, and a crate naming cleanup that makes loader, exporter, and codegen responsibilities explicit.

This version does not add encryption, integrity checks, file headers, or manifests.

## Decisions

- MessagePack data files use the `.msgpack` extension.
- Each table remains one file: `<TypeName>.msgpack`.
- File contents are raw MessagePack, not wrapped by a Coflow envelope.
- No manifest is generated in this version.
- Runtime code generation derives the data format from `outputs.data.type`; `outputs.code` does not define a separate data format.
- MessagePack C# loading uses generated explicit readers instead of typeless, reflection-based, or dynamic resolver deserialization.
- Crate naming is normalized during this work.

## Workspace Layout

The workspace should move toward responsibility-based crate names:

```text
crates/
  coflow-cft/
  coflow-cell-value/
  coflow-data-model/
  coflow-checker/
  coflow-loader-excel/
  coflow-exporter-core/
  coflow-exporter-json/
  coflow-exporter-messagepack/
  coflow-codegen-csharp/
  coflow-project/
  coflow-cft-lsp/
```

The existing crates are renamed as follows:

| Current crate | New crate |
| --- | --- |
| `coflow-excel-loader` | `coflow-loader-excel` |
| `coflow-json-export` | `coflow-exporter-json` |

`coflow-codegen-csharp` already follows the intended naming pattern. Core crates such as `coflow-cft`, `coflow-data-model`, `coflow-checker`, and `coflow-project` keep their current names because they are not loader, exporter, or codegen crates.

## Exporter Architecture

`coflow-exporter-core` owns the shared conversion from validated `CfdDataModel` plus compiled `CftContainer` into a table-oriented export model. This avoids duplicating schema traversal, table selection, field ordering, polymorphic type tagging, `@ref` ID preservation, and dictionary key handling across JSON and MessagePack exporters.

The core export model should be format-neutral:

```rust
pub enum ExportValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<ExportValue>),
    Map(Vec<(String, ExportValue)>),
}

pub type ExportTables = BTreeMap<String, ExportValue>;
```

`Map` deliberately stores ordered key-value pairs instead of a hash map. This preserves deterministic field order and allows encoders to write compact binary maps without sorting or reparsing.

`coflow-exporter-json` converts `ExportValue` into `serde_json::Value` and writes pretty JSON, keeping the existing JSON file shape.

`coflow-exporter-messagepack` encodes `ExportValue` directly to raw MessagePack bytes. It should not go through JSON text. It may use `rmp`/`rmp-serde`, but the public API should expose table bytes or a writer-oriented API rather than forcing the CLI to know encoding details.

## MessagePack Format

The MessagePack table format mirrors the JSON semantic model:

| CFT concept | MessagePack representation |
| --- | --- |
| table file | array |
| record | map |
| field key | string using the CFT source field name |
| `int` | integer |
| `float` | floating point |
| `bool` | boolean |
| `string` | string |
| nullable null | nil |
| enum | integer underlying value |
| object | map |
| polymorphic object | map containing `$type` string |
| array | array |
| dict | map with string keys |
| `@ref` | original ID value |

The format is compact enough for the first version because it avoids JSON text overhead, but it does not switch records to positional arrays. Positional records would be smaller, but they would make compatibility, inspection, and generated loader error paths harder. The first version keeps field-name maps so MessagePack and JSON remain semantically aligned.

Example logical structure:

```text
Item.msgpack:
[
  { "id": "sword_01", "name": "Iron Sword", "rarity": 10 }
]
```

There is no file magic, version field, schema hash, compression marker, encryption marker, or checksum in this version.

## Project Config And CLI

The project config continues to use `outputs.data.type`:

```yaml
outputs:
  data:
    type: messagepack
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Example.Rpg.Config
```

Supported data output types are:

- `json`
- `messagepack`

CLI commands:

```bash
coflow export json examples/rpg
coflow export messagepack examples/rpg
coflow codegen csharp examples/rpg
```

`coflow export messagepack` requires `outputs.data.type: messagepack`, just as `coflow export json` requires `outputs.data.type: json`.

`coflow codegen csharp` reads `outputs.data.type` and generates a matching runtime loader:

- `json` generates a Newtonsoft.Json loader for `.json` files.
- `messagepack` generates a MessagePack loader for `.msgpack` files.

If `outputs.data` is missing or uses an unsupported type, `coflow codegen csharp` returns a clear error. There is no `outputs.code.data_format` override.

`coflow init` may keep `json` as the default output type for now to preserve the lowest-friction starter project.

## C# Runtime Loader

The generated C# runtime remains a trusted artifact loader. It loads data produced by Coflow exporters after the Rust pipeline has already parsed, checked, and built the data model. It should provide useful errors for likely runtime issues such as missing files, malformed MessagePack, duplicate IDs, or failed `@ref` resolution, but it is not a full validator for arbitrary hand-authored binary data.

For MessagePack, generated C# code must be compatible with regular .NET and Unity/IL2CPP/AOT. To avoid reflection and generated-at-runtime resolver requirements, the loader should use explicit generated read methods over a low-level MessagePack reader API.

The generated MessagePack loader should follow the same high-level pipeline as JSON:

1. Load each `<TypeName>.msgpack` table file.
2. Read the root as an array.
3. Construct strongly typed records with generated `Load<Type>` functions.
4. Build `@id` unique indexes.
5. Build `@index` multi-indexes.
6. Build polymorphic `@ref` indexes when needed.
7. Resolve `@ref` fields in a second pass.
8. Return the generated database object.

MessagePack reader helpers should be generated or templated similarly to the JSON helper methods, but they should operate directly on `MessagePackReader`:

```csharp
private delegate T MessagePackRowLoader<T>(
    ref MessagePackReader reader,
    string path);

private static List<T> LoadTable<T>(
    string file,
    string tableName,
    MessagePackRowLoader<T> loadRow)
```

Generated object loaders use signatures such as:

```csharp
private static Item LoadItem(ref MessagePackReader reader, string path)
```

Each generated object loader reads a MessagePack map header, loops over map entries, reads the string field key, and uses a `switch` to route known fields to generated field readers. Unknown fields are skipped with `reader.Skip()` so future exporter additions can be tolerated by older generated code when the required fields are still present. Required fields are tracked with generated `has<Field>` booleans, and duplicate known field keys throw `CftLoadException`.

This keeps the MessagePack loader compact and AOT-safe without materializing dynamic dictionaries or relying on generated-at-runtime resolvers.

Polymorphic fields continue to use a `$type` entry in the object map. MessagePack loading dispatches on that string exactly like JSON loading dispatches on the `$type` JSON property.

Dictionary keys remain string keys in exported MessagePack maps. Generated C# loader methods convert string dictionary keys back to `string`, `long`, or enum keys using the field schema.

## C# Dependencies

Generated JSON loaders continue to depend on `Newtonsoft.Json`.

Generated MessagePack loaders depend on MessagePack-CSharp. The generator should not vendor or emit dependency packages. Project integration remains the user's responsibility:

- regular .NET: add the NuGet package;
- Unity: install a Unity-compatible MessagePack-CSharp package or NuGet package through the user's existing package workflow.

The generated code should not use typeless APIs or resolver features that require runtime code generation.

## Documentation

Add a MessagePack spec document, for example:

```text
docs/spec/08-messagepack-export.md
```

Update existing docs that mention only JSON where the project pipeline now supports JSON or MessagePack.

The C# codegen spec should describe that loader generation follows `outputs.data.type`.

## Testing Strategy

Tests should be added before implementation changes.

Exporter core tests:

- exports table files for all non-abstract `@id` types, including empty tables;
- preserves field order, default-expanded values, `@ref` IDs, `$type`, nullable values, arrays, and dictionaries;
- errors when model data cannot be matched to the compiled schema.

JSON exporter tests:

- existing JSON expectations still pass after moving shared logic into `coflow-exporter-core`;
- JSON output remains semantically unchanged.

MessagePack exporter tests:

- export a schema/model containing scalars, enum, nullable, nested object, polymorphic object, array, dict, and `@ref`;
- decode the produced MessagePack in Rust and assert it matches the expected export structure;
- verify files use `.msgpack` in CLI export.

CLI tests:

- `coflow export messagepack examples/rpg --out <dir>` writes expected `.msgpack` table files;
- `coflow export messagepack` rejects `outputs.data.type: json`;
- `coflow codegen csharp` emits `.msgpack` file paths and MessagePack reader code when `outputs.data.type: messagepack`;
- `coflow codegen csharp` still emits JSON loader code when `outputs.data.type: json`.

C# codegen tests:

- JSON loader generation remains covered by existing tests;
- MessagePack loader generation contains MessagePack-CSharp imports, `.msgpack` paths, generated field readers, `$type` dispatch, and `@ref` resolution;
- generated MessagePack loader output does not contain Newtonsoft.Json imports.

Full C# compile tests are desirable but not required for the first implementation if local CI does not already have a .NET/Unity build harness.

## Deferred Work

The following are intentionally out of scope for the first MessagePack implementation:

- encryption;
- integrity checks;
- digital signatures;
- file headers or format envelope;
- manifest files;
- schema hash validation;
- compression;
- positional record arrays;
- automatic Unity package installation;
- runtime loading for languages other than C#.
