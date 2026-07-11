# Core Architecture Refactor Plan

## Scope

This plan continues the core codebase architecture review after merging `main`
into `codex/aggressive-architecture-plan`. It excludes editor-specific
architecture work except where `main` already brought editor files through the
merge.

Normal development gates for every implementation commit:

```powershell
cargo check --workspace
cargo test --workspace
```

Do not run `cargo fmt` or `cargo clippy` for these normal refactor commits.

## Already Covered

- Table source option decoding is already deepened into
  `coflow-loader-table-core::TableSourceOptions`.
- The provider adapters now keep only thin diagnostic-label and type-specific
  adapter code around that shared table options module.

## Module Plan

### 1. Diagnostics flat view and source positions

Status: completed.

Goal: move diagnostic flattening locality into diagnostics-owning modules.

Implementation:

- Add `DiagnosticSet::flat_diagnostics()` for diagnostics without logical
  record context.
- Add `DiagnosticsStore::flat_diagnostics()` for diagnostics with logical
  record context.
- Replace duplicated flatten loops in runtime and CLI adapters.
- Keep wire shapes unchanged.

Commit boundary: `refactor: centralize flat diagnostics`

### 2. Schema path policy

Status: completed.

Goal: make schema source path validation and discovery use one deeper module
interface.

Implementation:

- Introduce an internal `SchemaPathPolicy` or equivalent module in
  `coflow-project`.
- Share resolve, normalize, `.cft` extension policy, canonicalization, and
  module id generation between config validation and schema file discovery.
- Keep existing diagnostics text and public behavior unchanged.

Commit boundary: `refactor: centralize schema path policy`

### 3. Runtime session intent

Goal: make session side effects visible in the runtime interface.

Implementation:

- Introduce explicit session open intent/options in `coflow-runtime`.
- Route `Runtime::open_project_session` and `Runtime::build_project_session`
  through that interface.
- Keep existing facade methods as the public host-facing adapter.

Commit boundary: `refactor: make runtime session intent explicit`

### 4. Runtime write plan

Goal: improve locality for write target resolution, validation, writer lookup,
and report metadata.

Implementation:

- Add an internal write plan module used by direct writes and mutation apply.
- Move duplicated path/type/source/writer preparation behind one internal
  interface.
- Keep `ProjectSession` public write methods behavior unchanged.

Commit boundary: `refactor: introduce runtime write plans`

### 5. Provider role registration

Goal: allow one provider implementation instance to satisfy multiple role
interfaces without repeated construction.

Implementation:

- Add registry helpers for shared `Arc` role registration or a role-bundle
  adapter.
- Update builtins registration where the same provider is constructed multiple
  times.
- Keep role lookup interfaces unchanged.

Commit boundary: `refactor: share provider role registrations`

### 6. LSP validation core

Goal: keep `LspServer` as protocol adapter and move validation/publishing
state into deeper modules.

Implementation:

- Extract document storage, build coordination, and diagnostic publishing out
  of `coflow-lsp/src/lib.rs`.
- Preserve all protocol behavior and schema-only LSP scope.
- Keep tests targeted at the extracted module where possible.

Commit boundary: `refactor: extract lsp validation core`
