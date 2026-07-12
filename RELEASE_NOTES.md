# Coflow 0.6.0

## Breaking Changes

- Output directories now use immutable generation publication. `outputs.data.dir` and
  `outputs.code.dir` are placement anchors; the active output paths are recorded in
  `.coflow/artifacts/active.json`. Do not assume the configured path is replaced in place.
- `coflow data patch --patch` now accepts inline JSON. Use `--patch-file` for a file and
  `--stdin` for standard input.
- The Rust Provider and Runtime integration APIs now use typed options, Provider registry roles,
  and capability sessions. Integrations that depend on internal crate APIs must adapt to 0.6.

## New Capabilities

- Added `coflow data create-table` for schema-guided Excel and Lark sheet/table creation.
- `data patch` is now transactional across sources. Failed preflight, writes, rebuilds, or commits
  compensate prior writes; successful batches refresh the generation once.
- Header synchronization now shares CSV, Excel, and Lark semantics for column additions,
  removals, and reordering while preserving row alignment.
- The CFD editor adds record creation, nullable clear/create controls, polymorphic type switching,
  and more stable table and graph views.
- Added a complete workflow example project.

## Reliability and Correctness

- Added structural/work limits to CFT compilation and parsing, DataModel materialization, and the
  checker, plus improved recovery and dependency-cycle diagnostics.
- Localized checks now run according to actual dimension dependencies and support nested
  dimension subtrees.
- Generated dimension files migrate on source-field rename while preserving variant values, and
  are removed when the source field is deleted.
- Generated C# MessagePack loaders support table self-references and mutual references.
- LSP and editor sessions isolate diagnostics from disposed generations.
- Artifact publication writes, syncs, and verifies a complete generation before atomically
  activating its manifest, preserving the previous active generation on failure.

## Other Improvements

- JSON and MessagePack exports are streamed and report table, record, and field paths on errors.
- Lark requests, credential refresh, and remote table state are unified to avoid cross-credential
  cache reuse.
- Source directory resolution, schema discovery, and portable artifact path validation are more
  strict.
