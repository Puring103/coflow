# CFD Examples

These files demonstrate CFD as a text data format for complex game configuration.
They are intentionally separate from the project pipeline because CFD sources are
currently exposed through the `coflow-cfd` crate rather than `coflow.yaml`.

- `schema.cft`: small schema used by all examples.
- `data/01-records.cfd`: basic records, same-type grouping, arrays, dictionaries,
  inline objects, and `&key` references.
- `data/02-polymorphic-and-paths.cfd`: polymorphic grouping and
  `@Type.key.path[index]` references.
- `data/03-spread.cfd`: object and dictionary `...` spread with local overrides.

The examples are loaded by `coflow-cfd` tests:

```powershell
cargo test -p coflow-cfd examples_cfd_files_load_together
```
