# CFD Examples

These files demonstrate CFD as a text data format for complex game configuration.
They can be loaded through the example `coflow.yaml` project or directly by the
CFD loader tests.

- `schema.cft`: small schema used by all examples. It also demonstrates editor-facing
  `@label` / `@description` metadata on table fields, nested object fields, enum
  variants, reference fields, and enum-keyed dictionaries. Open this project in the
  CFD editor to see labels while writes continue to use the stable schema names.
- `data/01-records.cfd`: basic records, same-type grouping, arrays, dictionaries,
  inline objects, and `&key` references.
- `data/02-polymorphic-and-paths.cfd`: polymorphic grouping and
  key-only `&key` references.
- `data/03-spread.cfd`: object and dictionary `...` spread with local overrides.
- `data/04-chemical-equations.cfd`: a `ChemicalEquation` record for the CFD
  editor's chemical-equation reading-plugin example.
- `data/05-formatted-strings.cfd`: an ordinary string that references fields on
  another record and mixes HTML and Unity rich-text tags.

The examples are loaded by `coflow-loader-cfd` tests:

```powershell
cargo test -p coflow-loader-cfd examples_cfd_files_load_together
```
