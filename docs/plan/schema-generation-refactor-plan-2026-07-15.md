# Schema Generation Refactor Plan - 2026-07-15

## Goal

Reduce project-level CFT work to one immutable pipeline:

```text
CftFile collection -> CftModuleSet -> CftSchema -> runtime SchemaGeneration
```

`coflow-project` discovers and reads configured files. `coflow-cft` parses the
modules and builds the final semantic schema. `coflow-runtime` owns schema
generation caching and decides whether a project refresh needs a schema rebuild.
Hosts do not build, inject, or cache schemas themselves.

## Final Interfaces

```rust
pub fn parse_modules(files: impl IntoIterator<Item = CftFile>) -> CftModuleSet;

pub fn build_schema(
    modules: &CftModuleSet,
    dimensions: &CftDimensions,
) -> Result<CftSchema, CftDiagnostics>;
```

`CftModuleSet` owns stable module ids, source text, parsed ASTs, paths, and
parse diagnostics. `CftSchema` is the only semantic schema consumed by loaders,
checkers, exporters, code generators, and data-model code. `SchemaGeneration`
is runtime-private and owns cached `Arc<CftModuleSet>` and `Arc<CftSchema>`
along with its input fingerprint.

## TDD Seams

Tests are written first at these approved public seams:

1. `parse_modules` and `build_schema`: module and dimension inputs yield a
   single validated `CftSchema`, including dimension and naming diagnostics.
2. `ProjectRuntime::refresh`: unchanged schema input reuses the current schema;
   schema input changes replace it only after a successful build.
3. Host integrations: CLI, LSP, and editor consume the same runtime-built
   schema; LSP reads ASTs from the module set instead of parsing a second time.

## Deletions

The migration removes rather than deprecates `CftContainer`, its registration,
compile, reflection forwarding, and runtime type injection methods. It also
removes `SchemaBuild`, `SchemaSourceOverride`,
`compile_schema_project_with_overrides`, `compile_project_schema`,
`build_project_schema_with_diagnostics`, `build_project_schema_session`, the
runtime dimension injection module, LSP's second schema parse, and the parallel
source/path maps.

## Phases

### 1. Immutable CFT Inputs

- Add `CftFile` and `CftModuleSet`.
- Add `parse_modules` and test parse/module source access through its public
  interface.
- Rename `CompiledSchema` to `CftSchema` while retaining semantic behavior.

### 2. Single Effective CFT Build

- Move dimension synthesis into CFT's schema build implementation.
- Add `CftDimensions`; validate variants and global names in the same build.
- Delete `CftContainer` and runtime type injection.

### 3. Runtime Generation

- Replace the runtime's duplicate schema wrappers with one build adapter.
- Add runtime-owned cached `SchemaGeneration` and project-level `refresh`.
- Reuse the schema for data-only and mutation reloads.

### 4. Host Migration and Cleanup

- Migrate CLI, LSP, and editor to the single runtime build path.
- Remove LSP reparsing and editor-owned full schema reload decisions.
- Delete all old entry points and fixtures.

## Commit and Verification Strategy

Each phase is developed as red-green vertical slices and committed only after:

```powershell
cargo check --workspace
cargo test --workspace
```

The final review verifies the deletion list, dependency direction, TDD seam
coverage, and that every host reaches the same `CftSchema` generation.
