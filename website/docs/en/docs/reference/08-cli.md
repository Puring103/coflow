# CLI commands

The CLI supports queries, editing, validation, diff previews and code generation. Project commands accept `--project <directory or coflow.yaml>`; otherwise discovery searches the current directory and parents. Project file paths are relative to the project root; request file paths are relative to the calling directory.

## Queries and context

```powershell
coflow describe --json
coflow describe data.set --json
coflow describe apply --schema input --json
coflow project inspect --json
coflow project files --kind schema --json
coflow source read --file schema/main.cft --json
coflow schema inspect --type Item --related --json
coflow schema symbols --kind field --json
coflow schema refs --symbol Item.price --json
coflow context --type Item --examples 3 --json
coflow data list --type Item --fields name,price --limit 20 --json
coflow data get --type Item --id sword --view both --json
coflow data search --text healing --json
coflow data refs --type Item --id sword --direction incoming --json
coflow rule list --type Item --json
coflow rule inspect --rule ItemPrices --json
```

`describe` returns a concise command catalog. `describe data.set` returns parameters, defaults, constraints, input formats and examples. For JSON Schema, add `--schema input|output|both` to a specific command; definitions appear once under `result.definition`.

`schema inspect` returns declarations, defaults, inheritance and positions. Field queries include enum, reference and concrete object candidates. `context` combines declarations, rules, sample records and reference candidates. Bound it with `--fields`, `--examples`, `--ref-limit`, `--depth` and `--budget`. Budget counts serialized result characters; omitted content is reported explicitly.

`data get --view source|effective|both` returns explicit assignments, effective values or both. `origin` distinguishes `explicit_base`, `schema_default` and `dimension_override`. Explicit None retains its assignment origin; omitting a field with `= None` uses the schema default. Paths crossing None return `origin: "unreachable"` and `reachable: false`, without source position or expression. Template expressions and rendering metadata are separate.

Lists share `--type`, `--include-derived`, `--file`, `--id`, `--where`, `--sort`, `--dimension name=variant`, `--limit` and `--cursor`. Default page size is 50; results contain `items`, `total`, `truncated` and `next_cursor`. Cursors bind query conditions and response `revision`. Only `--limit` may change while paging. A stale cursor returns conflict after project changes; restart without it. Aggregates cover all matching records; pagination limits output rows.

`--where` supports comparisons, boolean combinations and `contains`, `is_some`, `is_none`, `is_reachable`. `--sort` accepts comma-separated paths, with `-` for descending order. Text paths support `field.child`, `items[0]` and `labels["a.b"]`.

## Edits and previews

```powershell
coflow data draft --type Item --id new_sword --json
coflow data create --type Item --id new_sword --file data/items.cfd --input fields.json
coflow data set --type Item --id sword --path price --value-cfd 100 --expected-cfd 90 --rules
coflow data unset --type Item --id sword --path description
coflow data rename --type Item --id sword --new-id iron_sword
coflow data transfer --type Item --id sword --to-file data/weapons.cfd
coflow data delete --type Item --id obsolete --dry-run --json
coflow source write --file schema/main.cft --input candidates/main.cft --dry-run --json
coflow source create --file data/new.cfd --input candidates/new.cfd
coflow source delete --file data/obsolete.cfd --dry-run
coflow project add-input --kind data --path data/localization
coflow project remove-input --kind data --path data/localization --dry-run
coflow apply --input changes.json --rules --dry-run --json
coflow apply --input changes.json --rules --json
```

Drafts distinguish declared defaults, type defaults and required inputs, reporting `complete` and `required_inputs`. `set` creates an explicit assignment; `unset` removes it and reapplies defaults or missing-field checks. An unchanged explicit assignment preserves source and timestamps. Renaming updates recognized references, dimension targets and stable enum mappings. Transfers preserve override source files.

Writes build a candidate project, validate structure and data, then publish together. `--rules` adds business validation. For AI batches use `apply --rules --json`, with `--dry-run` for review. A successful result already includes selected validation; repeating the same check immediately is unnecessary.

Queries return `revision`. All project writes and candidate previews accept `--if-revision <queried revision>`. An apply request may also set top-level `if_revision`; both must agree if supplied. Changed inputs return exit code 3 and current revision; reread and adapt the plan. `data set --expected-cfd` and `set_field.expected` also compare the current base effective value after typed parsing and default completion. Templates compare their f-source expression. File replacement or deletion can guard content with `expected_fingerprint`.

Write results report operation, match and change counts, files and automatic updates. Default output lists failed operations; `operation_counts` summarizes successful and unexecuted operations. `--details` expands all operations and semantic changes. Preview `files` retain source patches; independent `diff` also shows semantics. Request full source with `diff --include-source`; `--details` does not imply full source. Failures retain actual publication metadata: `written`, `written_files`, `restored_files`, `restoration_failed` and failed operation index.

`apply` supports `kind: files` with `changes` for source/input edits, or `kind: operations` with `ops` for record, collection and dimension mutations. Operations execute sequentially against the current candidate; references are checked after all operations. Array insert/remove/move and dictionary set/remove use operations mode. Query `coflow describe apply --json` or `coflow describe apply --schema input --json` for request definitions.

```json
{
  "protocol_version": 1,
  "kind": "operations",
  "ops": [{
    "op": "set_field",
    "record": {"type": "Item", "id": "sword"},
    "path": [{"kind": "field", "name": "price"}],
    "value": {"kind": "expression", "source": "100"},
    "expected": {"kind": "expression", "source": "90"}
  }]
}
```

Values use CFD expressions or `{"kind":"model","value":...}`. Typed model values preserve decimal int64 strings, finite floats, every some/none layer, enum identity and number, reference identity, actual object types, collections and explicit template source. Enum `variant` is a single named member; combined values have no single member name. Query model values may be reused for writes; rendering and provenance are read-only. `--value-file` reads an expression; `-` means stdin. At most one carrier may read stdin per invocation.

## Project discovery and recovery

Create projects with `coflow init <directory>`. For existing projects inspect configuration and inputs before discovering types:

```sh
coflow project inspect --project . --json
coflow project files --project . --json
coflow schema symbols --project . --kind type --json
coflow context --project . --type Item --json
```

`context --type` requires an existing type. Write CFT before querying new types; use symbols when names are unknown. With configuration, schema or data errors, query the available layer. `project inspect/files` returns known information where possible. Even if model queries fail, read selected source without first repairing all data:

```sh
coflow source read --project . --file schema/items.cft --json
coflow source read --project . --file data/items.cfd --json
```

Fix schema declarations or configuration first. For unstructured CFD, repair source at diagnostic positions. Submit interdependent repairs together in a files changeset. `check --input changes.json --rules` validates only; `apply --input changes.json --rules` validates and writes. Invalid candidates leave files unchanged; revise them using returned diagnostics.

## Batch changes and request structures

Both modes use `protocol_version: 1`; select one mode per request. `record` uses actual type and ID. Structured path segments are `field`, `index` or `dict_key`; array indices start at zero. Values use `{"kind":"expression","source":"CFD expression"}` or typed model values.

### Record, collection and dimension operations

All supported `kind: operations` operations are listed below. Optional fields may be omitted. value, key and expected use the value inputs above.

| op | Fields | Behavior |
| --- | --- | --- |
| `create_record` | record, file; optional fields | Create a record, with values keyed by field name |
| `set_field` | record, path, value; optional expected | Assign a field; expected checks current base effective value |
| `unset_field` | record, path | Remove assignment and reapply default/required checks |
| `rename_record` | record, new_id | Update ID, recognized references and overrides |
| `delete_record` | record | Delete record and overrides; remaining references must be valid |
| `transfer_record` | record, to_file | Move base record, retaining override origins |
| `set_dimension_value` | record, dimension, variant, field, value; optional path, file | Set override; path is relative to the top-level field, still saved as a whole-field override |
| `clear_dimension_value` | record, dimension, variant, field | Remove explicit override |
| `array_insert` | record, path, index, value | Insert an array element |
| `array_remove` | record, path, index | Remove an array element |
| `array_move` | record, path, from, to | Move an array element |
| `dict_set` | record, path, key, value | Insert or update a dictionary entry |
| `dict_remove` | record, path, key | Remove a dictionary entry |
| `set_where` | selector, path, value, expected_count | Assign all matches; count must equal expectation |

Example adding a tag and setting a dictionary entry (both fields must be declared in CFT):

```json
{
  "protocol_version": 1,
  "kind": "operations",
  "ops": [
    {
      "op": "array_insert",
      "record": {"type": "Item", "id": "sword"},
      "path": [{"kind": "field", "name": "tags"}],
      "index": 0,
      "value": {"kind": "expression", "source": "\"weapon\""}
    },
    {
      "op": "dict_set",
      "record": {"type": "Item", "id": "sword"},
      "path": [{"kind": "field", "name": "labels"}],
      "key": {"kind": "expression", "source": "\"category\""},
      "value": {"kind": "expression", "source": "\"equipment\""}
    }
  ]
}
```

### Conditional edits and scope guards

Verify targets and total with `data list --type Item --where 'price < 100' --fields name,price --json`, then submit identical conditions. JSON selectors use `filter`, while command lines use `--where`:

```json
{
  "protocol_version": 1,
  "kind": "operations",
  "ops": [{
    "op": "set_where",
    "selector": {"type": "Item", "filter": "price < 100"},
    "path": [{"kind": "field", "name": "price"}],
    "value": {"kind": "expression", "source": "100"},
    "expected_count": 2
  }]
}
```

Set expected_count to the verified total. Mismatch returns conflict without publication. Selector limit does not cap writes: operations affect every match. Do not infer total targets from one page's items. Operations read the current candidate sequentially, so earlier changes affect later filters.

Protect the queried scope with `--if-revision <revision>` and individual old values with expected. Reread and review stale plans instead of simply replacing the revision and retrying.

### Joint CFT and CFD changes

`kind: files` supports `write`, `create`, `delete`, `add_input` and `remove_input`. write/create file paths are project-relative. Supply exactly one of content or content_file. content_file resolves relative to the request JSON directory, or calling directory for `--input -`. `--input` itself is relative to the calling directory. write/delete accept expected_fingerprint.

With the request and candidates directory together:

```json
{
  "protocol_version": 1,
  "kind": "files",
  "changes": [
    {"op": "write", "file": "schema/items.cft", "content_file": "candidates/items.cft"},
    {"op": "write", "file": "data/items.cfd", "content_file": "candidates/items.cfd"}
  ]
}
```

Candidate CFT and CFD must satisfy the new schema together; add required data when introducing required fields. add_input/remove_input take `kind: "schema"` or `"data"` and path; remove_input changes configuration only. File mode repairs sources that cannot load structurally. Operations mode preserves other source when editing known fields or collections.

```sh
coflow check --project . --input changes.json --rules --json
coflow apply --project . --input changes.json --rules --dry-run --json
coflow apply --project . --input changes.json --rules --json
```

Choose validation, preview or publication as needed. Dry-run validates the same candidate as apply, but successful preview does not write. Query `coflow describe apply --schema input --json` for the full structure.

## Dimensions and statistics

```powershell
coflow dimension list --json
coflow dimension inspect --dimension language --json
coflow dimension variants --dimension language --json
coflow dimension get --type Item --id sword --field name --dimension language --variant ja --json
coflow dimension set --type Item --id sword --field name --dimension language --variant ja --value-cfd '"剣"'
coflow dimension clear --type Item --id sword --field name --dimension language --variant ja
coflow dimension missing --dimension language --variant ja --json
coflow dimension diff --dimension language --from-base --to ja --json
coflow stats project --json
coflow stats records --group-by type --json
coflow stats field --type Item --field price --metrics count,min,max,mean --json
coflow stats dimension --dimension language --variant ja --group-by type --json
coflow stats refs --direction incoming --json
```

Missing overrides fall back to base values. get returns `explicit` for field coverage and `variant_exists` for dimension-wide existence; `override` and `effective` separate supplied and read values. Explicit None and values equal to base count as coverage. Clearing restores fallback. missing returns uncovered fields, display descriptions, base values and other explicit variants. Dimension diffs separately report override-state and effective-value changes. `--from-base` and `--to-base` select base perspectives; a variant named base remains an ordinary variant.

Statistics report participating counts, unreachable paths, unreachable Option layers, empty states and their counting definitions. Enum and boolean fields default to distributions. Fields and metrics are validated even for empty results. Integer sums use exact decimal strings; means retain exact numerators and denominators. Quantiles use nearest-rank. String lengths count Unicode scalars. Coverage divides by eligible record-field pairs and is null when the denominator is zero. Reference stats return only the selected direction, defaulting to both.

## Validation, diffs and generation

```powershell
coflow schema check --json
coflow check --json
coflow check --rules --explain --limit 20 --json
coflow rule list --json
coflow rule inspect --rule Item::ValidPrice --json
coflow check --rule Item::ValidPrice --explain --json
coflow check --input changes.json --rules --json
coflow explain DATA-011 --json
coflow diff --base HEAD --summary --json
coflow diff --input changes.json --json
coflow format --file data/items.cfd --check --json
coflow format --file data/items.cfd --dry-run --json
coflow format --stdin --language cfd
coflow codegen --dry-run --json
coflow codegen --check --json
coflow build --json
```

schema check validates schema; check validates structure, fields, references and dimensions; `check --rules` executes all rules. Repeated `--rule` selects their union and conflicts with `--rules`. Candidate checks use apply requests and are read-only. Named type rules use `Item::ValidPrice`; project rules use names directly. List anonymous rule IDs before selecting them and query again after source positions change; name rules for stable identities. Default results report `rule_counts` and requested/executed/unexecuted task counts. `--details` expands per-target `results` and statement `tasks`. Statuses distinguish passed, not triggered, business failure, evaluation error, not executed and budget exhausted. `complete` reports completion separately from success.

Diff defaults to Git HEAD versus the current project and accepts commits, branches or candidate changes. `--view source|schema|record|field|dimension` selects views; omission returns all. `--summary` returns counts and object summaries; `--include-source` adds full source. When semantics are unavailable, source diffs remain with a partial-result report.

Format defaults to all project inputs and accepts repeated `--file`. Success, preview and conflict results use `files` entries with path, change and patch. Read full files with source read. `--check` and `--dry-run` are mutually exclusive and read-only. Stdin mode is project-independent; text output is formatted source, while JSON places source in result.

Codegen generates after structure and data pass; `--dry-run` previews and `--check` checks output consistency. Build generates after all business rules pass. Targets publish together once prepared. Publication failure reports actual writes, restored files and restoration failures; recovery conflicts preserve external changes.

## Output and exit statuses

Default output is terminal text. `--json` emits single-line JSON; `--pretty` implies indented JSON. `--details` controls content independently of layout. Argument, read and execution failures also use the common envelope: `protocol_version`, `command`, `status`, `result`, and applicable `error_category`, `project`, `revision`, `data_scope`, `diagnostics`.

At most 50 diagnostics are returned by default. `diagnostics_summary` reports total, returned and truncated. `--details` removes that default cap; explicit `--limit` still applies. help offers brief repairs; `--explain` adds causes and actual values. Query codes with coflow explain. Truncation does not change validation completeness, writes or exit status.

Absent optional metadata and empty supplemental lists are omitted. Business null, None, empty strings, arrays and dictionaries are preserved. Request errors provide `result.request_path`, zero-based `operation_index` and target where possible. Source positions are zero-based with UTF-16 columns.

Pagination truncation and incomplete loading are separate. Default data_scope contains `complete`, `available_records`, `selected_file_count`, `incomplete_file_count`, `rejected_record_count`, `details_omitted`. `--details` expands selected/incomplete files, reasons and rejected records. Commands that do not read data omit this metadata.

| Exit code | Status |
| --- | --- |
| 0 | Complete success |
| 1 | Validation failure, unmet state check or incomplete result |
| 2 | Invalid arguments or request |
| 3 | Revision, cursor, file or expected old-value conflict |
| 4 | I/O or execution failure |

Zero list matches are successful; absent exact targets are validation failures. Warnings may accompany complete success. Help and version use their own output; lsp stdout is reserved for language service traffic.

## Installation and services

```powershell
coflow init <dir>
coflow lsp --project <project>
coflow skill install -g
coflow skill status -g
coflow skill uninstall -g
coflow self-update
```

## Complete command and parameter reference

All 53 commands are listed below. Prefix each with `coflow`. Required parameters must be supplied; mutually exclusive required groups are listed separately. Global parameters apply to every command. Value parameters take a following value; switches use the parameter name alone.

| Global parameter | Meaning |
| --- | --- |
| `--project` | Project directory or coflow.yaml; otherwise search parents |
| `--json` | Single-line JSON |
| `--pretty` | Indented JSON; implies --json |
| `--details` | Expand execution details and diagnostics; explicit --limit still applies |
| `--help` / `-h` | Current command help |
| `--version` / `-V` | CLI version at the root command |

### `init`

Create a project directory and initial configuration.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `dir` | Optional | Default `.` |

### `describe`

List commands, parameters and input/output structures.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `command` | Optional | See parameter descriptions below |
| `--schema` | Optional | `input`, `output`, `both`; requires positional parameter `command` |

### `project inspect`

Inspect project configuration and input/output scopes.

No command-specific parameters.

### `project files`

List actual project inputs.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--kind` | Optional | `schema`, `data` |

### `project add-input`

Add an input path to configuration.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--dry-run` | Optional | Switch |
| `--rules` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--path` | Required | See parameter descriptions below |
| `--kind` | Required | `schema`, `data` |

### `project remove-input`

Remove an input path from configuration, retaining files.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--dry-run` | Optional | Switch |
| `--rules` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--path` | Required | See parameter descriptions below |
| `--kind` | Required | `schema`, `data` |

### `context`

Read bounded context for a type or record.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--include-derived` | Optional | Switch |
| `--file` | Optional | See parameter descriptions below |
| `--id` | Optional | See parameter descriptions below |
| `--fields` | Optional | See parameter descriptions below |
| `--where` | Optional | See parameter descriptions below |
| `--sort` | Optional | See parameter descriptions below |
| `--cursor` | Optional | See parameter descriptions below |
| `--dimension` | Optional | See parameter descriptions below |
| `--limit` | Optional | Default `50`; Minimum 1 |
| `--type` | Required | See parameter descriptions below |
| `--examples` | Optional | Default `3`; Minimum 0 |
| `--ref-limit` | Optional | Default `10`; Minimum 0 |
| `--budget` | Optional | Default `16000`; Minimum 1 |
| `--depth` | Optional | Default `4`; Minimum 0 |

### `schema check`

Validate current or candidate CFT declarations.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--input` | Optional | See parameter descriptions below |
| `--limit` | Optional | Minimum 1 |
| `--explain` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |

### `schema inspect`

Query types, fields, defaults and related declarations.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--type` | Optional | See parameter descriptions below |
| `--include-derived` | Optional | Switch |
| `--related` | Optional | Switch |
| `--depth` | Optional | Default `4`; Minimum 0 |
| `--limit` | Optional | Default `100`; Minimum 1 |
| `--field` | Optional | requires `--type` |

### `schema symbols`

List type, field, enum, constant or rule symbols.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--cursor` | Optional | See parameter descriptions below |
| `--kind` | Optional | `type`, `field`, `enum`, `const`, `rule` |
| `--limit` | Optional | Default `50`; Minimum 1 |

### `schema refs`

Find uses of a declaration symbol.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--cursor` | Optional | See parameter descriptions below |
| `--symbol` | Required | See parameter descriptions below |
| `--limit` | Optional | Default `50`; Minimum 1 |

### `source read`

Read full source or a line range.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--start-line` | Optional | Minimum 0 |
| `--end-line` | Optional | Minimum 0 |
| `--file` | Required | See parameter descriptions below |

### `source write`

Replace an existing source file.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--expected-fingerprint` | Optional | See parameter descriptions below |
| `--dry-run` | Optional | Switch |
| `--rules` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--file` | Required | See parameter descriptions below |
| `--input` | Required | See parameter descriptions below |

### `source create`

Create a source file.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--dry-run` | Optional | Switch |
| `--rules` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--file` | Required | See parameter descriptions below |
| `--input` | Required | See parameter descriptions below |

### `source delete`

Delete a source file.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--expected-fingerprint` | Optional | See parameter descriptions below |
| `--dry-run` | Optional | Switch |
| `--rules` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--file` | Required | See parameter descriptions below |

### `data list`

Query matching records.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--type` | Optional | See parameter descriptions below |
| `--include-derived` | Optional | Switch |
| `--file` | Optional | See parameter descriptions below |
| `--id` | Optional | See parameter descriptions below |
| `--fields` | Optional | See parameter descriptions below |
| `--where` | Optional | See parameter descriptions below |
| `--sort` | Optional | See parameter descriptions below |
| `--cursor` | Optional | See parameter descriptions below |
| `--dimension` | Optional | See parameter descriptions below |
| `--limit` | Optional | Default `50`; Minimum 1 |
| `--view` | Optional | `source`, `effective`, `both`; Default `effective` |

### `data get`

Read explicit or effective values of one record.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--fields` | Optional | See parameter descriptions below |
| `--type` | Required | See parameter descriptions below |
| `--id` | Required | See parameter descriptions below |
| `--view` | Optional | `source`, `effective`, `both`; Default `effective` |
| `--dimension` | Optional | See parameter descriptions below |

### `data search`

Search text in record IDs and values.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--type` | Optional | See parameter descriptions below |
| `--include-derived` | Optional | Switch |
| `--file` | Optional | See parameter descriptions below |
| `--id` | Optional | See parameter descriptions below |
| `--fields` | Optional | See parameter descriptions below |
| `--where` | Optional | See parameter descriptions below |
| `--sort` | Optional | See parameter descriptions below |
| `--cursor` | Optional | See parameter descriptions below |
| `--dimension` | Optional | See parameter descriptions below |
| `--limit` | Optional | Default `50`; Minimum 1 |
| `--text` | Required | See parameter descriptions below |
| `--view` | Optional | `source`, `effective`, `both`; Default `effective` |

### `data draft`

Prepare a record draft and required-input guidance.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--id` | Optional | See parameter descriptions below |
| `--type` | Required | See parameter descriptions below |

### `data create`

Create a record from a field request.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--dry-run` | Optional | Switch |
| `--rules` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--type` | Required | See parameter descriptions below |
| `--id` | Required | See parameter descriptions below |
| `--file` | Required | See parameter descriptions below |
| `--input` | Required | See parameter descriptions below |

### `data set`

Set an explicit field value.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--expected-cfd` | Optional | See parameter descriptions below |
| `--value-cfd` | Optional | See parameter descriptions below |
| `--value-file` | Optional | See parameter descriptions below |
| `--dry-run` | Optional | Switch |
| `--rules` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--type` | Required | See parameter descriptions below |
| `--id` | Required | See parameter descriptions below |
| `--path` | Required | See parameter descriptions below |

Supply exactly one of `--value-cfd` and `--value-file`.

### `data unset`

Remove an assignment and reapply default checks.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--dry-run` | Optional | Switch |
| `--rules` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--type` | Required | See parameter descriptions below |
| `--id` | Required | See parameter descriptions below |
| `--path` | Required | See parameter descriptions below |

### `data rename`

Rename a record and update references and overrides.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--dry-run` | Optional | Switch |
| `--rules` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--type` | Required | See parameter descriptions below |
| `--id` | Required | See parameter descriptions below |
| `--new-id` | Required | See parameter descriptions below |

### `data delete`

Delete a record and its overrides.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--dry-run` | Optional | Switch |
| `--rules` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--type` | Required | See parameter descriptions below |
| `--id` | Required | See parameter descriptions below |

### `data transfer`

Move a record to another CFD file.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--dry-run` | Optional | Switch |
| `--rules` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--type` | Required | See parameter descriptions below |
| `--id` | Required | See parameter descriptions below |
| `--to-file` | Required | See parameter descriptions below |

### `data refs`

Query incoming or outgoing record references.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--cursor` | Optional | See parameter descriptions below |
| `--type` | Required | See parameter descriptions below |
| `--id` | Required | See parameter descriptions below |
| `--direction` | Optional | `incoming`, `outgoing`, `both`; Default `both` |
| `--limit` | Optional | Default `50`; Minimum 1 |
| `--depth` | Optional | Default `1`; Minimum 1; Maximum 64 |

### `dimension list`

List dimensions.

No command-specific parameters.

### `dimension inspect`

Inspect a dimension and its bound fields.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--dimension` | Required | See parameter descriptions below |

### `dimension variants`

List declared variants.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--cursor` | Optional | See parameter descriptions below |
| `--dimension` | Required | See parameter descriptions below |
| `--limit` | Optional | Default `50`; Minimum 1 |

### `dimension get`

Read an explicit override and effective value.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--dimension` | Required | See parameter descriptions below |
| `--variant` | Required | See parameter descriptions below |
| `--type` | Required | See parameter descriptions below |
| `--id` | Required | See parameter descriptions below |
| `--field` | Required | See parameter descriptions below |

### `dimension set`

Set an explicit field override.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--file` | Optional | See parameter descriptions below |
| `--path` | Optional | See parameter descriptions below |
| `--value-cfd` | Optional | See parameter descriptions below |
| `--value-file` | Optional | See parameter descriptions below |
| `--dry-run` | Optional | Switch |
| `--rules` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--dimension` | Required | See parameter descriptions below |
| `--variant` | Required | See parameter descriptions below |
| `--type` | Required | See parameter descriptions below |
| `--id` | Required | See parameter descriptions below |
| `--field` | Required | See parameter descriptions below |

Supply exactly one of `--value-cfd` and `--value-file`.

### `dimension clear`

Clear an explicit field override.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--dry-run` | Optional | Switch |
| `--rules` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--dimension` | Required | See parameter descriptions below |
| `--variant` | Required | See parameter descriptions below |
| `--type` | Required | See parameter descriptions below |
| `--id` | Required | See parameter descriptions below |
| `--field` | Required | See parameter descriptions below |

### `dimension missing`

Find fields missing explicit overrides.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--type` | Optional | See parameter descriptions below |
| `--include-derived` | Optional | Switch |
| `--file` | Optional | See parameter descriptions below |
| `--id` | Optional | See parameter descriptions below |
| `--fields` | Optional | See parameter descriptions below |
| `--where` | Optional | See parameter descriptions below |
| `--sort` | Optional | See parameter descriptions below |
| `--cursor` | Optional | See parameter descriptions below |
| `--field` | Optional | See parameter descriptions below |
| `--dimension` | Required | See parameter descriptions below |
| `--variant` | Required | See parameter descriptions below |
| `--limit` | Optional | Default `50`; Minimum 1 |

### `dimension diff`

Compare variants or base values and a variant.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--type` | Optional | See parameter descriptions below |
| `--include-derived` | Optional | Switch |
| `--file` | Optional | See parameter descriptions below |
| `--id` | Optional | See parameter descriptions below |
| `--fields` | Optional | See parameter descriptions below |
| `--where` | Optional | See parameter descriptions below |
| `--sort` | Optional | See parameter descriptions below |
| `--cursor` | Optional | See parameter descriptions below |
| `--from` | Optional | See parameter descriptions below |
| `--to` | Optional | See parameter descriptions below |
| `--from-base` | Optional | Switch |
| `--to-base` | Optional | Switch |
| `--field` | Optional | See parameter descriptions below |
| `--dimension` | Required | See parameter descriptions below |
| `--limit` | Optional | Default `50`; Minimum 1 |

Supply exactly one of `--from` and `--from-base`.

Supply exactly one of `--to` and `--to-base`.

### `stats project`

Count project contents.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--type` | Optional | See parameter descriptions below |
| `--include-derived` | Optional | Switch |
| `--file` | Optional | See parameter descriptions below |
| `--id` | Optional | See parameter descriptions below |
| `--fields` | Optional | See parameter descriptions below |
| `--where` | Optional | See parameter descriptions below |
| `--sort` | Optional | See parameter descriptions below |
| `--cursor` | Optional | See parameter descriptions below |
| `--dimension` | Optional | See parameter descriptions below |
| `--limit` | Optional | Default `50`; Minimum 1 |

### `stats records`

Count records by type or file.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--type` | Optional | See parameter descriptions below |
| `--include-derived` | Optional | Switch |
| `--file` | Optional | See parameter descriptions below |
| `--id` | Optional | See parameter descriptions below |
| `--fields` | Optional | See parameter descriptions below |
| `--where` | Optional | See parameter descriptions below |
| `--sort` | Optional | See parameter descriptions below |
| `--cursor` | Optional | See parameter descriptions below |
| `--dimension` | Optional | See parameter descriptions below |
| `--limit` | Optional | Default `50`; Minimum 1 |
| `--group-by` | Optional | `type`, `file`; Default `type` |

### `stats field`

Summarize field values, collections or Option states.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--type` | Optional | See parameter descriptions below |
| `--include-derived` | Optional | Switch |
| `--file` | Optional | See parameter descriptions below |
| `--id` | Optional | See parameter descriptions below |
| `--fields` | Optional | See parameter descriptions below |
| `--where` | Optional | See parameter descriptions below |
| `--sort` | Optional | See parameter descriptions below |
| `--cursor` | Optional | See parameter descriptions below |
| `--option-layer` | Optional | Minimum 1 |
| `--dimension` | Optional | See parameter descriptions below |
| `--limit` | Optional | Default `50`; Minimum 1 |
| `--field` | Required | See parameter descriptions below |
| `--group-by` | Optional | `value` |
| `--quantiles` | Optional | Default `0.5,0.9,0.95,0.99` |
| `--metrics` | Optional | See parameter descriptions below |

### `stats dimension`

Measure explicit override coverage.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--type` | Optional | See parameter descriptions below |
| `--include-derived` | Optional | Switch |
| `--file` | Optional | See parameter descriptions below |
| `--id` | Optional | See parameter descriptions below |
| `--fields` | Optional | See parameter descriptions below |
| `--where` | Optional | See parameter descriptions below |
| `--sort` | Optional | See parameter descriptions below |
| `--cursor` | Optional | See parameter descriptions below |
| `--field` | Optional | See parameter descriptions below |
| `--dimension` | Required | See parameter descriptions below |
| `--variant` | Required | See parameter descriptions below |
| `--group-by` | Optional | `all`, `type`, `file`; Default `all` |
| `--limit` | Optional | Default `50`; Minimum 1 |

### `stats refs`

Summarize record references.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--type` | Optional | See parameter descriptions below |
| `--include-derived` | Optional | Switch |
| `--file` | Optional | See parameter descriptions below |
| `--id` | Optional | See parameter descriptions below |
| `--fields` | Optional | See parameter descriptions below |
| `--where` | Optional | See parameter descriptions below |
| `--sort` | Optional | See parameter descriptions below |
| `--cursor` | Optional | See parameter descriptions below |
| `--dimension` | Optional | See parameter descriptions below |
| `--limit` | Optional | Default `50`; Minimum 1 |
| `--direction` | Optional | `incoming`, `outgoing`, `both`; Default `both` |

### `check`

Validate data and optionally execute business rules.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--rules` | Optional | Switch |
| `--input` | Optional | See parameter descriptions below |
| `--limit` | Optional | Minimum 1 |
| `--explain` | Optional | Switch |
| `--performance` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--rule` | Optional | conflicts with  `--rules`  |

### `rule list`

List rules and dependencies.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--type` | Optional | See parameter descriptions below |

### `rule inspect`

Inspect a selected rule.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--rule` | Required | See parameter descriptions below |

### `explain`

Explain an error code and repairs.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `code` | Required | See parameter descriptions below |

### `apply`

Validate and publish a changeset.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--dry-run` | Optional | Switch |
| `--rules` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--input` | Required | See parameter descriptions below |

### `diff`

Compare a Git revision or candidate changes.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--summary` | Optional | Switch |
| `--include-source` | Optional | Switch |
| `--type` | Optional | See parameter descriptions below |
| `--if-revision` | Optional | See parameter descriptions below |
| `--input` | Optional | conflicts with  `--base`  |
| `--view` | Optional | `source`, `schema`, `record`, `field`, `dimension` |
| `--base` | Optional | Default `HEAD` |

### `format`

Format files or standard input.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--dry-run` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--check` | Optional | Switch; conflicts with  `--dry-run`  |
| `--file` | Optional | See parameter descriptions below |
| `--stdin` | Optional | Switch; requires `--language`; conflicts with  `--file`, `--project`, `--if-revision`  |
| `--language` | Optional | `cft`, `cfd`; requires `--stdin` |

`--stdin` conflicts with file, project and revision parameters and requires `--language`. In file mode, repeat `--file` as needed; omission formats all inputs.

### `codegen`

Generate, preview or check code.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--dry-run` | Optional | Switch |
| `--if-revision` | Optional | See parameter descriptions below |
| `--check` | Optional | Switch; conflicts with  `--dry-run`  |

### `build`

Run all validation and generate code.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--if-revision` | Optional | See parameter descriptions below |

### `lsp`

Run the language server over standard input/output.

No command-specific parameters.

### `skill install`

Install companion AI skills.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--global` | Optional | Switch |

`--global` (`-g`) selects global installation; omission uses the project installation.

### `skill uninstall`

Remove installed companion skills.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--global` | Optional | Switch |

`--global` (`-g`) selects global installation; omission uses the project installation.

### `skill status`

Inspect companion skill installation status.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--global` | Optional | Switch |

`--global` (`-g`) selects global installation; omission uses the project installation.

### `self-update`

Check for or install CLI updates.

| Parameter | Requirement | Values and defaults |
| --- | --- | --- |
| `--check` | Optional | Switch |
| `--yes` | Optional | Switch |

`--check` only checks for updates; `--yes` skips interactive installation confirmation.

## Parameter descriptions

The same-named parameters have the meanings below. Command tables above determine availability and required status.

| Parameter | Meaning and usage |
| --- | --- |
| `dir` | init positional directory, defaults to current directory |
| `command` | describe positional command or group, dotted names such as data.set |
| `code` | explain positional complete code, e.g. DATA-011 |
| `--schema` | describe JSON Schema scope input/output/both; requires a leaf command |
| `--type` | Declaration or record type name |
| `--include-derived` | Include derived types |
| `--id` | Record ID |
| `--field` | Field name; schema inspect also requires a type |
| `--fields` | Comma-separated returned field paths |
| `--path` | Nested value path for data commands; input path for project add/remove-input; path inside override for dimension set |
| `--file` | Project-relative source path; query filter or write source/destination |
| `--kind` | schema/data for project commands; type/field/enum/const/rule for schema symbols |
| `--symbol` | Declaration symbol, e.g. Item or Item.price |
| `--related` | Include referenced declarations in schema inspect |
| `--depth` | Context/reference expansion depth; data refs supports 1–64, default 1 |
| `--examples` | Context example record limit, default 3 |
| `--ref-limit` | Context reference candidate limit, default 10 |
| `--budget` | Context serialized-character budget, default 16000, positive |
| `--start-line` / `--end-line` | Zero-based source line range, start inclusive and end exclusive |
| `--view` | source/effective/both for data; source/schema/record/field/dimension for diff |
| `--text` | Search text in record IDs and values |
| `--where` | Typed boolean filter: comparisons, logic, contains/is_some/is_none/is_reachable |
| `--sort` | Comma-separated field paths; prefix - for descending |
| `--limit` | Positive returned item/diagnostic cap; defaults in command tables |
| `--cursor` | Previous next_cursor; preserve query conditions and revision |
| `--dimension` | Dimension name for dimension commands; repeatable name=variant selections for record queries |
| `--variant` | Variant within the selected dimension |
| `--from` / `--to` | Source and destination variants for dimension diffs |
| `--from-base` / `--to-base` | Base perspective, each exclusive with corresponding --from/--to |
| `--direction` | incoming/outgoing/both reference direction |
| `--group-by` | type/file for records; all/type/file for dimension; value distribution for field |
| `--metrics` | Comma-separated metrics listed below |
| `--option-layer` | One-based Option layer to inspect |
| `--quantiles` | Comma-separated quantiles in [0,1], default 0.5,0.9,0.95,0.99 |
| `--input` | JSON request file, or source content for source write/create; - reads stdin |
| `--value-cfd` | CFD expression parsed against the target field type |
| `--value-file` | File containing a CFD expression; - reads stdin; exclusive with --value-cfd |
| `--expected-cfd` | Require current effective field value to equal this CFD expression |
| `--expected-fingerprint` | Require matching content fingerprint for replacement/deletion |
| `--if-revision` | Require the revision from a previous query |
| `--new-id` | New record ID |
| `--to-file` | Destination CFD path for a record transfer |
| `--dry-run` | Validate and preview without writing |
| `--rules` | Run all business rules |
| `--rule` | Repeatable selected rule identity, exclusive with --rules |
| `--explain` | Include diagnostic causes, actual values and rule explanations |
| `--performance` | Include rule timing and work counts |
| `--base` | Git baseline for diff, default HEAD; exclusive with candidate --input |
| `--summary` | Diff counts and summaries only |
| `--include-source` | Include full source in diff |
| `--stdin` | Read format input without a project |
| `--language` | Stdin formatting language, cft or cfd |
| `--check` | Check status without writing; exclusive with --dry-run in format/codegen |
| `--global` / `-g` | Global skill directory |
| `--yes` | Skip interactive self-update installation confirmation |

Metrics: `count`, `min`, `max`, `sum`, `mean`, `quantiles`, `some`, `none`, `empty_count`, `length_distribution`, `length_min`, `length_max`. Metrics must match field type. Select the Option layer with `--option-layer`. Quantiles use nearest-rank.

### Request files and standard input

source write/create --input reads complete source text. apply and candidate check/schema check/diff --input read JSON changesets. data create --input takes a field object:

```json
{
  "name": { "kind": "expression", "source": "\"Iron sword\"" },
  "price": { "kind": "expression", "source": "100" }
}
```

Query full schemas with `coflow describe data.create --schema input --json` or `coflow describe apply --schema input --json`. At most one parameter per invocation can read stdin. Your shell parses command lines first: quote strings, parentheses and spaces using its rules, or use `--value-file` to avoid escaping.

### Language service and AI tools

`coflow lsp --project <directory>` transports LSP over stdin/stdout for LSP-capable editors. Its stdout is not ordinary command JSON. Install companion skills with `coflow skill install -g`, inspect with `coflow skill status -g` and remove with `coflow skill uninstall -g`. Machine queries and edits use this page's JSON output and requests.
