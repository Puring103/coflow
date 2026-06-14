# CFT Examples

These files demonstrate core CFT syntax and are designed to compile together as one global CFT namespace.

- `01_constants.cft`: constants and literal defaults.
- `02_enums_and_flags.cft`: plain enums and `@flag` enums.
- `03_types_fields_defaults.cft`: type declarations, fields, annotations, virtual `id`, and defaults.
- `04_arrays_and_dicts.cft`: arrays, dictionaries, and dict entry checks.
- `05_inheritance.cft`: abstract types, inheritance, and sealed types.
- `06_nullable_and_ref.cft`: nullable fields and record-key references.
- `07_check_expressions.cft`: operators and chained comparisons.
- `08_when_and_quantifiers.cft`: `when`, `all`, `any`, and `none`.
- `09_builtin_functions.cft`: built-in check functions.
- `10_comprehensive_schema.cft`: a larger schema that combines several features.
- `11_unicode_identifiers.cft`: Unicode identifiers such as Chinese type, enum, and field names.

Validate all examples:

```powershell
cargo run --quiet -p coflow -- cft check examples/cft
```
