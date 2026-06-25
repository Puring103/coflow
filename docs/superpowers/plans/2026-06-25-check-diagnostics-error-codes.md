# Check Diagnostics Error Codes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement specific `CFD-CHECK-007` through `CFD-CHECK-019` runtime check diagnostics with detailed messages, while keeping `when` as context only.

**Architecture:** Add new `CfdErrorCode` variants in `coflow-data-model`, then teach `coflow-checker` to classify false conditions through an internal explanation model. Keep API and JSON shape unchanged: downstream engine/CLI continue consuming `CfdDiagnostic` as before.

**Tech Stack:** Rust workspace, `coflow-data-model`, `coflow-checker`, CLI integration tests.

---

### Task 1: Add Error Code Variants

**Files:**
- Modify: `crates/coflow-data-model/src/diagnostic.rs`
- Test: existing compiler coverage via `cargo test -p coflow-data-model`

- [ ] **Step 1: Add failing references through checker tests in Task 2 first**

Do not change production code until tests reference the new variants.

- [ ] **Step 2: Add variants**

Add variants after `CheckEmptyMinMax`:

```rust
CheckComparisonFailed,
CheckBoolExpectedTrue,
CheckNegationFailed,
CheckAndFailed,
CheckOrFailed,
CheckTypePredicateFailed,
CheckNullPredicateFailed,
CheckContainsFailed,
CheckUniqueFailed,
CheckMatchesFailed,
CheckAnyQuantifierFailed,
CheckNoneQuantifierFailed,
CheckAllQuantifierFailed,
```

Map them to `CFD-CHECK-007` through `CFD-CHECK-019` in `as_str`, and include them in `stage()` as `CfdStage::Check`.

### Task 2: Add Focused Failing Checker Tests

**Files:**
- Modify: `crates/coflow-checker/tests/check.rs`

- [ ] **Step 1: Add test helpers**

Add helpers near existing test helpers if needed:

```rust
fn assert_first_code(diags: &CfdDiagnostics, code: CfdErrorCode) {
    assert_eq!(diags.diagnostics[0].code, code, "{diags:#?}");
}

fn assert_message_contains(diags: &CfdDiagnostics, text: &str) {
    assert!(
        diags.diagnostics.iter().any(|diag| diag.message.contains(text)),
        "missing `{text}` in {diags:#?}"
    );
}
```

- [ ] **Step 2: Add one test covering scalar false conditions**

Cover `CFD-CHECK-007` through `CFD-CHECK-016` with small schemas and model values.

- [ ] **Step 3: Add one test covering quantifiers and when context**

Cover `CFD-CHECK-017` through `CFD-CHECK-019`, and assert `when` keeps inner code with Chinese context in message.

- [ ] **Step 4: Run RED**

Run:

```powershell
cargo test -p coflow-checker check_diagnostics
```

Expected: compile failure for missing `CfdErrorCode` variants or assertion failures against old broad diagnostics.

### Task 3: Implement Explanation Model and Classification

**Files:**
- Modify: `crates/coflow-checker/src/check/evaluator.rs`

- [ ] **Step 1: Add internal explanation type**

Add a private struct:

```rust
struct CheckExplanation {
    code: CfdErrorCode,
    expression: String,
    actual: Option<String>,
    expected: Option<String>,
    context: Vec<String>,
    path: Option<CfdPath>,
}
```

Add `message()` rendering with `校验失败:`, optional `实际值:`, `期望:`, and `上下文:` lines.

- [ ] **Step 2: Add expression render helpers**

Render `CftSchemaCheckExpr` and `CftSchemaCheckStmt` into compact text for diagnostics.

- [ ] **Step 3: Replace false condition handling**

Change `eval_stmt` false branch to call a recursive `explain_false_expr`. The recursive classifier must cover comparison, bare bool, negation, `&&`, `||`, `is`, `contains`, `unique`, and `matches`.

- [ ] **Step 4: Preserve hard errors**

Keep `CheckEvalTypeError`, `CheckNullAccess`, `CheckIndexOutOfBounds`, `CheckMissingDictKey`, and `CheckEmptyMinMax` behavior. Add context only where a body is evaluated inside `when` or a quantifier.

### Task 4: Quantifier and When Context

**Files:**
- Modify: `crates/coflow-checker/src/check/evaluator.rs`

- [ ] **Step 1: Add context stack**

Add a `contexts: Vec<String>` field to `CheckEvaluator`.

- [ ] **Step 2: Push `when` context only for true body evaluation**

When the condition evaluates true, push `在 when <condition> 内` before evaluating the body and pop afterward.

- [ ] **Step 3: Classify quantifier failures**

For false element body failures:

- `all` emits `CheckAllQuantifierFailed`.
- `any` emits `CheckAnyQuantifierFailed` if no element passed.
- `none` emits `CheckNoneQuantifierFailed` for each unexpected match.

Hard errors inside quantifier bodies keep hard-error codes and include quantifier context.

### Task 5: CLI Test Updates and Verification

**Files:**
- Modify: `tests/cli_check.rs`
- Modify: `docs/spec/10-diagnostics.md`
- Modify: `docs/spec/01-cft.md`

- [ ] **Step 1: Update CLI assertions**

Change representative check failure assertions from `CFD-CHECK-001` / broad message to `CFD-CHECK-007` and detailed message fragments.

- [ ] **Step 2: Update diagnostics docs**

Document `CFD-CHECK-007` through `CFD-CHECK-019`, and state that `when` is context only.

- [ ] **Step 3: Run focused tests**

Run:

```powershell
cargo test -p coflow-checker check_diagnostics
cargo test --test cli_check full_project_check_failure
```

Expected: pass.

- [ ] **Step 4: Run required repository checks**

Run from repository root:

```powershell
cargo check --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Expected: all pass.
