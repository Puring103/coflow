# Core Architecture Simplification Plan - 2026-07-11

本计划覆盖核心代码库的第二轮架构简化，不包含编辑器前端。目标不是按文件数量做机械拆合，而是用 module / interface / depth / seam / adapter / locality / leverage / deletion test 判断哪些改动能减少重复策略、收窄浅 interface，并保持现有功能行为。

普通开发检查只运行：

```powershell
cargo check --workspace
cargo test --workspace
```

## Scope

- `tests/repo_hygiene.rs` 架构规则测试拆分。
- `coflow-runtime` mutation public interface 简化。
- `coflow-runtime` write target 解析策略集中。
- `coflow-lsp` request context module 深化。
- `coflow-loader-lark` field write 复用 table-core planner。

## Module 1: Repo Hygiene Test Locality

### Problem

`tests/repo_hygiene.rs` 已经接近 5000 行，把 API、runtime、LSP、loader、codegen、CLI、editor 边界规则都放进同一个浅 module。新增架构规则时需要在一个超大文件中定位，locality 差。

### Plan

- 将 `tests/repo_hygiene.rs` 改为 integration test crate 入口。
- 按领域拆到 `tests/repo_hygiene/` 下的若干 module。
- 保留原有测试语义，不调整架构规则本身。

### Acceptance

- 所有原有 repo hygiene 测试仍运行。
- `cargo check --workspace` 和 `cargo test --workspace` 通过。

## Module 2: Runtime Mutation Interface

### Problem

`PreparedMutation` 作为 public interface 暴露，但当前 `prepare_mutation` 只是把 op 包成 `Pending`，真正 prepare 仍在 apply 时基于最新 session state 发生。这个 seam 偏浅，外部调用方实际使用的是 `apply_mutation`。

### Plan

- 将 `PreparedMutation`、`prepare_mutation`、`apply_prepared_mutation` 收回 runtime mutation module 内部，或删除对外暴露。
- 保留 `apply_mutation` 作为唯一执行入口。
- 保留内部 per-op prepare/apply 行为和 recoverable/terminal diagnostic 语义。

### Acceptance

- 外部 crate 不再能构造 prepared mutation。
- `apply_mutation` 行为不变。
- mutation/data patch/CLI AI data 测试通过。

## Module 3: Runtime Effective Write Target

### Problem

`mutation::prepare` 和 `writes::target` 同时解析 host record + path 到实际写入 source/path，尤其 spread source path 分支重复。write coordinate 策略散在两个 module，locality 不够。

### Plan

- 让 `writes::target` 提供唯一 effective write target interface。
- mutation prepare 通过该 interface 获取实际 `display_path` 和 `WriteFieldPathSegment`。
- 删除 mutation 中重复的 spread source 解析逻辑。

### Acceptance

- spread field file guard 仍使用实际 source file。
- dict/array path 写入行为不变。
- write path、engine command、data patch 测试通过。

## Module 4: LSP Request Context

### Problem

`coflow-lsp/src/lib.rs` 的 completion/hover/definition/documentSymbol/semanticTokens handler 重复做 CFD/CFT 分流、`parse_cfd`、`ensure_build`、`document_by_uri`。`LspValidationCore` 虽然已经承载 validation，但 request-time interface 仍偏浅。

### Plan

- 在 validation core 中增加 request context 查询，例如 `request_document(uri)`。
- 返回 `Cfd` 或 `Cft` context，封装 source、AST、schema/build/document。
- handler 只做 protocol response 组装，避免直接碰 project/open document/build 细节。
- 将 CFD record source overlay 移入 validation core 或相邻 request context module。

### Acceptance

- LSP protocol responses 不变。
- dirty CFD buffer 的 definition/diagnostics 仍正确。
- LSP CLI 测试和 crate 测试通过。

## Module 5: Lark Field Write Planner

### Problem

CSV/Excel field writes 使用 `coflow-loader-table-core::writer::plan_field_write`，但 Lark writer 手动解析 column 并直接渲染 `request.new_value`。这会让 nested collection 或 expanded object field write 的策略与 table-core 分叉。

### Plan

- Lark `write_field` 调用 table-core `plan_field_write`。
- 只在 Lark adapter 内处理 remote auth、spreadsheet validation、batch update IO。
- 删除或私有化不再需要的 `resolve_lark_column`。
- 补充 Lark writer 测试覆盖 planner 产生的多 cell update。

### Acceptance

- Lark field write 支持 table-core planner 的 SetCells 结果。
- token retry、source ownership guard 不变。
- Lark writer 测试通过。

## Commit Strategy

每个 module 单独提交：

1. `docs: add architecture simplification plan`
2. `test: split repo hygiene rules by domain`
3. `refactor: narrow runtime mutation interface`
4. `refactor: centralize runtime write target resolution`
5. `refactor: deepen lsp request contexts`
6. `refactor: reuse table planner for lark field writes`

每次提交前运行仓库要求的普通开发检查：

```powershell
cargo check --workspace
cargo test --workspace
```
