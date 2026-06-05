# RPG Coflow Example

This is a small project-style Coflow configuration for a typical RPG data set.

It includes:

- CFT schema in `schema/rpg.cft`.
- Excel data in `data/rpg.xlsx`.
- Project configuration in `coflow.yaml`.
- Declared JSON data and C# code outputs as placeholders.

Run schema-only validation:

```powershell
cargo run --quiet -p coflow -- cft check examples/rpg
```

Run the full project validation pipeline:

```powershell
cargo run --quiet -p coflow -- check examples/rpg
```

The export and codegen commands currently validate the declared output type and directory, then report that the implementation is still pending:

```powershell
cargo run --quiet -p coflow -- export json examples/rpg
cargo run --quiet -p coflow -- codegen csharp examples/rpg
```
