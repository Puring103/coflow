# CFD

CFD supplies base records and dimension overrides for CFT declarations. Applications select a set of mutually referencing files and load them together.

```cfd
sword: Item {
  name: "Sword",
  stats: Stats { hp: 100 },
  quality: Quality::Rare,
  next: Some(&Item::shield),
}
Item {
  shield {
    name: "Shield",
    stats: { hp: 80 },
    next: None,
  },
}
language[ja] { sword: Item { name: "剣" } }
```

Standalone records use `id: Type { ... }`; groups use `Type { id { ... } }`. Explicit actual types within a group must belong to that group's inheritance domain. Record IDs are language identifiers, unique within the entire inheritance tree.

A singleton record's ID is its declaration name. Separate fields with commas; trailing commas are allowed. `#` starts a line comment. Duplicate IDs, duplicate fields, extra fields, type mismatches and unknown types produce diagnostics.

## Values

| Value | Examples |
| --- | --- |
| Integer, float, boolean | `2147483648`, `1e100`, `true` |
| Ordinary string | `"{literal}"` |
| Interpolated text | `f"Price: {self.price}"` |
| Object | `Stats { hp: 100 }` or `{ hp: 100 }` where the type is known |
| Reference | `&Item::sword` or `&sword` where the target type is known |
| Optional | `None`, `Some(3)`, `Some(None)` |
| Array, dictionary | `[1, 2]`, `{ "a": 1 }` |
| Enum, flags | `Quality::Rare`, `Read \| Write` |
| Constant | A global constant declared in CFT |

Numbers are validated against declared types: int/long are signed 32/64-bit integers and float/double are finite binary32/binary64 values. Float rounds to 32-bit precision. Options may nest, retaining every layer's state; a bare value in an optional field supplies one Some layer. Omitted fields use explicit defaults only. Missing fields without defaults are errors, including Option fields.

References link after all record identities have been established, allowing cross-file links and cycles. Braces in ordinary strings are literal. Templates render field paths during loading; the terminal value must be a primitive convertible to text.

## Overrides

`dimension[variant]` overrides dimension fields on existing records, replacing only explicitly supplied complete field values. Other fields use base values; explicit None remains empty. Duplicate assignments to the same dimension, variant, record and field are errors. File order does not establish priority.

See [localization and dimensions](../10-localization.md) for selecting and reading variants.

## Record types and identity

| CFT declaration | CFD syntax | Identity and access |
| --- | --- | --- |
| `table Item` | `sword: Item { ... }` | Multiple records, queried or referenced by ID |
| `singleton Settings` | `Settings: Settings { ... }` | Exactly one record with the type name as ID |
| `data Stats` | Field value `Stats { ... }` | Inline object accessed through its owning field |

Standalone records and groups can coexist. Groups may specify derived types:

```cfd
Item {
  sword { name: "Iron sword", price: 100 },
  bow: RangedItem { name: "Longbow", price: 120, range: 10 },
}
Settings: Settings { start: &Item::sword }
```

This requires Item, a derived RangedItem and Settings with `start: &Item` in CFT. Abstract types may be group types; actual records must have concrete, instantiable types. IDs are unique per inheritance tree; unrelated trees may reuse IDs.

## Numbers, collections and objects

| Declared type | Range or constraint |
| --- | --- |
| `int` | -2147483648 to 2147483647 |
| `long` | -9223372036854775808 to 9223372036854775807 |
| `float` | Finite 32-bit floating point, rounded to binary32 precision |
| `double` | Finite 64-bit floating point |
| `bool` | `true` or `false` |
| `string` | Ordinary string or explicit interpolation template |
| `[T]` | Elements validated as T, ordered, may be empty |
| `{K: V}` | K is int, long, string or enum; unique keys |

Numbers support decimal and exponent forms such as `1.5` and `-2.5e-3`, checked against the target range. Enums use member names or `Enum::Member`. Flags combine same-type members with `&`, `^`, `|` and parentheses; precedence is `&`, then `^`, then `|`.

```cfd
example: Example {
  scores: [10, 20, 30],
  labels: { "title": "Item", "action": "Buy" },
  stats: Stats { hp: 100 },
  permissions: Permission::Read | Permission::Write,
}
```

Objects may use the declared type or a concrete subtype. Omit `Stats` when the declared type determines the object type; specify the actual type to instantiate a subtype. Required-field and default rules also apply inside objects.

## Defaults and optional values

| Field declaration | May omit? | CFD values |
| --- | --- | --- |
| `count: int;` | No | For example `3` |
| `count: int = 0;` | Yes, defaults to 0 | For example `3` |
| `note: Option<string>;` | No | `None`, `Some("note")` or `"note"` |
| `note: Option<string> = None;` | Yes, defaults to None | Same as above |
| `nested: Option<Option<int>>;` | No | `None`, `Some(None)`, `Some(Some(3))` |

Omission, explicit None and empty strings differ. Defaults are completed recursively inside objects, collections and Options. Each Option layer retains its state. Only a declared default makes a field omissible.

## References and the data model

`&Item::sword` locates a record by type and ID; `&sword` uses the field's target type. The target must exist and satisfy the reference type. Base-type references can point to subtype records.

Record identities are created before linking, so files may reference each other and references may form cycles. Load all dependencies together. References share record identity; inline data belongs to its owning object's field value.

Arrays compare in order, dictionaries by keys and values, inline objects by actual type and fields, and Options by every layer's state and contents. References compare identities without expanding targets.

## Strings and templates

Ordinary strings preserve braces; only `f"..."` interpolates. <code v-pre>{{</code> and <code v-pre>}}</code> represent literal braces within templates.

```cfd
sword: Item {
  name: "Iron sword",
  price: 100,
  label: f"{self.name}: {self.price}",
}
shield: Item {
  name: "Shield",
  price: 80,
  label: f"Pair with {&Item::sword.name}",
}
```

Item must declare `label: string`. `self` denotes the owning object; templates inside nested data use that inline object. Paths must end in string, int, long, float, double, bool or enum. Objects, collections and Options cannot be converted directly to text. Calculations and method calls belong in CFT checks.

Templates read dependencies in the selected dimension context. Missing overrides reevaluate the base template. Invalid paths, read errors and dependency cycles produce source diagnostics. Applications read the evaluated string.

## Dimension blocks and replacement scope

```cfd
language[ja] {
  sword: Item { name: "剣" },
}
```

The target record must already exist and name must be bound to language in CFT. Supply only top-level fields to override; other fields use base expressions. Object, array and dictionary overrides replace the entire field rather than merge elements. Defaults are still completed inside overridden inline objects.

Even an empty override block declares a variant. Blocks across all inputs establish known variants. A field cannot be assigned twice in one variant. See [localization and dimensions](../10-localization.md) for file organization and reading.

## File syntax

CFD uses UTF-8. Identifiers follow Unicode XID: `_` or XID_Start initially, then `_` or XID_Continue, case-sensitive. See [CFT lexical rules](./01-cft.md#files-and-lexical-rules) for reserved names. Strings support `\n`, `\r`, `\t`, `\"` and `\\`; enter Unicode directly.

```text
Record:     ID: Type { field: value, … }
Group:      Type { ID [: ActualType] { field: value, … }, … }
Override:   dimension[variant] { records or groups… }
Object:     [ActualType] { field: value, … }
Array:      [value, …]
Dictionary: {key: value, …}
Optional:   None | Some(value)
Reference:  &ID | &Type::ID
```

Separate fields, collection elements and dictionary entries with commas; trailing commas are allowed. `#` starts a line comment. Use `coflow format` for formatting, `coflow check` for declarations, values and references, or `coflow check --rules` to include business rules. See [diagnostics](../09-diagnostics/01-diagnostics.md) for error structures and codes.
