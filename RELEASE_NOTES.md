# Coflow 0.6.1

## Highlights

- Editor writes now reuse immutable runtime generations, reload only affected sources, and rerun
  checks for invalidated records and their dependents while preserving full-check equivalence.
- Scalar edits project into the editor immediately. Queued writes coalesce safely, retain pending
  optimistic values across publications, and reject stale replay or rollback after external
  generation changes.
- Graph views retain layout when scalar data changes and rerun layout only when topology or
  container shape changes.

## Performance

- Equal scalar writes are detected before opening a provider transaction or advancing the project
  generation.
- Adjacent Excel field writes are planned and saved once per workbook.
- Schema sessions and immutable source batches are reused between compatible generations, and
  stable check diagnostics avoid unnecessary cloning.
- Editor-attributed source and generated-dimension writes no longer trigger a duplicate watcher
  reload.

## Reliability and Correctness

- Spread writes publish every affected file and invalidate checks for both the displayed host and
  persisted source records.
- Editor diagnostics are derived from the current generation across table, record, inspector, and
  graph views.
- Undo and redo shortcuts are routed through committed controls so mutation history remains
  serialized with pending writes.
- Excel temporary files are ignored by the watcher, and merged cells are loaded without treating
  covered cells as conflicting values.
- Runtime mutation batches preserve ordered field-write semantics, failure indexes, and
  transaction compensation.

## Compatibility

- No intentional breaking changes from 0.6.0.
