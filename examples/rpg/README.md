# RPG Coflow Example

This is a project-style Coflow configuration for a larger RPG data set. It is
intended to exercise the full CFT surface in one coherent game-config example.

It includes:

- CFT schema split across `schema/*.cft`.
- Excel data in `data/rpg.xlsx`.
- CFD text data in `data/cfd/*.cfd`.
- Project configuration in `coflow.yaml`.
- Declared JSON data and C# code outputs.
- A workbook builder script in `scripts/build-rpg-workbook.mjs`.

The data set contains a validation-heavy RPG slice:

- `Item`, `Equipment`, `Skill`, `Buff`, `Monster`, and `Text` records in Excel.
- `DropTable`, `Stage`, `Quest`, and `Shop` records in CFD, where nested rewards,
  spawns, objectives, dialogue, and shop entries are easier to review as text.
- Field-level checks for IDs, ranges, names, prices, percentages, coordinates, and stat caps.
- Collection checks for unique arrays, dictionary key/value rules, drop weight sums, and matched
  reward/weight lengths.
- Conditional checks for active/passive skills, timed/passive buffs, raid stages, raid quests, and
  raid-gated shops.
- Cross-table checks for equipment text keys, skill buffs, monster skills/items, stage drops,
  quest stages/targets, shop quest gates, and localized text references.
- Bidirectional Excel/CFD references: Excel item/equipment rows reference CFD stages, while
  CFD progression records reference Excel items, equipment, monsters, skills, buffs, and text.

The schema demonstrates:

- typed constants and literal defaults;
- plain enums and `@flag` enums;
- `@struct`, `@idAsEnum`, `@singleton`, and `@localized`;
- sealed structs, abstract base types, multi-level inheritance, and polymorphic values;
- record-key based references, nullable references, self/forward references, arrays, dictionaries, and nested objects;
- `check` expressions with chained comparisons, arithmetic, bitwise and shift operators;
- `when`, `all`, `any`, `none`, field/index access, `is`, and built-in functions such as
  `len`, `contains`, `isUnique`, `min`, `max`, `sum`, `keys`, `values`, and `matches`.

Run schema-only validation:

```powershell
cargo run --quiet -p coflow -- cft check examples/rpg
```

Run the full project validation pipeline:

```powershell
cargo run --quiet -p coflow -- check examples/rpg
```

Export JSON data and generate C# runtime loading code:

```powershell
cargo run --quiet -p coflow -- export json examples/rpg
cargo run --quiet -p coflow -- codegen csharp examples/rpg
```

Regenerate the sample workbook after changing the table data:

```powershell
node examples/rpg/scripts/build-rpg-workbook.mjs
```

The script expects `@oai/artifact-tool` to be available through local Node module
resolution.
