# Luban extension matrix research (2026-07-14)

## Scope and source method

This note examines only the official [focus-creative-games/luban](https://github.com/focus-creative-games/luban) repository, pinned at [`0d2d589`](https://github.com/focus-creative-games/luban/tree/0d2d589532957595c2911615afd2a7a86ece90c0). It records architecture facts relevant to Coflow's future source, data-output, and code-generation matrix. It is not a proposal to reproduce Luban's implementation wholesale.

## Evidence-backed Luban observations

1. **Luban has three independently named extension families.** `DataLoaderManager`, `DataTargetManager`, and `CodeTargetManager` each create an implementation by name, respectively for input data loaders, output data targets/exporters, and code targets. The managers delegate construction to a shared custom-behaviour registry. Sources: [DataLoaderManager](https://github.com/focus-creative-games/luban/blob/0d2d589532957595c2911615afd2a7a86ece90c0/src/Luban.Core/DataLoader/DataLoaderManager.cs), [DataTargetManager](https://github.com/focus-creative-games/luban/blob/0d2d589532957595c2911615afd2a7a86ece90c0/src/Luban.Core/DataTarget/DataTargetManager.cs), [CodeTargetManager](https://github.com/focus-creative-games/luban/blob/0d2d589532957595c2911615afd2a7a86ece90c0/src/Luban.Core/CodeTarget/CodeTargetManager.cs), [CustomBehaviourManager](https://github.com/focus-creative-games/luban/blob/0d2d589532957595c2911615afd2a7a86ece90c0/src/Luban.Core/CustomBehaviour/CustomBehaviourManager.cs).

2. **The data target owns representation; a separate exporter owns iteration and aggregation.** `IDataTarget` declares `Table`, `Tables`, and `Record` aggregation modes plus `ExportTable`, `ExportTables`, and `ExportRecord`; `IDataExporter` traverses the generation context and calls the selected target according to that mode. Sources: [IDataTarget](https://github.com/focus-creative-games/luban/blob/0d2d589532957595c2911615afd2a7a86ece90c0/src/Luban.Core/DataTarget/IDataTarget.cs), [IDataExporter](https://github.com/focus-creative-games/luban/blob/0d2d589532957595c2911615afd2a7a86ece90c0/src/Luban.Core/DataTarget/IDataExporter.cs), [DataExporterBase](https://github.com/focus-creative-games/luban/blob/0d2d589532957595c2911615afd2a7a86ece90c0/src/Luban.Core/DataTarget/DataExporterBase.cs).

3. **Code targets consume a schema/data generation context rather than source formats.** `GenerationContext` holds the compiled definition assembly, selected export tables/types, and loaded records; `ICodeTarget` validates and generates tables, beans, and enums from that context. Sources: [GenerationContext](https://github.com/focus-creative-games/luban/blob/0d2d589532957595c2911615afd2a7a86ece90c0/src/Luban.Core/GenerationContext.cs), [ICodeTarget](https://github.com/focus-creative-games/luban/blob/0d2d589532957595c2911615afd2a7a86ece90c0/src/Luban.Core/CodeTarget/ICodeTarget.cs), [CodeTargetBase](https://github.com/focus-creative-games/luban/blob/0d2d589532957595c2911615afd2a7a86ece90c0/src/Luban.Core/CodeTarget/CodeTargetBase.cs).

4. **The official distribution demonstrates a practical matrix, but does not encode a strict compatibility matrix in the target interfaces.** The repository contains separate language target projects (for example C#, TypeScript, C++, Dart) and separate data target implementations (for example JSON, BSON, XML, YAML). C# itself has target variants for binary and multiple JSON runtime conventions. Sources: [C# targets](https://github.com/focus-creative-games/luban/tree/0d2d589532957595c2911615afd2a7a86ece90c0/src/Luban.CSharp/CodeTarget), [TypeScript targets](https://github.com/focus-creative-games/luban/tree/0d2d589532957595c2911615afd2a7a86ece90c0/src/Luban.Typescript/CodeTarget), [built-in data targets](https://github.com/focus-creative-games/luban/tree/0d2d589532957595c2911615afd2a7a86ece90c0/src/Luban.DataTarget.Builtin).

5. **Templates are a reusable layer in some code targets, not the complete extension model.** `TemplateCodeTargetBase` adapts a code target to template rendering, while target-specific template directories and extension helpers remain part of language projects. Sources: [TemplateCodeTargetBase](https://github.com/focus-creative-games/luban/blob/0d2d589532957595c2911615afd2a7a86ece90c0/src/Luban.Core/CodeTarget/TemplateCodeTargetBase.cs), [TypeScript templates/extensions](https://github.com/focus-creative-games/luban/tree/0d2d589532957595c2911615afd2a7a86ece90c0/src/Luban.Typescript).

## What Coflow should adopt

The useful abstraction is a pipeline with explicit, independently extensible stages:

```text
Source adapter -> normalized input records -> validated data model
                                           |                 |
                                           |                 +-> data encoder -> artifact layout
                                           +-> language model -> language renderer -> artifact layout
```

`coflow-runtime` should continue to own only the left half through `CfdDataModel`. Source providers should produce normalized input records and source locations, never decide export representation or generated-language conventions. This preserves the current provider-neutral runtime direction.

For the right half, replace the implicit `exporter` and `codegen` pairing with a declared **output profile**:

```text
output profile = data contract + data encoder + optional language binding + artifact layout
```

The data contract must be a first-class descriptor, for example `json/v1`, `messagepack/v1`, or a future `protobuf/v1`. A data encoder publishes the contract it produces. A language binding publishes the contracts it can read and the runtime/API conventions it targets. Project configuration then selects compatible named components, rather than each language generator containing an expanding `if data_format == ...` set.

This makes the matrix additive:

| New capability | Implement | Existing components changed |
|---|---|---|
| Source kind | `SourceProvider` plus optional write/table capabilities | No exporter or code generator |
| Data representation | `DataEncoder` plus one or more artifact layouts | No source provider; bindings opt in by contract |
| Language/runtime | `LanguageBinding` and renderer/templates | No source provider or encoder; declare supported contracts |
| A language plus format convention | A binding variant/profile | No Cartesian-product crate or central switch |

## Recommended internal module shape

### 1. Freeze a narrow provider-facing model boundary

`coflow-api` currently exposes core CFT and data-model structures directly to providers. Before third-party or numerous in-tree targets make that surface expensive to change, introduce read-only views with stability tiers:

- `SchemaView`: types, fields, annotations, defaults, references, table metadata.
- `DataView`: validated tables/records/values and deterministic iteration.
- `SourceInput`: raw source configuration and diagnostics/source-location sinks for source adapters only.
- `ArtifactSink` or `ArtifactSet`: a shared output ownership and atomic publication contract.

Do not create a lossy second schema/data model. These views should be facades over the existing compiled CFT and `CfdDataModel`, following Coflow's existing `CftSchemaView` direction. The aim is to prevent providers from depending on compiler internals, not to duplicate semantics.

### 2. Split output responsibilities into four explicit contracts

Current `DataExporter` and `CodeGenerator` are directionally correct but encode the matrix only indirectly. Introduce these separable concepts before adding several targets:

- `DataContractDescriptor`: stable identifier, version, semantic features (type tags, references, sparse/default policy), and canonical test fixtures.
- `DataEncoder`: transforms `DataView` into contract-conforming logical payloads. It declares `DataContractDescriptor` and a logical aggregation mode (`per-table`, `bundle`, `per-record`), adapting Luban's useful aggregation concept without importing its global context.
- `LanguageBinding`: transforms `SchemaView` into language-facing declarations and loading APIs; it declares accepted data contracts, language/runtime version, naming/nullability/collection conventions, and optional runtime support artifacts.
- `ArtifactLayout`: maps logical artifacts to paths, packaging groups, encoding, and collision policy. This prevents every encoder/binding from independently solving path construction and publication.

The orchestrator should resolve a selected profile once, validate compatibility before generating anything, then run independent data and code branches against immutable views. It should not make a generator inspect raw project options for another component's settings.

### 3. Make options typed at the extension boundary

Keep `serde_json::Value` only while parsing generic project config. Each registry descriptor should expose an option schema/defaults and decode into a provider-private typed options struct during preflight. The normalized `ResolvedOutputProfile` should contain no untyped JSON. This gives editor completion, validation before expensive source loading, compatibility diagnostics, and a stable place for deprecated-option migrations.

### 4. Use descriptor registration, not reflection or global state

Luban's name-based registration is a useful discovery pattern, but its shared reflection registry and global `GenerationContext.Current` are not suitable for Coflow's CLI/editor/LSP concurrency model. Retain Coflow's explicit `ProviderRegistry` and extend it with immutable descriptors, explicit registration calls, duplicate-ID rejection, capability metadata, and profiles such as `local-only` or `network-enabled`. Pass a per-run `GenerationContext` value explicitly.

### 5. Use an IR only where it buys reuse

Do not force all code targets through one large language-neutral code IR. Introduce a small `BindingModel` only for semantics every binding needs: exported type graph, wire names, type/tag/reference rules, and loader table metadata. Each language can then lower that into its own rich IR or templates. This avoids both C#-specific concepts leaking into Rust/TypeScript and the common failure mode of a universal IR that is merely the union of all languages.

Template renderers should be optional implementations of the final language-rendering step. A target package should be able to use templates, handwritten rendering, or both; templates receive a typed binding model and narrowly scoped helpers, not the mutable runtime/session.

## Extension workflow and safeguards

1. Add a new source by implementing `SourceProvider` against `SourceInput`, registering its descriptor, and passing source conformance fixtures for record identity, diagnostics, directory expansion, and optional write capabilities.
2. Add a data format by implementing `DataEncoder`, declaring its `DataContractDescriptor`, selecting aggregation/layout, and passing encode/decode golden fixtures.
3. Add a language by implementing `LanguageBinding`/renderer, declaring compatible contracts, and passing schema plus data-loader interoperability fixtures for every declared contract.
4. Add a combination by creating an output profile that references existing IDs. Reject unsupported combinations during config preflight; do not add a central match arm or a special-purpose product crate unless the pair has genuinely inseparable semantics.

The key CI gate is a compatibility-conformance suite: every declared `(language binding, data contract)` pair compiles generated code where practical and loads the encoder's golden output. This turns the advertised matrix into an executable contract and prevents widening support claims accidentally.

## Sequencing for Coflow

1. Add data-contract descriptors and compatibility validation while preserving existing JSON, MessagePack, and C# output behavior.
2. Refactor C# options into a binding descriptor that declares its supported data contracts; retain its existing C# IR internally.
3. Extract artifact layout/publication from exporter/codegen implementations so both branches use the root crate's artifact lifecycle consistently.
4. Introduce typed output/source option decoding and descriptor-provided schemas.
5. Add a second language or second runtime convention as the proving case. Only then decide whether a shared `BindingModel` is justified by real duplication.

This sequence is deliberately conservative: it creates the compatibility axes before adding matrix entries, but avoids a premature universal IR or external plugin ABI.
