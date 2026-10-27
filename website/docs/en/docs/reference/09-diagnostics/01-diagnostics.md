# Diagnostics and troubleshooting

The CLI, editor and LSP locate problems with error codes, messages and source positions. Use `coflow check --rules --json` for structured diagnostics and `coflow explain <code> --json` for meanings and repair guidance.

## CLI diagnostics

The response's `diagnostics` contains:

| Field | Meaning |
| --- | --- |
| `code` / `stage` / `severity` | Error code, validation stage and severity |
| `message` | Problem description |
| `help` | Short, actionable repair guidance |
| `location` | Source path, source range or configuration path |
| `record` / `field_path` | Identified record type, ID and field path |
| `related` | Related declaration or data positions and messages |
| `contexts` | Rule, quantifier, dimension and other contexts |
| `explanation` | Explanation requested by `--explain` |

Lines and columns are zero-based; columns count UTF-16 offsets. Unavailable positions, record coordinates and optional explanations are omitted. Display both code and message. The editor and LSP convert positions into navigable source or field locations.

Inputs exceeding size or nesting limits return corresponding codes. Rule budget exhaustion explicitly reports incomplete checking. Full success requires no errors and completion of all selected checks. See the [error code index](./02-codes.md) and [CLI exit statuses](../08-cli.md).

## Output limits and repairs

The CLI returns at most 50 diagnostics by default; `diagnostics_summary` reports total, returned and truncated counts. Default `data_scope` includes loading completeness, file counts and rejected-record counts. `--details` expands source lists and reasons. Start with available diagnostics; expand with `--details` or set `--limit`. These affect output only, not validation scope; `--details --limit 20` still returns at most 20 diagnostics.

Locate problems with `code`, `help`, `record` and `field_path`. For rules, use `coflow check --rule Item::ValidPrice --explain --json` to focus on one rule. `complete: false` means checking did not fully finish; resolve evaluation or budget issues and retry.

Malformed requests identify JSON fields with `result.request_path`; operation errors also include `operation_index` and `target`. For conflicts, reread the target and verify revision or old-value conditions before submitting. After transaction failure, inspect `written_files`, `restored_files` and `restoration_failed` to establish actual file state.

## Severity and stages

Severity is `error`, `warning` or `info`. Errors prevent full success; warnings and information may accompany successful results. Stage identifies where the problem occurred; code identifies its cause. Use both alongside source coordinates.

| Stage | Scope |
| --- | --- |
| `LEX` / `SYN` | CFT characters, strings and syntax |
| `SCHEMA` / `TYPE` | Declarations, inheritance, defaults, annotations and check expression types |
| `CFD` | CFD reading, syntax and assignments |
| `DATA` | Field values, required fields, record identity and data structure |
| `REF` | Reference targets and types |
| `CHECK` | Business assertions, evaluation errors and execution budgets |
| `PROJECT` | Configuration, input paths and project files |
| `CLI` | Requests, query conditions, mutations and conflicts |
| `ARTIFACT` | Generated code and artifact publication |
| `WRITE` | Write sources, ordering, transfers and content conflicts |
| `MUTATION` | Record, field and dimension requests |
| `RUNTIME` | Project execution state |
| `GIT` | Git diffs and historical inputs |

Other operational stages depend on the command. See the [complete code index](./02-codes.md) for triggers and repairs.

## Complete location and context structure

| Field | Contents |
| --- | --- |
| `location.path` | Source file path, usually project-relative for project inputs |
| `location.range.start` / `end` | Each has `line`, `character`; start inclusive, end exclusive |
| `location.config_path` | Configuration path array, e.g. `["dimensions", "language", "dir"]` |
| `record.type` / `record.id` | Record declaration type and ID |
| `field_path` | Located field or nested-value path within the record |
| `related[]` | Each entry has `location` and optional `message` |
| `contexts[].kind` | Context category |
| `contexts[].name` / `expression` | Rule or statement name, related expression |
| `contexts[].quantifier` / `binding` / `item` | Quantifier, binding name and element details |
| `contexts[].dimension` / `variant` | Dimension and variant involved in failure |
| `explanation` | Diagnostic-specific explanation requested by `--explain` |

Only available information is returned. Type rule names use `Item::ValidPrice`; project rules use their names directly. Both CFT declaration and CFD positions can be navigated. Quantifier and dimension contexts locate individual failures.

Example location metadata (actual messages may differ):

```json
{
  "code": "DATA-006",
  "stage": "DATA",
  "severity": "error",
  "message": "Missing required field name",
  "location": {
    "path": "data/items.cfd",
    "range": {
      "start": { "line": 0, "character": 0 },
      "end": { "line": 0, "character": 27 }
    }
  },
  "record": { "type": "Item", "id": "sword" },
  "field_path": "name"
}
```

## Rule results and completeness

`coflow check --rules --details --explain --json` returns rule results and per-target task statuses. Default output summarizes `rule_counts` and task counts; `--details` expands `results` and `tasks`.

| Status | Meaning and action |
| --- | --- |
| `passed` | Executed and assertion holds |
| `not_triggered` | Root when condition is false; body need not execute |
| `failed` | Business assertion failed; correct data according to the message |
| `evaluation_error` | Division by zero, out-of-bounds indexing or similar issue; correct rule or data |
| `budget_exhausted` | Evaluation budget reached; simplify rules or narrow scope and retry |
| `not_executed` | Earlier error or budget in the same rule prevented execution; resolve and retry |

Ordinary business failures continue collecting assertion errors. Evaluation errors stop that rule for the current instance; other rules continue. `complete` reports completion independently of diagnostic counts or output truncation.

Data queries use `data_scope.complete` for input availability, alongside available records, selected files, incomplete files and rejected records. `--details` expands file lists and rejection reasons. Returned data alone does not imply all inputs loaded successfully.

## Explaining codes and command results

```sh
coflow explain DATA-006 --json
coflow schema check --explain --json
coflow check --rule Item::ValidPrice --explain --json
```

`explain` returns `code`, `stage`, `meaning`, `trigger`, `repair` and `queries`: code, stage, meaning, trigger, repair guidance and suggested query entry points. The [code index](./02-codes.md) lists all defined codes with triggers and repairs.

Responses contain `protocol_version`, `command`, `status` and `result`, with optional `error_category`, `project`, `revision`, `data_scope` and `diagnostics`. `diagnostics_summary.total` is the actual count, `returned` the response count, and `truncated` indicates output truncation.

Exit code 0 means complete success, 1 validation failure, unmet state or incomplete results, 2 invalid request, 3 revision or precondition conflict, and 4 I/O or execution failure. Inspect response status and result for argument errors and conflicts as well as diagnostics. See [CLI output and exit statuses](../08-cli.md#output-and-exit-statuses).

## Editor logs and feedback

“Logs and feedback” is available even without an open project, including from the error screen. Review recent incidents and copy the error identifier and details. Generate a diagnostic preview and review it before downloading. Error text and stacks are excluded by default; include them only when needed, and remove information you do not want to share from the preview. Use “Submit issue” to describe reproduction steps, expected results, and actual results, and attach the diagnostic file.

Logs remain local and are not uploaded automatically. “Open log directory” opens the local files. On Windows, editor logs default to `%LOCALAPPDATA%\Coflow\logs\editor`. Logs have size and retention limits; export soon after an incident.

When changes were saved but refreshing failed, use recovery or reload rather than repeating the write. If file recovery is incomplete, inspect the affected files and retained backups before reloading.

For CLI troubleshooting, set `COFLOW_LOG=1` to write local logs under `Coflow/logs/cli` without changing stdout. In VS Code, select `Coflow Language Server` in the Output panel to inspect language server errors.
