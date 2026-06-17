# Comprehensive Quality Followup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the follow-up quality work for generated artifact ownership, enum lockfile placement, `@expand` header semantics, diagnostic coverage, LSP/VS Code ownership, and checker builtin structure.

**Architecture:** Coflow fully owns generated output directories and writes them through a staged commit path. The enum lockfile moves to the project config directory and is written as part of the same successful generation flow, while Rust LSP remains the semantic authority for editor language features.

**Tech Stack:** Rust workspace crates, `rust_xlsxwriter` loader tests, Node-based VS Code extension unit tests, GitHub CLI.

---

### Task 1: Artifact Directory Ownership and Atomic Commit

**Files:**
- Modify: `crates/coflow-pipeline/src/artifacts.rs`
- Modify: `crates/coflow-pipeline/src/lib.rs`
- Test: `crates/coflow-pipeline/tests/export.rs`
- Test: `crates/coflow-pipeline/tests/build.rs`
- Test: `crates/coflow-pipeline/tests/codegen.rs`
- Test: `crates/coflow-pipeline/tests/key_as_enum.rs`
- Test: `tests/cli_codegen.rs`

- [ ] **Step 1: Write failing tests for full directory takeover**

  Update existing manifest tests so stale `.json`, `.msgpack`, `.cs`, and `README.txt` files in generated directories are removed on successful generation, with no `coflow.data.manifest.json` or `coflow.csharp.manifest.json` written.

  Run: `cargo test -p coflow-pipeline export_project_data_removes_stale_generated_data_files build_project_removes_stale_generated_csharp_files_after_key_as_enum_rename`

  Expected before implementation: FAIL because user sidecar files are preserved and manifest files are written.

- [ ] **Step 2: Write failing tests for lockfile location**

  Change lockfile assertions to `root.join("coflow.enum.lock.json")`, including malformed lockfile tests and CLI codegen tests.

  Run: `cargo test -p coflow-pipeline key_as_enum codegen_writes_empty_key_as_enum_lockfile_when_only_declared_ids_exist`

  Expected before implementation: FAIL because the lockfile is still read/written under the C# output directory.

- [ ] **Step 3: Implement staged generated directory replacement**

  Replace manifest-based cleanup with:

  ```rust
  pub struct StagedArtifactDir {
      target: PathBuf,
      staging: PathBuf,
  }
  ```

  The helper creates a unique sibling staging directory, writes all files there, then commits by renaming the old target to a backup, renaming staging to target, and deleting the backup. If any write fails before commit, the previous target stays intact.

- [ ] **Step 4: Move enum lockfile to project config directory**

  Add a helper in `coflow-pipeline/src/lib.rs` that returns `project.config_path.parent().unwrap_or(&project.root_dir).join("coflow.enum.lock.json")`. Read/merge the lockfile before code render but write it only after code artifacts are staged successfully.

- [ ] **Step 5: Run targeted verification and commit**

  Run: `cargo test -p coflow-pipeline`

  Expected after implementation: PASS.

### Task 2: Strict Excel `@expand` Header Semantics

**Files:**
- Modify: `crates/coflow-loader-excel/src/lib.rs`
- Test: `crates/coflow-loader-excel/tests/excel_loader.rs`
- Modify: `docs/spec/04-excel-loader.md`
- Modify: `docs/spec/10-diagnostics.md`

- [ ] **Step 1: Write failing tests**

  Convert the explicit inner header test into a rejection test: an `@expand` parent may consume only immediately following columns whose header cells are blank. Keep merged-header behavior represented as blank following cells.

  Run: `cargo test -p coflow-loader-excel expand`

  Expected before implementation: FAIL because explicit child headers are still accepted.

- [ ] **Step 2: Implement strict blank-only rule**

  Change `resolve_columns` so `next_text` must be empty. Update `UnexpectedExpandHeader` message to say the adjacent header must be empty.

- [ ] **Step 3: Run targeted verification**

  Run: `cargo test -p coflow-loader-excel`

  Expected after implementation: PASS.

### Task 3: Diagnostic Coverage Matrix

**Files:**
- Modify or create: `crates/coflow-loader-excel/tests/error_coverage.rs`
- Modify or create: `crates/coflow-pipeline/tests/error_coverage.rs`
- Modify: `tests/cli_check.rs`
- Modify: `tests/cli_codegen.rs`
- Modify: `tests/cli_lsp.rs`
- Modify: `crates/coflow-lsp/src/tests/protocol.rs`

- [ ] **Step 1: Add missing Excel coverage**

  Cover `EXCEL-OPEN`, `EXCEL-SHEET`, `EXCEL-TYPE`, `EXCEL-COLUMN`, `EXCEL-ID`, and `EXCEL-CELL` with a negative trigger and an adjacent valid path where practical.

- [ ] **Step 2: Add pipeline and CLI coverage**

  Assert project diagnostics use `PROJECT-001`, artifact diagnostics use `ARTIFACT-001`, codegen diagnostics use their `CODEGEN-*` codes, and runtime CLI errors remain `CLI-ERROR`.

- [ ] **Step 3: Add LSP coverage**

  Assert LSP-published diagnostics preserve `DiagnosticJson.code` and source for CFT, Excel/data, project, artifact, and codegen paths that the LSP can surface.

- [ ] **Step 4: Run targeted verification**

  Run: `cargo test -p coflow-loader-excel error_coverage`

  Run: `cargo test -p coflow-pipeline error_coverage`

  Run: `cargo test --test cli_check --test cli_codegen --test cli_lsp`

### Task 4: VS Code Delegates Semantics to Rust LSP

**Files:**
- Modify: `editors/vscode-coflow/src/extension.js`
- Modify: `editors/vscode-coflow/test/extension-unit.test.js`

- [ ] **Step 1: Write failing extension tests**

  Assert completion, hover, document symbols, and definition providers return only LSP responses or `undefined`/empty results when LSP has no response. Keep tests for protocol conversion, session behavior, config discovery, semantic token legend, and command/cwd selection.

  Run: `node editors/vscode-coflow/test/extension-unit.test.js`

  Expected before implementation: FAIL because local fallback still returns symbols/completions/definitions.

- [ ] **Step 2: Remove semantic fallback logic**

  Keep process/session management, config parsing, LSP request plumbing, and LSP-to-VS Code conversion helpers. Remove local parser, workspace symbol collection, local definition fallback, and static semantic completion/hover logic from provider methods and exports.

- [ ] **Step 3: Run extension verification**

  Run: `node editors/vscode-coflow/test/extension-unit.test.js`

  Expected after implementation: PASS.

### Task 5: Checker Builtin Contract Centralization and Focused Split

**Files:**
- Create: `crates/coflow-checker/src/check/builtins.rs`
- Modify: `crates/coflow-checker/src/check.rs`
- Modify: `crates/coflow-checker/src/check/evaluator.rs`
- Test: `crates/coflow-checker/tests/error_coverage.rs`
- Test: `crates/coflow-checker/tests/check.rs`

- [ ] **Step 1: Write or update tests around builtin names and arity**

  Ensure type checking and evaluation agree for `len`, `contains`, `unique`, `min`, `max`, `sum`, `keys`, `values`, and `matches`.

- [ ] **Step 2: Centralize builtin signatures**

  Add a single builtin registry used by the checker type path and evaluator dispatch so supported names and arity are not duplicated.

- [ ] **Step 3: Run checker verification**

  Run: `cargo test -p coflow-checker`

  Expected after implementation: PASS.

### Task 6: Documentation and Final Verification

**Files:**
- Modify: `.gitignore`
- Modify: `README.md`
- Modify: `docs/design-issues.md`
- Modify: `docs/quality-improvements.md`
- Modify: `docs/spec/06-csharp-codegen.md`
- Modify: `docs/spec/07-project-pipeline.md`
- Modify: `docs/spec/09-cli.md`
- Modify: `docs/spec/10-diagnostics.md`

- [ ] **Step 1: Update docs**

  Remove manifest strategy references, document full generated-directory ownership, document root-level `coflow.enum.lock.json`, document staged artifact commit semantics, and document strict blank-only `@expand` consumed headers.

- [ ] **Step 2: Run required gate**

  Run from repository root:

  ```powershell
  cargo check --workspace
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```

  Expected: all commands exit 0.

- [ ] **Step 3: Push and create PR**

  Run: `git push -u origin codex/comprehensive-quality-followup`

  Run: `gh pr create --base main --head codex/comprehensive-quality-followup --title "Finish Coflow quality follow-up" --body-file <body-file>`

  Expected: PR URL is printed.
