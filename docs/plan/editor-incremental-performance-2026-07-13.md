# Editor Incremental Performance - 2026-07-13

## Problem

Before this work, one scalar editor write could pay for the complete project pipeline several
times:

1. The provider opened and saved its source for every field operation.
2. The runtime rebuilt schema state, resolved every source, loaded every source, rebuilt the model,
   and ran every check.
3. The filesystem watcher observed the editor's own source and generated-dimension writes, waited
   for its 350 ms debounce, and performed another full reload.
4. The frontend waited for the backend generation, refetched file and graph data, and reran ELK
   layout even when only scalar text changed.

On `examples/workflow`, the pre-change measurements were approximately 43-55 ms for an Excel
scalar write, 44-54 ms for a localized write, and another 33 ms for a watcher reload. Visible UI
stabilization could approach 430 ms because the duplicate reload followed the watcher debounce.

## Design

The optimization keeps immutable editor/runtime generations and narrows the work needed to produce
the next generation. It does not introduce an eventually consistent backend model.

### Runtime Generation

- A single-operation field write compares the validated effective target value with the current
  generation. An equal value returns a no-op outcome without opening a provider transaction or
  advancing the generation. Multi-operation mutations do not use this shortcut because later
  operations must observe the ordered effects of earlier operations.
- Mutation generations reuse the open project's compiled schema.
- Resolved source batches are cached by provider/source identity. Immutable record batches use
  shared ownership between generations; only mutation-affected sources are loaded again. Model
  construction still takes one owned copy because `CfdDataModel` owns its build input.
- Dimension regeneration reports every file it actually creates, changes, renames, or removes.
  A changed implicit dimension plan is refreshed and only its changed sources are loaded again.
- Check diagnostics and read dependencies are stabilized by `RecordCoordinate`. Scalar writes rerun
  checks for changed roots and records that read them, then merge the results with unaffected check
  state. Insert, delete, rename, missing coordinates, or an unstable diagnostic mapping fall back to
  full checks. A stabilization failure preserves the raw diagnostics and disables incremental reuse.
- Adjacent field writes for one resolved source use the provider-neutral batch writer contract.
  Excel plans the complete batch, opens the workbook once, applies plans in order, and saves once.
  The failing operation index and transaction compensation contract remain exact.

### Watcher Attribution

After a committed editor generation, the editor records content revisions for every reported source
and generated dimension file. Both expected-present and expected-missing paths are tracked. Watcher
events whose content matches those revisions are internal and do not schedule a reload; later external
content at the same path is still detected.

### Editor Publication

- Tauri commands are async adapters and move blocking runtime/provider work to the blocking pool.
- Scalar values are projected into file and graph caches immediately. Deep-equal values skip the
  backend call. Failed writes roll back only while their captured editor generation is current.
- Same-field writes waiting in the mutation history queue coalesce to the latest value. When an older
  write commits, all still-pending optimistic projections are reapplied before the next queued write
  starts, with a refreshed rollback baseline.
- A mutation publishes the union of persisted `affected_files` and the fallback host file. This is
  required for spread writes, where the persisted source record and displayed host record differ.
- Record and field diagnostics in table, record, inspector, and graph views derive from the current
  project diagnostics index rather than a possibly older cached row.
- Graph topology signatures include nodes, edges, collapse state, and container layout shape, but not
  scalar contents. Scalar generations replace visible node data while retaining positions. Reference,
  rename, collection, insert, and delete changes use the topology fallback and rerun layout.

## Correctness Boundaries

- Provider selection, source option decoding, and directory expansion remain in runtime source
  resolution.
- Transaction enlistment, compensation, and publication still use the shared mutation execution plan.
- Structural mutations always rebuild source/model/check state conservatively.
- Incremental check output is differentially tested against a full check result.
- Cached source inputs are immutable. A generation is published only after reload, model build,
  checks, provider prepare-commit, and transaction commit succeed.
- Frontend generation and history epochs reject queued work from an externally replaced project.

## Verification

An isolated copy of `examples/workflow` was measured through `SessionStore` with six samples per
operation. The probe was temporary and is not part of the repository.

| Operation | Minimum | Median | Maximum | Average |
| --- | ---: | ---: | ---: | ---: |
| Warm full reload | 28.82 ms | 30.57 ms | 36.28 ms | 31.02 ms |
| Excel scalar write | 31.75 ms | 34.32 ms | 35.08 ms | 33.58 ms |
| Localized write | 28.71 ms | 31.58 ms | 37.41 ms | 32.35 ms |

The same probe measured an authoritative no-op at 0.073 ms and a 4-node/3-edge graph query at
0.151 ms. All 18 internal write event sets were classified as internal, including generated
localization files, and the copied project passed `coflow check` after the probe restored its values.

Deterministic regression coverage verifies:

- no-op transaction suppression and ordered multi-write semantics;
- affected-source load counts and immutable schema reuse;
- incremental/full check equivalence and diagnostic fallback;
- Excel one-open/one-save batching, failure index, and compensation;
- generated-file watcher attribution;
- deep field projection, generation rollback, queued-write coalescing/reapplication;
- cross-file spread publication, current-generation diagnostics, and graph topology reuse.

Normal development gates remain the repository-required `cargo check --workspace` and
`cargo test --workspace`. The frontend additionally runs `npm test` and `npm run build`.
