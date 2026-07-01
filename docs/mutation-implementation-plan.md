# Mutation Implementation Plan

## Goal

Unify CLI, editor, and future host data edits behind an engine-level mutation API. Hosts should adapt user input into mutation requests; the engine owns validation, default materialization, value coercion, writer dispatch, rebuild, and diagnostics.

## Target Shape

`coflow-engine` exposes staged mutation entry points:

```rust
pub fn prepare_mutation(&self, request: MutationRequest) -> Result<PreparedMutation, DiagnosticSet>;
pub fn apply_prepared_mutation(
    &mut self,
    registry: &ProviderRegistry,
    prepared: PreparedMutation,
) -> Result<MutationReport, DiagnosticSet>;
pub fn apply_mutation(
    &mut self,
    registry: &ProviderRegistry,
    request: MutationRequest,
) -> Result<MutationReport, DiagnosticSet>;
```

`apply_mutation` remains the normal path. The staged interface exists for dry-run, preview, editor validation, and future batch planning. Batch application must still prepare each pending op against the latest session state, because earlier ops can create or rename records that later ops address.

## Mutation Operations

The first implementation supports:

- `insert_record`
- `set_field`
- `rename_record`
- `delete_record`

Paths must support field, array index, and dict key segments. CLI patch should gain `rename_record` and dict-key path support while editor commands keep their current Tauri surface and adapt internally.

## Default Materialization

Schema defaults must not be blindly written into data files. A schema default is dynamic project behavior; materializing it turns it into a record-specific override and prevents later schema default changes from flowing through.

Use explicit policies:

- `Minimal`: write user-provided fields and only structure required for a valid persisted edit. This is the default for inserts.
- `EditableShape`: build a full object shape for editor drafts and inline object editing; this is not the persisted payload.
- Snapshot/undo restore is not a defaulting policy. Restore flows pass captured fields as explicit values so schema defaults are not re-materialized accidentally.

Rules:

- User-provided values are written.
- Fields with explicit schema defaults are omitted under `Minimal` unless the user explicitly provides a value.
- Fields with no schema default are still required by the data model, including nullable, array, and dict fields. Under `Minimal`, materialize safe structural values (`null`, `[]`, `{}`) only when they are required to keep the record loadable.
- Inline objects are omitted under `Minimal` unless they are schema-required and can be safely instantiated. Recursive required inline objects return mutation diagnostics for persisted edits; `EditableShape` truncates recursion by cycle detection for UI drafts.
- Refs without a user value are not fabricated. Required `@ref`, abstract, singleton, or otherwise unsafe object fields return mutation diagnostics unless the caller provides an explicit value.
- For required scalar or enum fields with no schema default, materialize a safe type default or return diagnostics when no safe value exists. The first implementation uses safe defaults for scalar/enum/nullable/collection fields where required, but keeps explicit schema defaults unmaterialized.

## Implementation Strategy

Implement in one development pass, but keep the code boundary staged:

1. Add `crates/coflow-engine/src/mutation.rs`.
2. Move shared patch preparation rules from `data_patch.rs` into mutation helpers.
3. Move editor default object logic into engine as `EditableShape`.
4. Keep existing `writes.rs` writer-dispatch and rebuild logic as internal execution helpers. Do not reimplement rename/reference rewrite logic.
5. Change `data_patch.rs` into a JSON patch adapter over `MutationRequest`.
6. Change editor backend write commands to build mutation requests and wrap mutation reports into existing editor response DTOs.
7. Change frontend record creation so default editor shape is not sent as the persisted insert payload.

## Testing

Add or update tests for:

- insert does not materialize explicit schema defaults under `Minimal`;
- insert still writes enough structure for valid records when fields have no schema default;
- set-field uses shared path/type coercion, including dict-key paths;
- rename updates references through the mutation API;
- CLI patch covers insert, set, rename, delete, and dict-key paths;
- editor backend insert returns refreshed file records and does not block on incomplete default objects;
- undo/redo restore re-inserts captured snapshot fields as explicit values.

## Checks

Before pushing a branch, run from the repository root:

```powershell
cargo check --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
