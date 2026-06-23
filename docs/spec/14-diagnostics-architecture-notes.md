# Diagnostics Architecture Notes

本文记录 Coflow 诊断收束后的当前边界，以及后续维护必须遵守的规则。

## 当前结论

项目内部允许底层语言和数据模型保留适合自身实现的诊断结构，但跨 crate 运行时、宿主层和 provider 边界只使用 `coflow_api::DiagnosticSet` 作为 canonical diagnostics。

`DiagnosticJson` 已迁入根 crate `coflow`，只用于 CLI JSON 输出。editor 的 `DiagnosticItem` 只存在于 `editors/cfd-editor/src-tauri` 的前端通信边界。LSP Diagnostic JSON 只在 `textDocument/publishDiagnostics` 输出时生成。

## 诊断格式

### CFT 语言诊断

`coflow-cft` 使用 `CftDiagnostics` / `CftDiagnostic`，定位为 `module + span`。这适合 lexer/parser/schema/type checker 内部使用，进入 project 或 engine 边界时转换成 `DiagnosticSet`。

### 数据模型和 checker 诊断

`coflow-data-model` 使用 `CfdDiagnostics` / `CfdDiagnostic`，定位为 `record + field path`。checker 复用这套 record/path 诊断模型，不单独定义平行的 `CheckerDiagnostic`。

进入 engine 时，`CfdDiagnostics + RecordOrigin` 映射为：

```text
DiagnosticSet
DiagnosticLogicalLocation { record_key, field_path }
```

`DiagnosticSet` 负责最终可展示来源位置，`DiagnosticLogicalLocation` 服务 editor 的 record/field 定位。

### Canonical Diagnostics

`coflow-api` 提供唯一跨模块诊断格式：

```rust
pub struct DiagnosticSet {
    pub diagnostics: Vec<Diagnostic>,
}

pub struct Diagnostic {
    pub code: String,
    pub stage: String,
    pub severity: Severity,
    pub message: String,
    pub primary: Option<Label>,
    pub related: Vec<Label>,
}

pub struct Label {
    pub location: SourceLocation,
    pub message: Option<String>,
}
```

`SourceLocation` 覆盖本地文件 span、本地表格单元格、远程单元格、项目配置和产物路径。

## Host DTO

- CLI：`src/diagnostics.rs` 定义 `DiagnosticJson` / `RelatedJson`，由 `DiagnosticSet` 或 CFT schema build diagnostics 渲染而来。
- editor：`editors/cfd-editor/src-tauri/src/editor/session/diagnostics.rs` 把 `DiagnosticsStore` 转成 `DiagnosticItem`。
- LSP：`coflow-lsp` 在协议边界把 canonical diagnostics 转成 LSP Diagnostic 并发送 `publishDiagnostics`。

这些 DTO 不能进入 engine 内部状态，也不能成为 provider 或 project 的返回类型。

## DiagnosticsStore

`coflow-engine::DiagnosticsStore` 保存 canonical `DiagnosticSet`，并在其上建立索引：

```text
by_stage
by_file
by_record
logical_locations
```

索引用于 editor/LSP/CLI 查询和展示，但不改变诊断事实来源。新增诊断来源时，应先进入 `DiagnosticSet`，再让 store 重建索引。

## String 错误边界

`Result<_, String>` 只用于不可定位或无法聚合的命令级、I/O、协议、环境失败。能定位到 project/source/artifact 的用户可修复问题应优先进入 `DiagnosticSet`。

当前允许保留的 `String` 边界包括：

- project/config 文件无法读取或 YAML 无法反序列化。
- schema 文件 I/O 失败。
- artifact staging/commit 中无法稳定映射到更细来源的文件系统错误。
- LSP 协议读写错误。
- provider HTTP/API 的底层环境错误。

## 维护规则

- 不在 `coflow-project` 中新增 CLI JSON DTO。
- 不在 editor 中新增平行的 schema/load/check 诊断桶。
- 不在 LSP 中把项目诊断直接手写成 JSON 后绕过 `DiagnosticSet`。
- 不为 checker 新增独立诊断格式。
- provider 返回可定位错误时使用 `DiagnosticSet`。
- 共享运行时只保存 canonical diagnostics 和索引，不保存宿主层 DTO。
