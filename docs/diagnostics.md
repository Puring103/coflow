# Coflow Diagnostics

Coflow diagnostics are structured errors produced by project config validation,
CFT compilation, Excel loading, data model construction, reference resolution,
`check {}` execution, artifact preflight, and codegen preflight.

Diagnostics are different from unrecoverable CLI errors. Diagnostics describe
problems in project inputs that Coflow can report in a structured form. CLI
errors describe failures where Coflow cannot reliably continue, such as being
unable to read or parse `coflow.yaml`.

## Human Output

Human diagnostics are written to stderr. Each diagnostic is separated by a line:

```text
----------------------------------------
[CODE] [STAGE]
file    path/to/file.cft
line    12
column  5
message
  description of the problem
```

Excel diagnostics may show `sheet` and `cell` instead of only line/column:

```text
----------------------------------------
[CELL-TypeMismatch] [CELL]
file    data/items.xlsx
sheet   Item
cell    B2
message
  failed to parse `Item.level` cell: expected int
```

Project-level diagnostics do not always have a source file location. Those use
line `1`, column `1` as a fallback position in human output.

## JSON Output

`coflow check --json` and `coflow cft check --json` emit:

```json
{
  "diagnostics": [
    {
      "code": "CFT-SCHEMA-006",
      "stage": "SCHEMA",
      "severity": "error",
      "message": "unknown type `Missing`",
      "path": "schema/main.cft",
      "startLine": 0,
      "startCharacter": 14,
      "endLine": 0,
      "endCharacter": 21,
      "related": []
    }
  ]
}
```

All positions are zero-based in JSON. Human output displays one-based line and
column numbers.

Diagnostic fields:

| Field | Meaning |
| --- | --- |
| `code` | Stable diagnostic code or code family identifier. |
| `stage` | Pipeline stage that produced the diagnostic. |
| `severity` | Currently always `error`. |
| `message` | Human-readable explanation. |
| `path` | File path when a file location is known. |
| `sheet` | Excel worksheet name, when applicable. |
| `cell` | Excel A1 cell, when applicable. |
| `startLine`, `startCharacter`, `endLine`, `endCharacter` | Zero-based source range. |
| `related` | Additional locations, for example duplicate declarations or duplicate IDs. |

`related` entries have the same location fields and may include `label`.

## Diagnostic Families

### `CLI-ERROR`

Stage: `CLI`

Unrecoverable command-level errors. These are not normal structured project
diagnostics and are not aggregated.

Examples:

- config path does not exist;
- `coflow.yaml` cannot be read;
- YAML syntax cannot be parsed;
- artifact write fails after preflight, such as permission errors;
- malformed lockfile when codegen preflight is otherwise clean.

### `PROJECT-001`

Stage: `PROJECT`

Project config and command preflight diagnostics after `coflow.yaml` has been
successfully read and parsed.

Examples:

- schema path is empty or missing;
- schema list is empty;
- source workbook path is empty or missing;
- source sheet list is empty;
- sheet name is empty;
- sheet type override is empty;
- output type is unsupported;
- output directory is empty;
- `outputs.data.namespace` is set;
- `outputs.code.namespace` is empty;
- command-specific output config is missing or incompatible.

Project diagnostics are aggregated where possible. For example, a single
`coflow check --json` run can report multiple schema path, source, and output
configuration problems.

### `CFT-LEX-*`

Stage: `LEX`

Lexical errors in `.cft` files, such as invalid characters, invalid string
escapes, unterminated strings, and invalid numeric literals.

Lex/syntax recovery inside a single `.cft` file is intentionally limited: a
single file may report the first parse-blocking error. Other schema files still
continue to be processed when possible.

### `CFT-SYN-*`

Stage: `SYN`

Syntax errors in `.cft` files, such as unexpected tokens, unexpected EOF,
invalid top-level items, malformed annotations, invalid check statements, or
duplicate check blocks.

Like lexical diagnostics, syntax diagnostics may stop within the current file
because a reliable AST is not available after a parse-blocking error. Other
schema files can still contribute diagnostics.

### `CFT-SCHEMA-*`

Stage: `SCHEMA`

Schema compilation diagnostics after parsing succeeds. These are broadly
aggregated.

Examples:

- duplicate module, type, field, enum variant, or enum value;
- unknown named type;
- invalid inheritance;
- invalid `@struct`, `@expand`, `@flag`, `@display`, `@deprecated`, or `@keyAsEnum` usage;
- legacy field-level `@id`, `@index`, `@ref`, `@IdAsEnum`, and `@GenAsEnum`
  annotations have been removed; use the field's CFT type and `@Type.key` or `&key`
  cell references instead;
- reserved `id` field declarations;
- invalid default value;
- invalid enum value sequence;
- reference target has no ID;
- reference ID type mismatch.

### `CFT-TYPE-*`

Stage: `TYPE`

Type-checking diagnostics for CFT `check {}` expressions.

Examples:

- unknown value or field;
- invalid enum variant use;
- operator or comparison type mismatch;
- non-bool condition;
- unknown function;
- wrong function arity or argument type;
- invalid indexing;
- invalid `is` predicate;
- unsupported `unique` element type;
- invalid regex pattern.

### `EXCEL-OPEN`

Stage: `EXCEL`

Coflow could not open a workbook.

Typical causes:

- file is not a valid Excel workbook;
- file is locked or unreadable;
- path points to a non-workbook file.

Missing source files are normally reported earlier as `PROJECT-001`.

### `EXCEL-SHEET`

Stage: `EXCEL`

Workbook sheet-level diagnostics.

Examples:

- configured sheet is missing;
- sheet cannot be read;
- sheet is empty.

Coflow continues with other workbooks and other sheets when possible.

### `EXCEL-TYPE`

Stage: `EXCEL`

A sheet maps to an unknown CFT type. This can come from an explicit sheet
`type`, or from the sheet name when `type` is omitted.

### `EXCEL-COLUMN`

Stage: `EXCEL`

Header mapping diagnostics.

Examples:

- header maps to an unknown field;
- two columns map to the same CFT field;
- `@expand` does not have enough adjacent columns for all child fields.

If a sheet has header diagnostics, Coflow skips data row parsing for that sheet
because row values cannot be mapped reliably. Other sheets continue.

### `EXCEL-CELL`

Stage: `EXCEL`

Unsupported raw Excel cell values.

Examples:

- Excel error cells;
- native Excel date/time cells;
- ISO duration/date cells that are represented as typed Excel values instead of
  plain text.

Use text cells for values that should be parsed by Coflow's schema-guided cell
parser.

### `CELL-*`

Stage: `CELL`

Schema-guided cell parser diagnostics. The exact suffix comes from the cell
value parser, for example `CELL-TypeMismatch`.

Examples:

- `not_int` in an `int` field;
- malformed arrays, dictionaries, or objects;
- invalid enum values;
- invalid string escaping;
- invalid polymorphic object type marker.

Cell diagnostics are collected across the whole sheet when the header is valid.

### `CFD-DATA-*`

Stage: `DATA`

Data model construction diagnostics after Excel cells have been parsed.

Codes include:

| Code | Meaning |
| --- | --- |
| `CFD-DATA-001` | unknown record or object type |
| `CFD-DATA-002` | abstract record type used directly |
| `CFD-DATA-003` | missing polymorphic object type |
| `CFD-DATA-004` | object actual type is not assignable |
| `CFD-DATA-005` | unknown field |
| `CFD-DATA-006` | missing required field |
| `CFD-DATA-007` | value type mismatch |
| `CFD-DATA-008` | invalid enum variant |
| `CFD-DATA-009` | duplicate dictionary key |
| `CFD-DATA-010` | missing ID field |
| `CFD-DATA-011` | duplicate ID |
| `CFD-DATA-012` | duplicate ID in polymorphic range |
| `CFD-DATA-013` | invalid record key identifier |

Data model diagnostics are aggregated within the data model stage. Reference
resolution and checks do not run if the data model is invalid.

### `CFD-REF-*`

Stage: `REFERENCE`

Reference resolution diagnostics.

| Code | Meaning |
| --- | --- |
| `CFD-REF-001` | reference target type has no ID |
| `CFD-REF-002` | referenced target record was not found |

Checks do not run if reference resolution fails.

### `CFD-CHECK-*`

Stage: `CHECK`

Runtime `check {}` diagnostics.

| Code | Meaning |
| --- | --- |
| `CFD-CHECK-001` | check condition evaluated to false |
| `CFD-CHECK-002` | runtime type error during check evaluation |
| `CFD-CHECK-003` | null access |
| `CFD-CHECK-004` | index out of bounds |
| `CFD-CHECK-005` | missing dictionary key |
| `CFD-CHECK-006` | `min` / `max` over no non-null values |
| `CFD-CHECK-007` | invalid runtime regex |

A hard runtime error stops the current check block, but Coflow continues with
later check blocks on the same record, other records, and nested object checks.
False conditions, `any` / `none` failures, and failures across multiple records
are aggregated.

### `CODEGEN-CSHARP-001`

Stage: `CODEGEN`

C# codegen preflight diagnostics before any generated files are changed.

Examples:

- invalid namespace;
- invalid C# type, enum, enum variant, member, or database class name;
- generated file name collision;
- generated member name collision;
- configured database file name collision;
- invalid `@keyAsEnum` generated variant;
- duplicate `@keyAsEnum` variant values.

When these diagnostics are present, Coflow does not read or write the enum
lockfile, does not clean stale `.cs` files, and does not generate new `.cs`
files.

### `ARTIFACT-001`

Stage: `ARTIFACT`

Artifact preflight diagnostics. Currently this reports output paths that already
exist and are not directories.

This is a non-writing preflight. Coflow does not create directories or test
permissions during this check. Permission errors and true write failures remain
runtime `CLI-ERROR`s.

## Non-Blocking Collection Rules

"Non-blocking" means Coflow continues collecting diagnostics when it can do so
without relying on invalid intermediate data. It does not mean Coflow generates
artifacts with invalid inputs.

| Stage | Continues collecting? | Why it may stop |
| --- | --- | --- |
| Config file discovery/read/YAML parse | no | no valid project config exists |
| Parsed project config validation | yes | independent config fields can be checked together |
| Schema path discovery | yes | multiple path errors can be reported together |
| Single CFT file lex/syntax | limited | no reliable AST after parse-blocking errors |
| Multiple CFT files | yes | other files can still be parsed |
| Schema/type compilation | yes | compiler can aggregate semantic diagnostics |
| Excel workbook/sheet discovery | yes | other workbooks/sheets can still be processed |
| Excel header mapping | yes within header; sheet rows skipped on header errors | rows cannot be mapped reliably |
| Excel cell parsing | yes | valid headers allow independent cell parsing |
| Data model input validation | yes within substage | invalid model blocks references and checks |
| Reference resolution | yes within substage | unresolved refs block checks |
| Check runtime | yes across blocks/records | hard errors stop only current block |
| Codegen preflight | yes | independent naming checks can be aggregated |
| Artifact preflight | yes | output path checks are independent |
| Artifact writes | no | partial write failures are operational errors |

## Artifact Safety

Commands that write artifacts are gated:

- `build` writes nothing if project, schema, Excel, data model, reference, check,
  codegen preflight, or artifact preflight diagnostics are present.
- `export` writes nothing if diagnostics are present before export.
- `codegen` writes nothing if schema, project, codegen preflight, or artifact
  diagnostics are present.

Actual filesystem write failures after preflight are reported as `CLI-ERROR`
because Coflow cannot reliably continue or aggregate those failures.

## Reading Diagnostics

Recommended order:

1. Fix `CLI-ERROR` first; the command could not reach structured validation.
2. Fix `PROJECT-001`; config problems can prevent later stages.
3. Fix `CFT-*`; schema errors block Excel and artifact stages.
4. Fix `EXCEL-*` and `CELL-*`; these block data model construction.
5. Fix `CFD-DATA-*` and `CFD-REF-*`; these block checks and exports.
6. Fix `CFD-CHECK-*`; data is structurally valid but violates rules.
7. Fix `CODEGEN-*` and `ARTIFACT-*`; data may be valid but artifacts cannot be
   generated safely.

Use JSON output when integrating with editors, CI systems, or custom tooling.
