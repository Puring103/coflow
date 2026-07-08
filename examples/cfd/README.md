# CFD Examples

These files demonstrate CFD as a text data format for complex game configuration.
They can be loaded through the example `coflow.yaml` project or directly by the
CFD loader tests.

- `schema.cft`: small schema used by all examples.
- `data/01-records.cfd`: basic records, same-type grouping, arrays, dictionaries,
  inline objects, and `&key` references.
- `data/02-polymorphic-and-paths.cfd`: polymorphic grouping and
  key-only `&key` references.
- `data/03-spread.cfd`: object and dictionary `...` spread with local overrides.

The examples are loaded by `coflow-loader-cfd` tests:

```powershell
cargo test -p coflow-loader-cfd examples_cfd_files_load_together
```
