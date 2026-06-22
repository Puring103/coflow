# Diagnostics Architecture Notes

本文记录 2026-06-22 对 Coflow 当前错误收集、诊断复用性和一致性的讨论结论。重点不是立即改代码，而是给后续 `coflow-engine` / diagnostics 收敛设计提供输入。

## 当前判断

核心诊断模型已经有雏形，但复用只做到 pipeline/provider 层。CLI、Editor、LSP 的收集、转换和渲染仍明显分叉。

## 已经做得较好的部分

- `coflow-api` 已有统一的 `DiagnosticSet`、`Diagnostic`、`Severity`、`SourceLocation`。
- provider、pipeline、artifact/codegen 大多已经返回 `DiagnosticSet`，pipeline 通过 `PipelineOutcome::Diagnostics` 聚合可定位错误。
- data model / checker 诊断可以通过 record origin 映射到文件、表格或远程单元格：
  - `coflow-api::map_diagnostics_with_origins`
  - `coflow-data-model::map_diagnostics`
  - `RecordOrigin::{File, Table}`
- Excel 和 Lark 的表格来源已经在 origin 层趋向统一，具备继续收敛的基础。

## 主要问题

### Editor 是平行诊断管线

`coflow-editor-core` 内部维护自己的：

- `Diagnostics { schema, load, check }`
- `DiagnosticItem`
- `diagnostic_from_api`
- `diagnostic_from_project`
- `diagnostic_from_cfd`
- `diagnostic_from_cft_schema`

这说明 editor 并没有真正复用 canonical `DiagnosticSet`。它把 schema/load/check 诊断在 editor core 内重新分类、转换和扁平化，容易和 CLI/pipeline 的行为漂移。

### `DiagnosticJson` 位置偏内部化

`coflow-project` 现在定义 `DiagnosticJson`，CLI 再用它做人类可读输出和 JSON 输出。

这让 `coflow-project` 同时承担两类职责：

- 项目配置、路径、schema 文件发现。
- CLI JSON DTO / human rendering 的中间结构。

更清晰的边界应是：`coflow-project` 只产生 canonical `DiagnosticSet`，CLI JSON 是 CLI renderer 的输出 DTO。

### LSP 直接产出 JSON

`coflow-lsp` 里有自己的 `lsp_diagnostic`，而 `coflow-lsp/src/cfd/mod.rs` 的 CFD syntax diagnostics 直接返回 `Vec<serde_json::Value>`。

这会带来几个问题：

- LSP severity、source 字段和 range 逻辑会独立演化。
- CFD syntax diagnostics 和 CFT/project diagnostics 不走同一条映射链。
- 后续 editor 如果需要复用 language diagnostics，很难共享。

### 位置模型有重复

`coflow-api::SourceLocation` 和 `coflow-data-model::SourceLocation` 语义接近，目前靠转换桥接。现在可以工作，但长期会增加映射成本。

后续如果引入 `coflow-engine`，应尽量让 engine 内部只保存一种 canonical location 表达，host 层再做渲染。

### 仍有部分错误是 `String`

`Project::open_schema_only`、YAML 解析、配置文件读取、部分 compile/open 错误仍返回 `Result<_, String>`。

这在 CLI 里可以输出，但在 Editor/LSP 中会降级成普通错误，无法和其它诊断一起按文件、stage、source、record 聚合。

## 推荐收敛方向

### 1. 保留唯一 canonical diagnostics

内部流转只认：

```rust
coflow_api::DiagnosticSet
coflow_api::Diagnostic
coflow_api::SourceLocation
```

`DiagnosticJson`、`DiagnosticItem`、LSP diagnostic JSON 都应是 host renderer 的输出，不参与 engine/pipeline/editor 内部业务流。

### 2. Editor 内部改存 canonical diagnostics

Editor 可以继续返回当前前端需要的 `DiagnosticItem`，但边界应调整为：

```text
ProjectSession / engine
  stores DiagnosticSet

editor command boundary
  DiagnosticSet -> Vec<DiagnosticItem>
```

也就是说：

- editor session 内部不要维护平行的 `Diagnostics { schema, load, check }` DTO。
- 如果需要按 stage 分组，使用 `Diagnostic.stage` 或 diagnostics store 的索引。
- `DiagnosticItem` 只作为 Tauri/TS wire shape。

### 3. CLI renderer 从 project 中拿出来

建议把 `DiagnosticJson` 从 `coflow-project` 的核心职责中移走，至少语义上改为 CLI renderer 输出。

目标边界：

```text
coflow-project
  Project::schema_diagnostic_set() -> DiagnosticSet
  Project::data_diagnostic_set() -> DiagnosticSet

coflow-cli
  DiagnosticSet -> JSON output DTO
  DiagnosticSet -> human stderr output
```

### 4. LSP diagnostics 先转 canonical，再渲染

CFD syntax diagnostics 不应直接返回 JSON。

推荐路径：

```text
CFD parser diagnostics
  -> DiagnosticSet
  -> LSP publishDiagnostics JSON
```

这样 CFT、CFD、project diagnostics 都能走同一个 LSP renderer。

### 5. `coflow-engine` 应包含 DiagnosticsStore

后续如果引入 `coflow-engine`，diagnostics store 应该是核心能力之一：

```rust
pub struct DiagnosticsStore {
    diagnostics: DiagnosticSet,
    by_stage: BTreeMap<String, Vec<DiagnosticId>>,
    by_file: BTreeMap<PathBuf, Vec<DiagnosticId>>,
    by_record: BTreeMap<String, Vec<DiagnosticId>>,
}
```

底层保存 canonical diagnostics，索引服务 editor/LSP 的查询和 UI 分组。

### 6. 区分可聚合诊断和命令级失败

不是所有错误都必须立刻变成 `DiagnosticSet`。可以保留两类错误：

- 可定位、用户可修复的问题：进入 `DiagnosticSet`。
- 进程级或环境级失败：返回 command/session error。

但边界要明确。比如配置文件找不到、YAML parse error、权限错误，如果能定位到文件或配置位置，长期应优先进入 `DiagnosticSet`。

## 与架构重构的关系

如果做 `coflow-engine`，diagnostics 应该和 project lifecycle 一起收敛，否则 editor/LSP 会继续各自维护错误收集路径。

建议 engine 首版就负责：

- schema diagnostics 聚合。
- source resolve/load diagnostics 聚合。
- data model/check diagnostics origin mapping。
- write diagnostics 聚合。
- diagnostics store 和基础索引。

CLI、Editor、LSP 只负责从 `DiagnosticSet` 渲染到自己的输出形态。

## 一句话结论

当前错误模型的核心已经基本正确，但 host 层重复转换太多。下一轮重构应把 `DiagnosticSet` 提升为真正唯一的内部诊断格式，并把 CLI JSON、Editor `DiagnosticItem`、LSP diagnostic JSON 都降级为边界渲染结果。
