# Coflow Showcase

This is the repository's only user-facing example project. Each numbered schema
file introduces one feature, and the data file with the same number shows its CFD
representation.

| Files | Feature |
| --- | --- |
| `01-records` | Records and scalar fields |
| `02-defaults` | Field defaults |
| `03-enums` | Plain enums |
| `04-flags` | Flag enums |
| `05-arrays` | Arrays |
| `06-dictionaries` | Dictionaries |
| `07-inheritance` | Abstract types and polymorphic objects |
| `08-references` | Record references |
| `09-options` | Optional values |
| `10-checks` | Validation expressions |
| `11-conditional-checks` | Conditional validation |
| `12-quantifiers` | Collection quantifiers |
| `14-dimensions` | Localized fields and language overlays |

Run the example from the repository root:

```powershell
coflow schema check --project examples/showcase
coflow check --rules --project examples/showcase
coflow codegen --project examples/showcase
```

`schema check` validates declarations, `check --rules` validates data and business
rules, and `codegen` generates C# source. Use `coflow build --project examples/showcase`
to check declarations, data and all business rules before generating code in one step.
