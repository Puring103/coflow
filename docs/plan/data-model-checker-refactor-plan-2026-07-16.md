# Data Model and Checker Refactor Plan

- Date: 2026-07-16
- Status: Final implementation plan
- Scope: `coflow-cft`, `coflow-data-model`, `coflow-checker`, dimension orchestration in
  `coflow-runtime`, and direct consumers of the affected public Rust APIs
- Branch: `codex/data-model-checker-refactor-plan`

## 1. Purpose

This plan removes duplicate type and value semantics from the data-model and checker pipeline,
clarifies the boundary between source drafts and the successful canonical model, and makes the
existing incremental-check path observable and genuinely target-local. It also defines a staged
path toward incremental non-structural DataModel rebuilds without introducing persistent numeric
record identities, mutable generations, synthetic dimension records, or a second schema model.

The work is a behavior-preserving refactor first and a measured performance optimization second.
No phase may change user data syntax, project configuration, diagnostic codes, exported data
shape, generated C# shape, editor wire JSON, or mutation transaction semantics unless a later,
separately approved product change explicitly does so.

## 2. Repository Baseline

The current pipeline has several strong foundations that must be preserved:

1. `CftSchema` owns the unique successful type, field, enum, const, and dimension declarations.
2. Inherited fields share the same `Arc<CftField>` instead of copying field declarations.
3. `TypedCheckPlan` and `ValueDependencyPlan` are compiled before schema publication.
4. Dimension variant values are overlays on their owner `CfdRecord`; there are no synthetic
   dimension types, records, or independent long-lived dimension stores.
5. Direct refs, spreads, and checker read dependencies use separate edge models because their
   invalidation semantics differ.
6. Runtime generations are immutable and are published only after load, model build, checks, and
   mutation transaction requirements succeed.
7. Scalar mutation rebuilds reuse the compiled schema and reload only affected source batches.
8. Check diagnostics and read dependencies are stabilized across generations by record business
   identity.

The current limitations motivating this plan are:

1. `CftSchemaTypeRef` is a misleading name for a recursive field-value type expression.
2. `CheckedType` duplicates most successful value-type variants in addition to compiler-only
   inference states.
3. DataModel build validation and mutation value validation implement overlapping semantic rules.
4. `CfdInputValue`, `CfdValue`, and `CheckValue` form three broad parallel value trees.
5. DataModel copies schema type/domain/ancestry information into `CfdDomainIndex`.
6. Checker exposes a combinatorial set of full/subset/dimension/dependency/options entry points.
7. Checker state is split between checker output and runtime-owned stable diagnostic structures.
8. A subset dimension check still constructs each `DimensionRoundView` by scanning the entire
   model.
9. Spread path lookup scans all spread edges even though related indexes already exist.
10. Source reload is incremental, but `CfdDataModel::builder(...).build()` still rebuilds the full
    model from all cached input records.

## 3. Fixed Architectural Decisions

The following decisions are final for this refactor.

### 3.1 Canonical schema declarations

`CftSchema` remains the only successful schema semantic authority.

- `CftType` is the unique named type declaration.
- `CftField` is the unique field declaration.
- `CftEnum` is the unique enum declaration.
- `CftDimension` is the unique dimension declaration.
- Semantic declaration identity uses validated typed names.
- DataModel and checker must not publish or persist another type declaration graph.

### 3.2 Value type expressions

Rename `CftSchemaTypeRef` to `CftValueType` and rename `CftField.ty_ref` to
`CftField.value_type`.

`CftValueType` is a recursive value-type expression tree, not a declaration object and not a
handle to a copied schema type:

```rust
pub enum CftValueType {
    Int,
    Float,
    Bool,
    String,
    Object(TypeName),
    Enum(EnumName),
    RecordRef(TypeName),
    Array(Box<CftValueType>),
    Dict(Box<CftValueType>, Box<CftValueType>),
    Nullable(Box<CftValueType>),
}
```

The module moves from `schema/type_ref.rs` to `schema/value_type.rs`. No deprecated alias for
`CftSchemaTypeRef` will remain. This repository is not publishing the crate, and retaining both
names would preserve the ambiguity the migration is intended to remove.

### 3.3 Compiler inference

Replace `CheckedType` with `InferredType`. Successful value types are represented only through
`CftValueType`; compiler-only states remain explicit:

```rust
enum InferredType {
    Value(CftValueType),
    Null,
    EmptyArray,
    EmptyObject,
    EnumNamespace(EnumName),
    Entry(Box<InferredType>, Box<InferredType>),
    Unknown,
}
```

The compiler may keep unresolved syntax names before validation, but successful schema and typed
check plans must contain only validated typed names. Assignability and comparability for successful
types must have one implementation; `InferredType` only adds the rules needed for transient states
and error recovery.

### 3.4 Source drafts are not a second canonical model

Source input requires a representation that can contain missing fields, spread directives,
unresolved syntax, and invalid data. That representation remains, but it is explicitly an ingest
IR rather than another successful DataModel.

The public provider contract will use names such as:

```rust
LoadedRecordDraft
LoadedValueDraft
DimensionValueDraft
```

These replace the misleading `CfdInput*` naming. Draft types may contain syntax/build directives
that cannot exist in a successful `CfdValue`. They must not duplicate schema assignability,
nullable, enum, dict-key, object, or reference-target rules.

Provider-private parser AST or cell tokens may continue to exist. They are adapter state, not
domain types, and must be lowered through the shared ingest/semantic boundary before model
publication.

### 3.5 Record identity

Move `RecordCoordinate` from `coflow-runtime` to `coflow-data-model` and type it as:

```rust
pub struct RecordCoordinate {
    pub actual_type: TypeName,
    pub key: RecordKey,
}
```

It is the stable business identity used across generations and wire boundaries. Its serialized
shape remains the existing `{ actual_type, key }` string object.

`CfdRecordId` remains a dense, generation-local index. It must never be stored across generations
without first converting to `RecordCoordinate`. This plan deliberately rejects globally persistent
numeric IDs, tombstones, arenas, and record generation tokens.

### 3.6 Incremental scope

The first incremental DataModel implementation handles only non-structural value changes where
record membership, coordinate, and ordering remain stable. Structural changes use an explicit full
fallback until every affected index and lifecycle rule has a tested delta implementation.

Fallback is a correctness boundary, not an invisible optimization failure. Every fallback reports
a machine-readable reason.

## 4. Target DataModel Architecture

The target directory structure is:

```text
coflow-data-model/src/
  lib.rs
  model/
    mod.rs
    ids.rs
    record.rs
    value.rs
    dimensions.rs
  ingest/
    mod.rs
    record.rs
    value.rs
    directives.rs
  semantics/
    mod.rs
    validation.rs
    assignability.rs
    references.rs
  build/
    mod.rs
    context.rs
    defaults.rs
    resolve.rs
    materialize.rs
  indexes/
    mod.rs
    records.rs
    references.rs
    spreads.rs
  dependencies/
    mod.rs
    materialization.rs
  diagnostics/
    mod.rs
    codes.rs
    paths.rs
    mapping.rs
```

Directory movement happens only after the corresponding semantic ownership has been established.
No commit should consist of a large file shuffle followed immediately by another large content
rewrite.

### 4.1 Successful model identity

The successful model uses typed semantic identity internally:

```rust
CfdRecord.actual_type: TypeName
CfdRecord.key: RecordKey
CfdObject.actual_type: TypeName
CfdObject.fields: BTreeMap<FieldName, CfdValue>
CfdValue::Ref(RecordKey)
CfdEnumValue.enum_name: EnumName
CfdEnumValue.variant: Option<EnumVariantName>
CfdRecord.dimension_fields: BTreeMap<FieldName, CfdDimensionOverlay>
```

Dictionary string keys, ordinary string values, source display paths, diagnostic text, and unknown
draft field names remain strings because they are not schema declaration identities.

### 4.2 Shared semantic validation

DataModel build and runtime mutation preflight must call one recursive semantic validator. Its
conceptual request is:

```rust
pub struct ValueValidationRequest<'a> {
    pub expected: &'a CftValueType,
    pub value: ValueView<'a>,
    pub mode: ValidationMode,
    pub pending_insert: Option<PendingInsertRef<'a>>,
}
```

`ValidationMode` distinguishes:

- source fragment validation, where omitted object fields are legal;
- complete value validation, where required fields must be present;
- mutation validation, where pending records and expected-state constraints may apply.

The following rules must have exactly one implementation:

- nullable handling;
- primitive compatibility and finite floats;
- object actual-type validation and assignability;
- abstract and singleton restrictions;
- required fields and defaults;
- enum name, variant, and numeric-value consistency;
- array and dict recursion;
- dict-key compatibility;
- record-ref shape and target validation.

Source-specific spread syntax, source positions, default application, and mutation expected-state
checks remain outside this semantic core.

### 4.3 Schema relationship indexes

Remove `CfdDomainIndex`, `CfdTypeId`, and `CfdDomainId` as public DataModel concepts. They currently
copy type names, domains, members, and ancestors already owned by `CftSchema`.

Data indexes may still use canonical typed names:

```rust
record_by_type_key: BTreeMap<(TypeName, RecordKey), CfdRecordId>
record_by_domain_key: BTreeMap<(TypeName, RecordKey), CfdRecordId> // key is inheritance root
```

`CftSchema` provides the canonical queries required by model construction and consumers, including
inheritance root, assignability, ancestors, and descendants. The model index stores record lookup
results, not a second declaration graph.

Before removing the numeric caches, add lookup benchmarks. If a regression is material, optimize
inside `CftSchema` without recreating a public DataModel type system.

### 4.4 Record and relation indexes

Keep exact-type and inheritance-domain record indexes because they answer different queries.
Remove `inheritance_index` only after migrating its single production enumeration consumer to a
canonical schema/type-index query and measuring the result.

Add `spread_by_host` to the successful model. `spread_edge_at_path` must search only edges for the
requested host, not every spread edge in the project. Exact `spread_by_site` remains useful for
source rewrite and exact-site queries.

Direct ref, spread, and check-read graphs remain separate:

- direct ref edges describe stored `RecordRef` relationships;
- spread edges describe materialized source provenance and write routing;
- check read dependencies describe runtime evaluation reads.

### 4.5 Non-structural incremental model rebuild

After behavior-preserving refactors and instrumentation, successful records may use shared
ownership:

```rust
records: Vec<Arc<CfdRecord>>
```

For a non-structural mutation:

1. Reload only affected source batches.
2. Compare record draft fingerprints by `RecordCoordinate`.
3. Identify directly changed records.
4. Expand through materialization dependencies, initially spread provenance only.
5. Revalidate and rematerialize only affected records.
6. Reuse unchanged `Arc<CfdRecord>` nodes.
7. Replace relation edges for affected hosts and repair reverse buckets.
8. Produce a `ModelDelta` containing changed coordinates, dimension coordinates, and relation
   changes.

Defaults that depend only on schema and the same record do not create cross-record dependencies.
A normal ref target's field-content change does not rematerialize the referring record; checker
reads handle evaluation invalidation. A spread source content change does rematerialize every host
that inherited affected fields.

Structural insert/delete/rename remains a full model fallback in this project. Supporting it
incrementally is a later optimization requiring transactional deltas for record ordering, key
uniqueness, singleton rules, refs, spreads, dimension rows, idAsEnum, diagnostics, and checker
snapshot membership.

## 5. Target Checker Architecture

The target directory structure is:

```text
coflow-checker/src/
  lib.rs
  request.rs
  output.rs
  snapshot.rs
  dependencies.rs
  dimensions.rs
  engine/
    mod.rs
    runner.rs
    evaluator.rs
    statements.rs
    expressions.rs
  eval/
    mod.rs
    value.rs
    location.rs
    collections.rs
  operations/
    mod.rs
    access.rs
    comparison.rs
    predicates.rs
    builtins.rs
    quantifiers.rs
  diagnostics/
    mod.rs
    explanations.rs
    trace.rs
```

### 5.1 Single public execution interface

Replace the combinatorial public functions with:

```rust
pub fn run_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    request: CheckRequest<'_>,
) -> CheckOutput;
```

`CheckRequest` contains targets, rounds, structural limits, and dependency-collection policy.
`CheckOutput` always contains rooted diagnostics, a snapshot delta or complete snapshot as
requested, dependency data, and execution statistics. Empty targets perform no work.

Delete `CfdCheckExt` and the full/subset/dimension/dependency/options wrapper matrix after all
consumers migrate.

### 5.2 Borrowed evaluation values

`CheckValue` must no longer clone the complete successful `CfdValue` algebra. Replace it with a
borrowed model view plus genuinely evaluator-only states:

```rust
enum EvalValue<'a> {
    Model {
        value: &'a CfdValue,
        location: ValueLocation,
    },
    Temporary(TemporaryValue),
    EnumNamespace(EnumName),
    Entry(EvalEntry<'a>),
    UnresolvedRef,
}
```

Temporary values cover expression results that do not exist in the model. Collection iteration
remains lazy and location-aware so structural budgets and precise diagnostics are preserved.

### 5.3 Checker snapshot ownership

Move incremental checker state from runtime into `coflow-checker`:

```rust
pub struct CheckRoot {
    pub record: RecordCoordinate,
    pub round: CheckRound,
}

pub struct RootCheckState {
    pub diagnostics: Vec<LogicalCheckDiagnostic>,
    pub reads_from: BTreeSet<RecordCoordinate>,
}

pub struct CheckSnapshot {
    pub roots: BTreeMap<CheckRoot, RootCheckState>,
}
```

Runtime owns physical source-location mapping and the published session, but it must not maintain a
parallel stable checker diagnostic model. Checker diagnostics remain logical until runtime combines
them with record/source provenance.

Dependencies are recorded per root and per round. Merging all dimension rounds into one graph is
safe but over-invalidates; the new snapshot makes the round explicit.

### 5.4 Incremental checker invalidation

Given a `ModelDelta`, checker invalidation is:

1. Include every changed record as a root.
2. Include every previous root whose `reads_from` intersects changed records.
3. Resolve affected coordinates in the new generation.
4. Run only affected roots and required rounds.
5. Replace their old snapshot entries.
6. Preserve all unaffected root states.

Record-level dependencies are the initial granularity. Field-level dependencies are not introduced
until measurements demonstrate a need; premature field-level tracking would substantially expand
path, dimension, and collection invalidation complexity.

## 6. Dimension Architecture and Optimization

### 6.1 Semantics that must not change

The following current behavior is contractual:

1. Project dimension names and variants participate in schema compilation.
2. `@localized` and `@dimension` bind ordinary owner fields to a dimension.
3. `CftDimension` contains typed variants and shared canonical fields.
4. The ordinary source field is the only semantic default value.
5. Physical dimension-file `default` columns are managed mirrors.
6. Variant values attach directly to owner records.
7. Missing variant and explicit null remain distinct states.
8. Dimension refs participate in direct ref indexes and rename rewrites.
9. Singleton dimension sources retain their CFD grouping behavior.
10. Non-singleton dimension sources retain their CSV path/table behavior.
11. Mutation writes, managed-file changes, compensation, and generation publication remain one
    transaction lifecycle.
12. JSON/MessagePack exports and C# codegen retain existing dimension table names and shapes.

### 6.2 Generation-bound runtime plan

Runtime currently derives `Vec<DimensionField>` repeatedly from canonical schema fields. Replace
that repeated derivation and directory matching with one generation-bound plan:

```rust
pub(crate) struct DimensionRuntimePlan {
    fields_by_dimension: BTreeMap<DimensionName, Vec<DimensionFieldPlan>>,
    source_by_path: BTreeMap<PathIdentity, DimensionSourcePlan>,
}
```

`DimensionFieldPlan` references `TypeName`, `FieldName`, and canonical field semantics; it does not
copy `CftField` or `CftValueType`. `DimensionSourcePlan` contains runtime-only storage policy such as
bucket, managed path, provider choice, singleton grouping, and decoded source options.

The plan is rebuilt only when schema, dimension configuration, provider availability, or managed
source topology changes. It belongs to runtime, not `CftSchema`, because file naming and CSV/CFD
provider policy are project runtime concerns.

### 6.3 Target-local dimension checking

Delete the whole-model compilation performed by `DimensionRoundView::compile`.

For each requested root and round, checker will:

1. Resolve dimension-relevant statements from `TypedCheckPlan`.
2. Determine relevant dimension fields from the canonical type and runtime/check request plan.
3. Read only that root record's overlay for the requested variant.
4. Preserve missing versus explicit-null behavior.
5. Traverse nested checks only for materialized values that require them.
6. Record ref reads through the same dependency collector as default-round evaluation.

A full check still visits every root by request; a subset check must not project unrelated records.

### 6.4 Impact-scoped dimension regeneration

Dimension regeneration and reload use an explicit impact plan:

- ordinary non-dimensional field change: no dimension source work;
- default value change for a dimensional field: update only its managed default mirror;
- one variant value change: write and reload only its owning dimension source;
- insert/delete/rename: update every dimensional field applicable to the owner type;
- dimension ref rewrite: update only affected dimension sources;
- schema, dimension variants, output directory, bucket, provider, or source-topology change: full
  dimension reconcile.

The initial implementation may conservatively regenerate all fields of one affected dimension, but
must not regenerate unrelated dimensions. Execution statistics make any conservative expansion
visible.

## 7. Runtime Responsibilities

`coflow-runtime` remains the generation orchestrator. It owns:

- compiled schema and module snapshots;
- source resolution and source batch caching;
- the generation-bound dimension runtime plan;
- DataModel build/rebuild invocation;
- checker full/incremental invocation;
- source provenance and physical diagnostic mapping;
- mutation transaction, compensation, and publication;
- explicit fallback decisions.

It must not reimplement value typing, DataModel materialization, check evaluation, or checker
snapshot merge semantics.

The fallback reasons are a stable internal enum exposed through execution diagnostics/statistics as
appropriate:

```rust
enum IncrementalFallbackReason {
    SchemaChanged,
    RecordInserted,
    RecordDeleted,
    RecordRenamed,
    SourceTopologyChanged,
    DimensionConfigurationChanged,
    ProviderConfigurationChanged,
    UnstableCoordinateMapping,
    IncompleteDependencyState,
}
```

## 8. Compatibility Requirements

The refactor must preserve:

- CFT and CFD source syntax;
- `coflow.yaml` shape and dimension configuration;
- diagnostic codes, stages, primary/related source spans, and representative message content;
- CLI human and JSON output contracts;
- editor request/response JSON and TypeScript bindings;
- record coordinate wire shape;
- JSON and MessagePack table names, ordering, and value representation;
- C# public type names, fields, dimension tables, loaders, and idAsEnum behavior;
- CSV/Excel/CFD loader and writer behavior;
- mutation expected-state, no-op, batch, transaction, compensation, and affected-file behavior;
- source-origin mapping and spread write routing;
- missing versus explicit-null dimension semantics.

Rust source compatibility for renamed internal unpublished types is not a goal. The migration is
repository-wide and intentionally removes obsolete names.

## 9. Implementation Phases and Commit Boundaries

### Phase 0: Baseline observability

Add internal execution statistics without changing behavior:

```text
sources_resolved
sources_reloaded
draft_records_collected
records_validated
records_materialized
records_reused
ref_edges_rebuilt
spread_edges_rebuilt
check_roots_executed
dimension_records_projected
dimension_sources_planned
dimension_sources_written
full_fallback
fallback_reason
```

Add baseline tests that assert current full/incremental output equivalence and record representative
work counts. Statistics may remain internal or test-only unless a public diagnostic use is approved.

Commit: `test(runtime): expose model and checker execution statistics`

### Phase 1: Value type naming

1. Rename `type_ref.rs` to `value_type.rs`.
2. Rename `CftSchemaTypeRef` to `CftValueType`.
3. Rename `CftField.ty_ref` to `value_type`.
4. Update all loaders, writers, model, checker, exporter, codegen, runtime, LSP, and tests.
5. Do not change matching logic or output.

Commit: `refactor(cft): rename schema type refs to value types`

### Phase 2: Compiler inference unification

1. Introduce `InferredType`.
2. Wrap successful types as `InferredType::Value(CftValueType)`.
3. Centralize successful-type assignability/comparability.
4. Preserve all current CFT diagnostic codes and spans.
5. Delete `CheckedType` after migration.

Commit: `refactor(cft): unify canonical and inferred value types`

### Phase 3: Typed record identity

1. Move `RecordCoordinate` to `coflow-data-model`.
2. Replace internal actual-type, record-key, field-name, enum-name, and dimension-field map keys with
   validated typed names where they represent schema identity.
3. Preserve wire serialization as strings.
4. Keep unknown and invalid names confined to draft/diagnostic layers.

Commit: `refactor(data-model): use canonical typed record identity`

### Phase 4: Shared value semantics

1. Extract the single semantic validator.
2. Adapt build validation and mutation validation to it.
3. Add a conformance matrix that runs equivalent draft/build and mutation values through the same
   rule cases.
4. Delete duplicate recursive rule implementations.
5. Keep source spread/default application separate.

Commit: `refactor(data-model): centralize value semantics`

### Phase 5: Ingest and model module boundaries

1. Rename and relocate `CfdInput*` as explicit loaded draft IR.
2. Split model, build, indexes, dependencies, and diagnostics by ownership.
3. Remove `compiler_context.rs` after its schema adapter and build state have distinct owners.
4. Add `spread_by_host` and migrate path lookup.

Commit: `refactor(data-model): separate ingest build and model state`

### Phase 6: Remove duplicate schema relation model

1. Add required canonical schema inheritance-root queries.
2. Migrate record indexes and edge metadata to typed names.
3. Benchmark lookup and build costs.
4. Remove `CfdTypeId`, `CfdDomainId`, and `CfdDomainIndex`.
5. Migrate or remove `inheritance_index` after its enumeration consumer is covered.

Commit: `refactor(data-model): use canonical schema relationships`

### Phase 7: Checker value and API consolidation

1. Introduce borrowed `EvalValue` and temporary evaluator values.
2. Consolidate operation modules.
3. Introduce `CheckRequest` and `CheckOutput`.
4. Migrate every consumer.
5. Delete wrapper APIs and `CfdCheckExt`.

Commit: `refactor(checker): consolidate evaluation and execution APIs`

### Phase 8: Checker snapshot ownership

1. Introduce per-root/per-round `CheckSnapshot`.
2. Move stable diagnostic and dependency merge logic from runtime to checker.
3. Keep runtime physical-location mapping.
4. Differentially compare incremental and fresh full outputs.

Commit: `refactor(checker): own incremental check snapshots`

### Phase 9: Dimension runtime and checker optimization

1. Compile `DimensionRuntimePlan` once per generation.
2. Replace whole-model dimension round compilation with target-local projection.
3. Scope regeneration and reload by dimension impact.
4. Preserve source generation, transaction, export, and codegen contracts.

Commit: `perf(runtime): make dimension work generation and target scoped`

### Phase 10: Non-structural incremental DataModel rebuild

1. Introduce shared successful record nodes.
2. Add draft fingerprints and materialization dependencies.
3. Implement affected-record rematerialization and relation-edge replacement.
4. Produce `ModelDelta`.
5. Drive checker invalidation from `ModelDelta`.
6. Keep structural fallback explicit.

This phase proceeds only after Phase 0 statistics show that full model materialization remains a
meaningful cost after checker and dimension optimization. If it is not a material cost, retain full
immutable model rebuilds and document that deliberate choice rather than adding unused complexity.

Commit if justified: `perf(data-model): rebuild non-structural model deltas`

## 10. Verification Matrix

### 10.1 Schema and compiler

- field value types for every primitive/container/object/enum/ref/nullable combination;
- unknown and ambiguous names;
- invalid dict keys;
- nullable/null, empty array, and empty object inference;
- enum namespace and entry inference;
- inherited check scheduling;
- diagnostic code, span, and related-label stability.

### 10.2 DataModel semantics

- draft/build and mutation validation conformance;
- required/default/nullable behavior;
- abstract, sealed, struct, singleton, and polymorphic objects;
- enum and flag enum behavior;
- dict key validation and duplicates;
- direct refs and missing/wrong-domain targets;
- object and dict spreads;
- spread provenance and effective ref lookup;
- structural budget behavior.

### 10.3 Checker

- every expression, statement, builtin, predicate, and quantifier;
- nested object/array/dict checks;
- borrowed evaluation values produce identical diagnostics;
- read dependencies include every dereferenced record;
- per-round dimension dependencies;
- budget exhaustion behavior;
- subset execution performs no work for empty targets;
- incremental snapshot output equals a fresh full run.

### 10.4 Dimensions

- `@localized` and generic `@dimension` binding;
- multiple dimensions and variants;
- missing versus explicit null;
- nested dimension values and checks;
- dimension refs and rename rewrite;
- singleton CFD grouping;
- non-singleton CSV sources and buckets;
- generated source create/update/remove/rename;
- mutation set/clear/insert/delete/rename;
- compensation and affected files;
- target-local checker work counts;
- impact-scoped regeneration work counts;
- JSON/MessagePack export golden files;
- C# generation golden output.

### 10.5 Runtime and editor

- source reload count for affected and unaffected providers;
- no-op mutation does not advance generation;
- structural mutation reports full fallback;
- stale expected-state behavior;
- diagnostics indexes remain generation-current;
- editor wire binding compatibility;
- watcher attribution and generated dimension paths;
- incremental and fresh session diagnostic equality.

## 11. Performance Acceptance Criteria

Performance work must demonstrate both result equivalence and reduced work.

For a scalar change to one record with no readers or spread dependents:

- only its source is reloaded;
- no unrelated dimension source is planned, written, or reloaded;
- only that record is materialized if incremental DataModel rebuild is enabled;
- only that record's check roots are executed;
- no unrelated records are dimension-projected;
- no full fallback occurs.

For a changed record read by another record's check:

- the changed root and direct reader roots execute;
- unrelated roots remain in the previous snapshot;
- output equals a fresh full check.

For a spread source change:

- hosts inheriting affected fields rematerialize;
- unrelated spread hosts do not;
- effective ref and write provenance remain correct.

Benchmarks must include projects with deep inheritance, many refs, many spreads, multiple dimensions,
and multiple variants. Wall-clock measurements are supporting evidence; deterministic work counters
are the primary regression guard.

## 12. Risk Controls

1. Do not combine the type rename with semantic behavior changes.
2. Do not combine directory movement with algorithm replacement.
3. Do not implement incremental model deltas before execution statistics exist.
4. Do not remove a full fallback until a differential test covers the structural operation.
5. Do not move runtime storage policy into canonical schema declarations.
6. Do not let source draft recovery states enter a successful `CfdValue`.
7. Do not persist `CfdRecordId` across generations.
8. Do not merge ref, spread, and checker dependency graphs.
9. Do not alter dimension export or codegen shape as part of performance work.
10. Do not publish a generation after any failed model, check, transaction, or required mapping step.

Each phase must be independently revertible. If a phase cannot maintain the existing public behavior
and required checks, stop at the previous stable boundary rather than layering compatibility shims
over an incomplete migration.

## 13. Required Repository Checks

For every normal development commit, run only the repository-required commands from the worktree
root:

```powershell
cargo check --workspace
cargo test --workspace
```

Do not require `cargo fmt` or `cargo clippy` for normal development commits. Release or packaging
work is outside this plan and must use the separate full release gate defined in `AGENTS.md`.

## 14. Completion Criteria

The refactor is complete when all of the following are true:

1. `CftSchemaTypeRef`, `CheckedType`, and `CfdCheckExt` no longer exist.
2. Successful schema value types use only `CftValueType` and validated typed names.
3. DataModel build and mutation share one value semantic implementation.
4. Source drafts are clearly named ingest IR and cannot be mistaken for successful model values.
5. DataModel no longer publishes a duplicate schema type/domain declaration model.
6. Cross-generation record identity uses typed `RecordCoordinate`; numeric IDs remain local.
7. Checker uses borrowed model values and one public execution request/output pair.
8. Checker owns per-root/per-round snapshots and dependency merge behavior.
9. Subset dimension checks perform no whole-model projection.
10. Dimension runtime planning is generation-bound and regeneration is impact-scoped.
11. Incremental work and fallback reasons are observable.
12. Incremental outputs match fresh full outputs for every supported incremental operation.
13. All compatibility requirements and golden outputs remain unchanged.
14. `cargo check --workspace` and `cargo test --workspace` pass.

The plan intentionally accepts full immutable model rebuilds for structural mutations and may retain
them for all model mutations if measured model-build cost is not material after checker and dimension
optimization. That is a documented simplicity tradeoff, not an unobserved claim of incremental work.
