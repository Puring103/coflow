# CFT

CFT defines configuration with `table`, `data` and `singleton`, alongside enums, constants, aliases, defaults, dimensions and business validation. CFT files in a project share declarations.

```cft
enum Quality { Common, Rare = 10, Epic }
sealed data Stats {
  hp: int = 100;
  check PositiveHealth { hp > 0: "Health must be positive"; }
}
table Item {
  @localized
  name: string;
  stats: Stats;
  quality: Quality = Quality::Common;
  next: Option<&Item> = None;
  @localized
  label: string = f"{self.name}: {self.stats.hp}";
  check NameRequired { name.len() > 0: "Name cannot be empty"; }
}
singleton Settings { start: &Item; }
```

## Declarations and identity

| Declaration | Meaning |
| --- | --- |
| `table` | Multiple records with IDs, available as reference targets |
| `data` | Inline data objects with optional self-validation |
| `singleton` | Exactly one record, whose ID is the declaration name |
| `type Name = T;` | Type alias |

Use `&TableName` for record references and data names for inline values. References use `&Type::id` or `&id`. Singletons have a unique access point. Tables cannot be constructed inline; data objects cannot be top-level records.

A record's intrinsic identity is `id`; dictionary entries use `key`. Tables and singletons cannot declare an `id` field, while data types can. `@idAsEnum(Name)` generates an enum for record IDs.

Types use single inheritance: tables inherit tables, data types inherit data types, and singletons may inherit tables. Abstract types require concrete subtypes, sealed types cannot be inherited, and singletons are closed. Field names must be unique across an inheritance chain.

### Inheritance syntax

```cft
abstract table Entity { name: string; }
table Item: Entity { price: int = 0; }
singleton StartingItem: Entity { count: int = 1; }
abstract data Shape { color: string; }
sealed data Circle: Shape { radius: float; }
@struct sealed data Point { x: float; y: float; }
```

Place the base type after a colon. Inherited fields form part of the actual type. A singleton has exactly one same-named record and cannot be a field type or reference target. `&` targets tables; inline fields use data types.

## Types and omission

| Type | Meaning |
| --- | --- |
| `int` / `long` | Signed 32-bit / 64-bit integers |
| `float` / `double` | Finite binary32 / binary64 floating-point numbers |
| `bool` / `string` | Boolean / string |
| `&TableName` | Record reference |
| Data name | Inline object |
| `Option<T>` | `None` or `Some(T)`, nestable |
| `[T]` / `{K: V}` | Array / dictionary |
| Enum name | Named enum value |

Mixed numeric arithmetic promotes `int → long → float → double`. Literals are range-checked against their target type; float rounds to 32-bit precision. Declared numeric values cannot narrow implicitly. Without a target type, integer literals infer int within the 32-bit range and long otherwise; floating-point literals infer double.

```cft
data Metrics {
  count: int = 10;
  total: long = 4294967296;
  ratio: float = 0.1;
  precise: double = 0.1;
}
```

**Only fields with declared defaults may be omitted.** `Option<T>` permits an empty value; an Option field without a default must still be supplied.

```cft
data Note {
  required: Option<string>;         # Required; explicit None is allowed
  optional: Option<string> = None;  # May be omitted
  count: int = 1;                   # May be omitted
}
```

Each Option layer is independent: `None`, `Some(None)` and `Some(Some(3))` differ. A bare value in a known Option position supplies one `Some` layer.

```cft
const LIMIT = 100;
const EMPTY: Option<int> = None;
type ItemLink = Option<&Item>;
```

Constants infer unambiguous types; `None` and empty collections need explicit types. Alias and constant dependencies must be acyclic. Dictionary keys are int, long, string or enum.

## Strings and names

All strings use `string`. Ordinary double-quoted strings preserve braces. Only explicit `f"..."` strings interpolate field paths; doubled opening or closing braces represent literal braces.

```cft
data Label {
  name: string;
  literal: string = "{self.name}";
  title: string = f"Name: {self.name}";
}
```

Templates work in field values, defaults, array elements and dictionary values. Templates dependent on an owning object cannot be constants. Rule messages use fields and bindings from rule scope; see [business validation](#business-validation).

Syntax and literal keywords are reserved. Names such as `min`, `max`, `keys`, `values`, `records` and `return` may be business identifiers. Type declarations cannot replace built-in type names. `#` starts a line comment.

## Annotations and rules

| Annotation | Purpose |
| --- | --- |
| `@label("text")` / `@description("text")` | Display metadata for types, enums, members and fields |
| `@struct` | Generate a target struct for sealed data without a base type |
| `@flag` | Bit flags with automatic None and Everything |
| `@int64` | Use 64-bit long for an enum instead of default 32-bit int |
| `@idAsEnum(Name)` | Populate an already declared empty enum with table IDs |
| `@localized` / `@dimension("name")` | Bind table or singleton fields to dimensions |
| `@expand` | Expansion metadata for concrete data fields |

Enums default to 32-bit int; use `@int64` for 64 bits. Omitted member values increment. User member names and values must be unique. Out-of-range values are errors rather than automatic width upgrades.

```cft
enum Status { Pending = 0, Done = 1 }
@int64 enum LargeCode { Special = 4294967296 }
@flag enum Permission { Read = 1, Write = 2, Execute = 8 }
@flag @int64 enum LargePermission { Manage = 4294967296 }
```

Flags automatically provide `None = 0` and `Everything`, the bitwise OR of all actual flags, recomputed when members change. Here `Permission::Everything` is 11. These built-in names cannot be declared manually. Nonzero user members must be positive powers of two; 32-bit flags support 31 independent bits and 64-bit flags support 63. Combine same-type members with `&`, `^` and `|`. Ordinary enums do not add these members.

Data, table and singleton declarations may have multiple named or anonymous `check` blocks after their fields. Rules execute from parent to child, then in declaration order. Named rules must be unique in an inheritance chain. Top-level project rules may be anonymous and use `records(TypeName)` for collection and relationship checks.

## Business validation

Checks define read-only business rules. Data, tables and singletons can declare rules after their fields; project rules can be declared at top level.

```cft
data Reward {
  count: int;
  check PositiveCount: "Invalid reward count" {
    count > 0: f"Count {count} must be greater than 0";
  }
}
table Item {
  price: int;
  tags: [string];
  reward: Reward;
  check Price { price >= 0: "Price cannot be negative"; }
  check Tags { tags.isUnique(): "Tags must be unique"; }
}
```

Each rule needs at least one statement. Anonymous rules can repeat. Named rules must be unique within a type and its inheritance chain; unrelated types may reuse short names. Parent rules run first, then local rules in declaration order. Data rules automatically validate every inline instance, including objects inside arrays, dictionaries and Options.

```cft
table Sample {
  value: int;
  check { value >= 0; }
  check: "Value exceeds the limit" { value <= 100; }
}
check { records(Sample).len() > 0; }
```

Anonymous rules have identifiers derived from source file and position. The CLI can list, inspect and select them. Identifiers may change after source movement; name rules when stable identity is needed.

Assertions return bool and may provide an ordinary or `f"..."` message after a colon. Rules, `when` and quantifiers may also provide messages before their opening brace. Outer messages supplement the assertion's error and position.

### Conditions, collections and relationships

Top-level rules use `records(Type)` for table or singleton records. Reference fields allow direct target access, such as `reward.item.enabled`.

```cft
check AvailableItems {
  any item in records(Item): "At least one available item is required" {
    item.price > 0 && item.tags.contains("available");
  }
}
```

| Syntax | Requirement |
| --- | --- |
| `when condition { ... }` | Execute the body when the condition holds |
| `all item in collection { ... }` | Every element satisfies the body |
| `any item in collection { ... }` | At least one element satisfies the body |
| `none item in collection { ... }` | No element satisfies the body |

Empty collections satisfy all and none, but not any. Array dual bindings are `item, index` with zero-based indices; dictionary dual bindings are `key, value`. Bindings exist only inside the body; messages before the body use outer scope.

A false `when` condition skips its body and completes normally. To filter and test inside any/none, combine booleans such as `item.enabled && item.price > 0`.

`is Some(value)` unwraps one Option layer, `is None` tests emptiness, and `is TypeName` tests actual type. Expressions support field paths, indexing, arithmetic, comparison, logic, bit operations and methods such as `len`, `contains`, `isUnique`, `matches` and `sum`. Logical conditions short-circuit.

### Checking dimensions

Direct reads return base values. `for` selects a variant, `hasVariant` tests explicit coverage, and `variants` iterates effective values for all discovered variants in the dimension.

```cft
table Text {
  @localized name: string;
  check TranslationComplete {
    !name.isBlank(): "Base name cannot be blank";
    name.hasVariant("ja"): "Japanese name is required";
    all variant, text in name.variants(): "Incomplete translations" {
      name.hasVariant(variant): f"Missing {variant} name";
      !text.isBlank(): f"The {variant} name cannot be blank";
    }
    !name.for("ja").isBlank(): "Japanese name cannot be blank";
  }
}
```

Explicit `None`, empty strings and overrides equal to base values all count as present. Without an override, for evaluates the base expression and template dependencies in the selected variant. variants includes fallback values; hasVariant distinguishes explicit coverage. Use explicit rules such as `hasVariant("ja")` for variants not yet present in the project.

### Execution and diagnostics

```sh
coflow rule list
coflow check --rules
coflow check --rule Reward::PositiveCount --explain
```

Select type rules with `Type::RuleName` and top-level rules by name. `coflow check` validates declarations and CFD, `--rules` runs all business rules, `--rule` selects rules, and `--explain` provides explanations. `coflow build` generates code after all checks pass. The editor runs rules and displays diagnostics during editing.

Ordinary assertion failures continue collecting errors. Evaluation errors such as division by zero or invalid indices stop the current rule; other rules continue. Reports distinguish passed, not triggered, business failure, evaluation error, exhausted budget and not executed. They include rule names, CFT positions, CFD field positions and related elements or variants. Budget exhaustion means checking is incomplete. Default CLI output provides counts and bounded diagnostics; `--details` expands per-target rules and statements, while `--explain` adds actual values and explanations.

Data, statistics, reference and dimension queries help locate affected records. Generated C# types and Coflow.Loader load data; validate business rules through CLI or editor before delivery.

### Operators and built-in methods

Type rules access current-object fields directly; references may continue into target fields. Top-level `records(Type)` includes that type and its subtypes. Single dictionary bindings expose `.key` and `.value`; dual bindings use `key, value`.

| Operation | Syntax and behavior |
| --- | --- |
| Access | `stats.hp`, `items[0]`, `labels["name"]` |
| Arithmetic | `+`, `-`, `*`, `/`, `//`, `%`, `**`, unary `-` |
| Comparison | `==`, `!=`, `<`, `<=`, `>`, `>=`; chained comparisons must have consistent direction |
| Logic | `!`, `&&`, `\|\|`; the latter two short-circuit |
| Bits | `~`, `&`, `^`, `\|`, `<<`, `>>` for applicable integers or flags |
| Type tests | `value is TypeName`, `value is Some(item)`, `value is None` |
| Grouping | Parentheses control evaluation order |

A binding from `is Some(item)` is available to subsequent `&&` conditions and a successful `when` body. Successful actual-type tests enable subtype field access. Ordinary operands evaluate left to right. Integer overflow, division by zero, invalid shifts, out-of-bounds indices and missing dictionary keys produce evaluation diagnostics.

| Receiver | Methods | Meaning |
| --- | --- | --- |
| String | `len()`, `isBlank()` | Length, blankness |
| String | `contains(text)`, `startsWith(text)`, `endsWith(text)` | Substring, prefix, suffix |
| String | `matches("regex")` | Static regular expression; pattern must be a string literal |
| Array | `len()`, `contains(value)`, `isUnique()` | Length, membership, uniqueness |
| Array | `min()`, `max()`, `sum()` | Minimum, maximum, sum; extrema require nonempty collections |
| Array | `isSorted()`, `isStrictlySorted()` | Nondecreasing, strictly increasing |
| Dictionary | `len()`, `contains(key)`, `containsKey(key)`, `containsValue(value)` | Size, key or value membership |
| Dictionary | `keys()`, `values()` | Key and value collections |
| Scalar-element collections | `intersects(other)`, `isDisjoint(other)` | Intersection, disjointness |
| Scalar-element collections | `isSubsetOf(other)`, `isSupersetOf(other)` | Subset, superset |
| Number | `abs()` | Absolute value |
| Floating point | `isFinite()`, `approxEqual(other, tolerance)` | Finiteness, approximate equality within tolerance |
| Dimension field | `for(variant)`, `hasVariant(variant)`, `variants()` | Effective value, explicit coverage, discovered variants |

`//` is integer division, `%` integer remainder and `**` exponentiation. Precedence, highest first: field/index/call → unary `!` / `~` / `-` → `**` → `*` / `/` / `//` / `%` → `+` / `-` / `<<` / `>>` → `&` / `^` / `|` → comparisons → `is` → `&&` → `||`. Operators at the same level associate left to right, except exponentiation, which associates right to left. Parentheses override precedence.

Checks may construct enum values from int or long expressions with `EnumName(integer)`, for example `quality == Quality(10)`. Use `records(TypeName)` for record collections; other built-ins use the methods above.

Receiver, element and argument types are checked. Numeric aggregates retain declared types and check overflow. Collection comparisons and uniqueness use value equality.

## Data model and recursion

Records have a type and ID. References connect record identities and may form cycles. Inline data is represented by actual type and field contents. Arrays preserve order; dictionary keys are unique. Equality compares arrays element by element, dictionaries by keys and values, Options by every layer's state and contents, and references by identity.

Required inline objects must not create endless recursion. Use Options or collections to terminate trees:

```cft
data Node {
  name: string;
  children: [Node] = [];
}
```

Defaults must also expand finitely. `None`, empty collections or explicitly supplied terminal fields can end recursion. Aliases, constants and inheritance must be acyclic.

## Files and lexical rules

CFT uses UTF-8. Identifiers start with `_` or Unicode XID_Start and continue with `_` or XID_Continue. Names are case-sensitive without Unicode normalization. All project files share declaration names.

Fields end with semicolons, enum members use commas and `#` starts a line comment. Strings support `\n`, `\r`, `\t`, `\"` and `\\`; enter other Unicode characters directly.

Globally reserved names are `_`, `const`, `enum`, `type`, `table`, `data`, `singleton`, `abstract`, `sealed`, `check`, `when`, `all`, `any`, `none`, `in`, `is`, `true`, `false`, `None` and `Some`. Built-in type names have fixed meanings in type positions.

```text
Declaration: [annotations] [abstract | sealed] table|data|singleton Name [: Base] { fields… checks… }
Field:       [annotations] name: Type [= default];
Enum:        [annotations] enum Name { Member [= integer], … }
Alias:       type Name = Type;
Constant:    const Name [: Type] = value;
Check:       check [Name] [: message] { statements… }
Type:        int | long | float | double | bool | string | Name | &Name
           | Option<Type> | [Type] | {KeyType: ValueType} | (Type)
```

Interpolation reads field paths ending in a number, bool, enum or string. Select a readable scalar field before using objects, collections or Options. Templates inside data objects use that object as `self`. Ordinary defaults use value expressions; business computations and conditions belong in checks. See [CFD](./02-cfd.md) for value examples.
