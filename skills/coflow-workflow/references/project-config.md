# CFD-only project configuration

```yaml
schema: schema/
data: data/
codegen:
  - language: csharp
    dir: generated/csharp
```

`schema` discovers `.cft`, `data` discovers `.cfd`, and `codegen` declares one or more target-language outputs. Unknown fields are errors.
