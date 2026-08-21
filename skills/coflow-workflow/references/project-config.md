# CFD-only project configuration

```yaml
schema: schema/
data: data/
codegen:
  - language: csharp
    dir: generated/csharp
```

`schema` discovers `.cft`, `data` discovers `.cfd`, and `codegen` is the only output section. Unknown fields are errors; there is no provider selection or export target.
