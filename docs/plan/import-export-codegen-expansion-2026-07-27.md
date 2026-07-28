# Import, Export, and Code Generation Expansion Plan

- Date: 2026-07-27
- Status: implemented
- Branch: `feature/import-export-codegen-expansion`

## 1. Scope

This plan adds:

- TSV and JSON source providers.
- A Protobuf data exporter.
- C++11, Lua 5.1, GDScript, and Rust code generators.
- JSON loaders for C++, Lua, GDScript, and Rust.
- Protobuf loaders for C++, Rust, and C#.

Existing C# JSON and MessagePack support remains compatible. CBOR is out of scope. New languages do
not initially receive MessagePack loaders, and Lua/GDScript do not initially receive Protobuf
loaders.

The supported output matrix after this work is:

| Code | JSON | MessagePack | Protobuf |
| --- | --- | --- | --- |
| C# | existing | existing | new |
| C++11 | new | unsupported | new |
| Lua 5.1 | new | unsupported | unsupported |
| GDScript | new | unsupported | unsupported |
| Rust | new | unsupported | new |

## 2. Product Constraints

- `cpp` always emits conservative, exception-free C++11. No language-standard option is added.
- `lua` always targets Lua 5.1. No Lua-version option is added.
- `gdscript` targets Godot 4.
- `rust` targets Rust edition 2021.
- Loader selection remains automatic from `(code.type, data.type)`.
- JSON remains the common human-readable wire format.
- Lua and GDScript JSON loaders reject integers outside the exactly representable IEEE-754 range.
- Protobuf artifacts from different Coflow builds are not wire-compatibility promises.
- Protobuf data, contract, and generated code must be built and deployed together.
- No Protobuf lockfile, schema fingerprint, manifest, descriptor set, contract output slot, or new
  project configuration layer is introduced.
- Protobuf exports localized dimension tables. C# Protobuf loaders read those tables; C++ and Rust
  continue to reject localized Protobuf schemas until their generated runtimes expose a variant
  table lookup API.

## 3. Architecture

### 3.1 Shared code generation model

Add `coflow-codegen-core` as the language-neutral lowering boundary. It owns projections needed by
all generators: complete fields, inheritance and concrete subtype relationships, enums and flags,
singleton/localized/id-as-enum metadata, table dependencies, collection/value shapes, and symbol
origins. It does not own target-language names, syntax, templates, or runtime choices.

Each language package lowers the shared model into a private language-specific IR. The existing C#
generator migrates only where doing so removes actual duplicated schema traversal; no universal
rendering IR is introduced.

### 3.2 Provider roles

- The existing CSV package registers a separate `tsv` source/writer role backed by a parameterized
  delimited format implementation.
- `coflow-loader-json` owns JSON source parsing and source diagnostics.
- `coflow-exporter-protobuf` owns CFT-to-Protobuf wire lowering, `.proto` rendering, and `.pb`
  encoding. There is no user-visible `coflow-protobuf` component.
- Language packages own their code generator and compatible loader generator roles.

### 3.3 Artifact model

`ArtifactSet` already supports mixed text and byte files. Protobuf returns one set containing table
`.pb` files and `_schema/coflow.proto`. `ExporterDescriptor.content_kind` becomes
`table_content_kind` so the descriptor describes primary table artifacts rather than every support
file in the set.

The existing data/code release slots and atomic publication lifecycle remain unchanged.

## 4. Source Formats

### 4.1 TSV

TSV uses a fixed tab delimiter and otherwise shares CSV table semantics, source options, cell value
grammar, mutations, transaction behavior, dimensions, and diagnostics. It supports UTF-8 BOM,
quoted cells, embedded newlines, and CRLF. It exposes the provider id `tsv` and extension `.tsv`.

### 4.2 JSON

The first JSON source shape is one table per file with a root record array. The file stem resolves
the CFT type; `record_type` is available only when the stem cannot identify the type. Records use
the existing JSON export contract: `id`, optional `$type`, schema field names, string record refs,
  symbolic enum strings, arrays, objects, nullable values, and schema-parsed dictionary keys.

The parser rejects duplicate and unknown properties and reports JSON paths plus source ranges.
Initial write capabilities may be read-only; any writer added in this scope must use transactional
whole-file canonical rewriting and advertise a required full refresh.

## 5. Output Formats

### 5.1 JSON

JSON enum values become symbolic strings: named variants use `Enum.Variant`, while flag composites
or unnamed values use `Enum(<integer>)`. JSON import also accepts the legacy integer representation
for migration. Other JSON shapes remain stable and no output option is added. Missing JSON table
files retain the existing empty-table semantics. C++ uses
`nlohmann/json`, Lua uses `cjson.safe`, GDScript uses Godot's `JSON`, and Rust uses `serde_json`.

### 5.2 Protobuf

The exporter emits:

```text
<data-dir>/
  <Table>.pb
  _schema/coflow.proto
```

Field numbering is deterministic for the current schema: record `id` uses field 1, fields 2 through
15 are reserved for Coflow, and user fields are assigned from 16 in canonical-name order. Table
envelopes, dictionary entries, nullable wrappers, and polymorphic wrappers use fixed structural
numbers. Schema changes may change user field numbers.

CFT integers and enums use `sint64`; floats use `double`; refs use string keys; arrays use repeated
fields; dictionaries use repeated entry messages; nullable collections use wrappers; polymorphic
objects use wrapper messages with `oneof` concrete alternatives.

C++ receives a generated schema-specific C++11 decoder and does not depend on a recent Google
Protobuf runtime. Rust uses `prost`. C# uses a generated schema-specific decoder compatible with the
existing generated domain model and does not change the existing JSON/MessagePack loaders.

## 6. Language Contracts

- C++11 uses ordinary classes, `std::vector`, `std::map`, a generated optional type, classic base
  classes for polymorphism, stable pointers for resolved record refs, and result objects instead of
  exceptions.
- Lua 5.1 uses modules returning tables, metatables for generated types, LuaLS annotations, arithmetic
  flag helpers, and a null sentinel for nullable array entries.
- GDScript uses Godot 4 typed `RefCounted` classes, typed arrays where representable, database lookup
  helpers, and localized value support.
- Rust uses owned domain values, typed record ids resolved through the database, Rust enums for
  polymorphism, and edition-2021 modules.

## 7. Delivery Stages

1. Add this plan and the shared codegen model with behavior-preserving tests.
2. Add TSV and JSON source providers plus source conformance tests.
3. Add C++11, Lua 5.1, GDScript, and Rust generators with JSON loaders.
4. Add Protobuf wire lowering, `.proto` generation, and `.pb` export.
5. Add C++11, Rust, and C# Protobuf loaders.
6. Register all built-ins, update reference documentation, and add matrix/golden/integration tests.
7. Run `cargo check --workspace` and `cargo test --workspace`, then commit, push, and open a PR.

Every stage must keep the workspace compiling. Unsupported pairs must fail through the existing
provider diagnostic rather than silently selecting another loader.

## 8. Acceptance Criteria

- TSV loads the same logical tables as equivalent CSV and supports the intended writer capabilities.
- Exported JSON can be loaded by the JSON source provider for supported schema values.
- Every supported JSON code/data pair is registered and covered by generated artifact tests.
- Protobuf `.pb` artifacts decode according to the generated `.proto` contract.
- C++, Lua, GDScript, Rust, and C# generators cover inheritance, refs, nullable values, arrays, dicts,
  enums/flags, singleton, localization, and id-as-enum or emit explicit diagnostics for a documented
  unsupported schema shape.
- No CBOR provider, Protobuf lockfile, fingerprint, manifest, descriptor, or unnecessary project
  option is introduced.
- The required repository checks pass without starting or stopping the CFD editor.
