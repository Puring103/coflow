# Core Architecture Follow-up Plan

- Date: 2026-07-23
- Scope: non-host workspace crates and the root application service
- Excluded: editor/frontend/host implementation details
- Status: proposed

## 1. Purpose

The current core architecture has a sound primary flow:

```text
CFT schema -> source provider drafts -> DataModel -> Checker -> Runtime -> application service
```

The recent core hardening work removed several important sources of ambiguity: duplicate schema
models, provider-owned directory discovery, duplicate record field storage, generation-local IDs in
public contracts, and scattered reference resolution. The remaining work does not justify another
large crate reorganization. It is a focused follow-up covering source-write reliability, command
semantic consistency, public dependency boundaries, residual table-provider duplication, and API
clarity.

This plan records the problems, proposed changes, compatibility constraints, verification, and
acceptance criteria so that each item can be implemented and reviewed independently.

## 2. Current Structure

### 2.1 Primary ownership

| Crate | Current ownership |
| --- | --- |
| `coflow-structure` | Shared structural limits and traversal accounting |
| `coflow-cft` | CFT syntax, compilation, schema semantics, static check plans |
| `coflow-cfd` | CFD syntax, parser, canonical AST and source spans |
| `coflow-loader-cfd` | Schema-guided CFD lowering and CFD source writes |
| `coflow-loader-table-core` | Shared table value grammar and table algorithms |
| `coflow-loader-csv` / `coflow-loader-excel` | Physical CSV/Excel adapters |
| `coflow-data-model` | Source-neutral values, records, validation and relationship indexes |
| `coflow-checker` | Runtime check execution and incremental snapshots |
| `coflow-project` | Project configuration, path policy and schema discovery |
| `coflow-api` | Provider SPI, provider registry, diagnostics and write contracts |
| `coflow-runtime` | Source resolution, load/build/check, queries and mutation orchestration |
| `coflow-builtins` | Default provider composition root |
| Root `coflow` crate | Application commands and artifact publication lifecycle |

### 2.2 Boundaries to preserve

The following splits are useful and should remain:

- `coflow-cft` and `coflow-cfd`: schema and data languages have different semantics and lifecycles.
- `coflow-cfd` and `coflow-loader-cfd`: parsing is independent from schema-guided lowering.
- `coflow-data-model` and `coflow-checker`: successful data semantics are independent from check
  execution state.
- `coflow-loader-table-core` and concrete table providers: the shared crate owns real cross-provider
  behavior while concrete crates retain physical-format access.
- `coflow-builtins`: the default registry composition root is intentionally thin.

## 3. Findings

### 3.1 P1: Source file replacement is not atomic

`schema write-file` and `data write-file` ultimately use `std::fs::write` through
`src/write_file.rs`. This truncates an existing file before the replacement contents have been
fully persisted. A full disk, interrupted process, antivirus/file-system error, or machine failure
can leave a valid user source as an empty or partial file.

This is inconsistent with the repository's artifact publication lifecycle, which already stages
and atomically publishes generated outputs. User-authored CFT/CFD files deserve at least the same
single-file replacement guarantee.

Required outcome:

- Write a temporary file in the destination directory.
- Write and flush the complete contents before replacement.
- Atomically replace the destination using behavior that is correct on Windows.
- Preserve useful diagnostics for create, write, flush and replace failures.
- Remove temporary files on ordinary failure paths without risking the destination.

### 3.2 P1: Data dry-run cannot validate candidate contents

`schema write-file --dry-run --check` compiles the in-memory candidate source. The corresponding
data command suppresses checking whenever `--dry-run` is active and returns `check_ok: null`.

Although this limitation is documented, it is an inconsistent command contract and prevents tools
and agents from safely validating a complete CFD replacement before touching disk. A check that
only runs after the destructive action has lower practical value.

Required outcome:

- `data write-file --dry-run --check` loads and checks the candidate CFD text in memory.
- Candidate validation uses the same provider lowering, DataModel build and Checker execution as a
  real project load.
- No dimension source is generated and no source file is mutated during dry-run.
- The result reports `written: false`, the real `changed` value, and a non-null `check_ok`.

### 3.3 P2: `coflow-api` combines foundational diagnostics and high-level provider SPI

`coflow-api` is described as the public provider API, but it directly depends on both `coflow-cft`
and `coflow-data-model`. Its diagnostics module also maps DataModel diagnostics and origins into the
wire diagnostic representation. Consequently, `coflow-project` depends on the entire provider API
even though it primarily needs foundational diagnostic and source-location types.

This is not a dependency cycle, but it makes the layering harder to explain, broadens recompilation,
and turns unrelated provider/DataModel changes into potential API compatibility changes for project
configuration consumers.

Target direction:

- Extract only genuinely foundational diagnostic contracts into a lightweight lower-level crate,
  tentatively `coflow-diagnostics`.
- Keep DataModel-specific diagnostic mapping with the DataModel/runtime integration boundary rather
  than in the foundational crate.
- Keep provider traits, registry, typed options, writer requests and output contracts together.
- Consider renaming `coflow-api` to `coflow-provider-api` only in a planned breaking release; do not
  combine the extraction with an immediate rename.

The extraction must be justified by a concrete dependency graph and consumer scan before editing.
If it does not materially reduce coupling, retain the current crate and document it explicitly as a
provider SPI aggregation layer.

### 3.4 P2: Runtime public API mixes domain services and presentation helpers

`coflow-runtime/src/lib.rs` exports sessions, mutation commands, schema inspection DTOs, file-tree
views, display-name helpers, value summaries, path formatting, and re-exported domain name types from
one flat facade. Some exports explicitly exist to keep host wire formatting consistent.

This creates two risks:

- Presentation needs can continuously expand the core runtime compatibility surface.
- Consumers cannot easily distinguish stable domain operations from convenience projections.

Target direction:

- Organize public APIs into explicit `session`, `query`, `mutation`, `schema_inspect`, and
  `presentation` modules while preserving temporary root re-exports for compatibility.
- Move pure formatting/projection helpers out of load or mutation internals.
- Keep authoritative queries and mutation semantics in runtime; do not duplicate them in hosts.
- Remove a root re-export only after a workspace consumer scan and a documented compatibility
  decision.

This is primarily an API organization task, not a request to split `coflow-runtime` into more crates.

### 3.5 P2: CSV and Excel option adapters retain structural duplication

The CSV and Excel option modules duplicate typed-option decoding, allowed-key validation,
sheet-to-type/type-to-sheet lookups, and diagnostic construction. Most of the semantic behavior
already belongs to `coflow-loader-table-core`; the remaining adapter code differs mainly by provider
identity and diagnostic labels.

Required outcome:

- Add a small table-core helper for decoding table source options with provider-specific metadata.
- Centralize unknown-key validation and shared key-path construction.
- Keep provider-specific diagnostic codes, stages and physical sheet behavior in the concrete crate.
- Prefer ordinary generic functions/configuration structs over a macro that hides control flow.
- Add CSV/Excel conformance tests proving identical option behavior where the formats are intended
  to agree.

### 3.6 P3: `Project` exposes fields that form a validated invariant

`Project` publicly exposes `config_path`, `root_dir`, and `config`. Consumers can mutate one field
without updating the others after `Project::open` has canonicalized and validated them. This permits
states that normal construction cannot produce and makes future caching or derived path state hard
to introduce safely.

Required outcome:

- Make the fields private.
- Add narrow immutable accessors such as `config()`, `config_path()` and `root_dir()`.
- Prefer explicit project methods for path resolution and diagnostic construction.
- Provide a test-only builder only if existing tests genuinely need synthetic invalid states.

This change requires a complete workspace consumer migration and may be staged with deprecated
accessors if external compatibility is relevant.

### 3.7 P3: Source-neutral DataModel types retain format-specific `Cfd` names

The DataModel is explicitly source-neutral, but its principal types use names such as
`CfdDataModel`, `CfdValue` and `CfdRecord`. Because CFD is also the name of a concrete input syntax,
the names imply that CSV and Excel are converted into a CFD-owned representation instead of a
canonical Coflow data model.

Target direction for a breaking release:

- `CfdDataModel` -> `DataModel`
- `CfdValue` -> `DataValue` or `Value`
- `CfdRecord` -> `DataRecord` or `Record`
- Retain temporary type aliases and migration notes where practical.

Do not perform this rename as an isolated large diff in the current compatibility window. Combine
it with a deliberate public API version boundary and avoid renaming diagnostic codes or the actual
CFD language types.

### 3.8 P3: Historical design material is mixed with current architecture guidance

The first cleanup removed the legacy `docs/superpowers` implementation plans and completed audits.
The remaining material under `docs/plan` still mixes active proposals with historical decision
context and does not consistently identify which document describes the current architecture.

Required outcome:

- Maintain one concise current architecture document and decision index.
- Mark completed plans with final status and links to superseding documents.
- Remove obsolete implementation plans after their durable decisions are reflected in current
  architecture or reference documentation.
- Keep `README.md` user-facing, consistent with repository policy.

## 4. Implementation Phases

### Phase 0: Baseline and compatibility inventory

1. Record the current crate dependency graph using `cargo metadata`.
2. Scan all workspace consumers of `Project` fields and runtime root re-exports.
3. Add failure-injection coverage for source replacement before changing the writer.
4. Add a failing integration test for `data write-file --dry-run --check` candidate validation.
5. Record existing command JSON shapes as compatibility fixtures.

Exit criteria:

- Every public API change has a known consumer list.
- Both P1 problems have reproducible tests.
- No host/editor process is required for verification.

### Phase 1: Reliable and symmetric source writes

1. Introduce a single atomic source replacement helper in the root application service.
2. Route schema and data whole-file writes through it.
3. Add runtime/provider support for an in-memory candidate source override.
4. Run data dry-run candidate contents through full load/build/check without writes.
5. Align CLI documentation and tests for schema/data dry-run behavior.

Exit criteria:

- Injected write/flush/replace failures never truncate the original source.
- Schema and data dry-run checks both return a non-null `check_ok`.
- Dry-run creates or modifies no project files.

### Phase 2: Low-risk duplication and invariant cleanup

1. Consolidate CSV/Excel shared option decoding in table core.
2. Privatize `Project` state and migrate workspace consumers to accessors.
3. Add conformance and invariant tests.

Exit criteria:

- Concrete table providers retain no duplicated unknown-key traversal.
- `Project` cannot be placed into an invalid state through its public API.
- No project configuration or provider behavior changes.

### Phase 3: Public boundary clarification

1. Measure the actual dependency reduction from a diagnostics extraction.
2. Extract foundational diagnostics only if the measured graph supports it.
3. Group runtime public APIs into named modules with compatibility re-exports.
4. Publish a current architecture and API ownership document.

Exit criteria:

- Crate ownership can be explained without exceptions or hidden reverse dependencies.
- Existing consumers continue to compile during the compatibility period.
- Presentation helpers are visibly separated from authoritative runtime operations.

### Phase 4: Breaking-release naming cleanup

1. Decide canonical source-neutral DataModel names.
2. Introduce aliases and migration guidance.
3. Rename only as part of a declared breaking API release.
4. Archive superseded plans and update the architecture index.

Exit criteria:

- `Cfd` names refer to the CFD language/format rather than the source-neutral model.
- Public migration is documented and mechanically searchable.

## 5. Verification

Normal implementation commits must run the repository-required checks from the root:

```powershell
cargo check --workspace
cargo test --workspace
```

Focused tests should additionally cover:

| Area | Required coverage |
| --- | --- |
| Atomic write | Existing file preserved on create/write/flush/replace failure |
| Schema write-file | write, dry-run, check success, check diagnostics, unchanged content |
| Data write-file | same matrix, including dry-run candidate full-project checks |
| Candidate override | default and dimension data, references, spreads and invalid syntax |
| Table options | CSV/Excel valid, unknown, malformed and empty options conformance |
| Project API | canonical root/config invariants and path resolution |
| API migration | workspace consumer compile checks and JSON compatibility fixtures |

Release or packaging work must use the full gate defined in `AGENTS.md`; this plan does not alter
those requirements.

## 6. Non-goals

The following changes are explicitly outside this follow-up unless new measurements demonstrate a
need:

- Merging `coflow-cft`, `coflow-cfd`, `coflow-loader-cfd`, `coflow-data-model`, or
  `coflow-checker`.
- Introducing incremental DataModel mutation, permanent numeric IDs, arenas, or new cache layers.
- Combining reference, spread and checker dependency graphs.
- Moving provider selection or directory discovery back into concrete loaders.
- Rewriting diagnostics, CLI JSON, export formats, C# output, CFT syntax or CFD syntax.
- Broad directory moves or naming churn without a compatibility plan.

## 7. Acceptance Criteria

1. Whole-file source replacement cannot corrupt the previous file on an interrupted write.
2. Schema and data `write-file --dry-run --check` validate candidate contents consistently.
3. Foundational diagnostic ownership and provider SPI ownership are explicit and defensible.
4. Runtime's public surface distinguishes authoritative operations from presentation projections.
5. CSV and Excel share table option semantics without duplicated validation algorithms.
6. A publicly constructed `Project` always satisfies its path/config invariants.
7. Source-neutral model naming has a documented breaking-release migration path.
8. Current architecture guidance is distinguishable from historical plans.
9. `cargo check --workspace` and `cargo test --workspace` pass for every implementation commit.
10. No change in this plan requires starting, stopping or restarting the CFD editor.

## 8. Recommended Order

Implement Phase 1 first because it closes user-data reliability and safe-preview gaps. Phase 2 is
small enough to follow independently. Phase 3 should proceed only after the dependency and consumer
inventory proves the benefit. Phase 4 belongs to a breaking release and should not block the earlier
correctness work.
