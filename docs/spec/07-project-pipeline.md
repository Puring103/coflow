# 项目运行时与命令编排规格

项目运行时负责把 `coflow.yaml`、schema、数据 source、data model、check 诊断和来源索引构造成可复用的 `ProjectSession`。CLI 在这个运行时之上执行导出、代码生成、产物暂存和提交。

本文保留原文件名是为了稳定文档链接；当前架构中不再存在独立的 `coflow-pipeline` crate。

---

## 边界

- `coflow-project`：读取和校验 `coflow.yaml`，解析项目根目录和项目相对路径，发现 schema 文件，提供项目初始化能力。
- `coflow-engine`：编译 schema，resolve/load sources，构建 `CfdDataModel`，运行 checker，聚合 `DiagnosticSet`，维护 `SourceIndex`、`RecordIndex` 和 `FileIndex`。
- 根 crate `coflow`：CLI 参数解析、命令编排、human/json 输出、export/codegen/build 产物安全检查、staging 和 commit。
- `coflow-builtins`：注册默认 provider。CLI、editor、LSP 通过它装配默认 `ProviderRegistry`。
- provider crates：实现 loader、writer、exporter、codegen；不依赖 engine、CLI、editor 或 LSP。

---

## 输入

- `coflow.yaml`
- 项目配置中发现的 CFT schema 文件
- 数据 source 定义
- CLI 命令和命令行覆盖项
- 宿主传入的 `ProviderRegistry`

---

## 配置发现和路径规则

命令的 `CONFIG_OR_DIR` 参数由 project 层统一解析：

- 未提供时，在当前目录查找 `coflow.yaml`，然后查找 `coflow.yml`。
- 参数是目录时，在该目录下查找 `coflow.yaml`，然后查找 `coflow.yml`。
- 参数是文件时，直接作为项目配置读取。

项目相对路径均以配置文件所在目录为根。`schema` 可以是单个精确小写 `.cft` 文件、单个目录或文件/目录列表；目录会递归发现精确小写 `.cft` 文件，忽略其他扩展名。schema 文件按 module id 排序后注册到 `CftContainer`，因此同一项目在不同文件系统遍历顺序下仍保持稳定。

`coflow.yaml` 的顶层和 outputs 容器使用严格字段集。source 只能使用通用字段 `type`、`path`、`url` 加 provider options；output 只能使用通用字段 `type`、`dir` 加 provider options。source 必须且只能设置 `path` 或 `url` 之一。旧字段 `file`、`dir` 和 `lark_sheet` 会在 YAML 反序列化阶段被拒绝。`columns` 映射拒绝重复 Excel header key，避免 YAML map 后写覆盖导致隐式丢配置。

---

## 命令阶段矩阵

| 命令 | Schema | 数据源存在性 | Data model | Check | 产物写入 |
| --- | --- | --- | --- | --- | --- |
| `cft check` | 是 | 否 | 否 | 否 | 否 |
| `lsp` | 是 | 否 | 否 | 否 | 否 |
| `check` | 是 | 是 | 是 | 是 | 否 |
| `build` | 是 | 是 | 是 | 是 | 是 |
| `export json/messagepack` | 是 | 是 | 是 | 是 | 是 |
| `codegen csharp` | 是 | 否 | 否 | 否 | 是 |

`Project::open_schema_only` 只打开配置和 schema 视图。需要数据源的命令由 engine 继续执行 data diagnostics、source resolve/load、model build 和 check。

---

## Engine 数据流

```text
Project::open_schema_only
  -> coflow-engine::build_project_session
  -> compile schema
  -> resolve configured sources
  -> provider.preflight
  -> provider.load
  -> collect input records + origins
  -> build SourceIndex / RecordIndex / FileIndex
  -> CfdDataModel::builder
  -> coflow-checker
  -> ProjectSession
```

`ProjectSession` 是共享运行时状态，不是 CLI 输出结构，也不是 editor UI session：

```rust
pub struct ProjectSession {
    pub project: Project,
    pub schema: CftContainer,
    pub model: CfdDataModel,
    pub diagnostics: DiagnosticsStore,
    pub sources: SourceIndex,
    pub records: RecordIndex,
    pub files: FileIndex,
    pub dependencies: DependencyIndex,
}
```

`DependencyIndex` 是 engine 自己的依赖视图。checker 可以在内部生成依赖图，但 `ProjectSession` 不直接暴露 checker crate 的具体类型。

engine 返回可定位诊断时不抛命令级错误。`Err(String)` 只表示配置读取、schema 文件读取等还不能稳定聚合进 `DiagnosticSet` 的不可恢复失败。

---

## Source Resolve

本地目录 source 没有显式 `type` 时，engine 会把目录交给所有注册 loader 的 `resolve` 阶段，各 loader 自己发现可处理的文件并返回 `ResolvedSource`。单文件或远端 URL source 先通过 registry probe 选择 loader；若多个 loader 同等匹配，则要求显式设置 `type`。

engine 只通过 `ProviderRegistry` 和 provider trait 调用 loader/writer/exporter/codegen，不依赖具体 provider crate。默认 provider 装配属于宿主层，当前由 `coflow-builtins::default_provider_registry()` 提供。

---

## 诊断处理

跨模块、engine 边界和宿主层流转只使用 `coflow_api::DiagnosticSet`：

```text
CftDiagnostics
  -> DiagnosticSet

CfdDiagnostics + RecordOrigin
  -> DiagnosticSet + DiagnosticLogicalLocation

provider / project / artifact diagnostics
  -> DiagnosticSet

DiagnosticSet
  -> CLI DiagnosticJson
  -> editor DiagnosticItem
  -> LSP Diagnostic + publishDiagnostics
```

`DiagnosticsStore` 在 canonical diagnostics 上建立 `by_stage`、`by_file` 和 `by_record` 索引。editor 需要的 `record_key` / `field_path` 由 engine 在映射 data-model/check 诊断时保存为 `DiagnosticLogicalLocation`，宿主层不重新猜测 record/path。

---

## 产物写入安全

产物写入属于 CLI 命令语义，不属于 engine。

所有会写产物的命令都在写入前执行可聚合诊断和 artifact preflight：

- `build`：先完成项目、schema、数据加载、data model、引用和 check；再检查 data output path；如果配置了 `outputs.code`，还会检查 C# codegen preflight 和 code output path。任一诊断存在时不写数据，也不写代码。
- `export json/messagepack`：先完成数据校验，再检查目标输出目录；有诊断时不写任何导出文件。
- `codegen csharp`：先完成 schema-only 校验、codegen 配置校验、schema 编译、codegen preflight 和 code output path 检查；有诊断时不读写 enum lockfile，不替换 C# 输出目录，也不生成新 `.cs` 文件。

artifact preflight 会检查输出目标是否能被 Coflow 安全接管，例如目标路径已经存在但不是目录、输出目录指向项目根或包含 schema/source、多个输出目录互相重叠。staging、commit、lockfile 读写和 artifact path 安全检查失败会返回 `DiagnosticSet`，使用 `SourceLocation::Artifact` 定位。

数据导出和 C# codegen 的输出目录由 Coflow 完全接管。写入阶段先创建同级 staging 目录并写入完整产物；所有文件成功写入后，再用 staging 目录替换目标输出目录。目标目录内旧文件、子目录、人工文件和其他工具产物均不会保留。

C# codegen 的 `coflow.enum.lock.json` 写在 `coflow.yaml` 同级，而不是 C# 输出目录内。codegen 会先读取并合并 lockfile，生成完整 C# staging 目录和 lockfile staging 文件；全部 staging 成功后再提交写入。

---

## 本地化配置

`coflow.yaml` 顶层支持可选 `localization` 段。完整规格见 [13-localization.md](13-localization.md)。

```yaml
localization:
  out_dir: "data/localization"
  languages:
    - "zh_CN"
    - "en"
```

`localization.out_dir` 默认 `data/localization`，相对项目根。`localization.languages` 列表中每项必须为合法 CFT 标识符且不为 `default`，列表内不允许重复。

未配置 `localization` 段时，engine 跳过翻译表生成；schema 中 `@localized` 字段仍保留语义，仅默认轮 check 会执行。

---

## 非职责

- `coflow-project` 不执行 loader、exporter 或 codegen。
- `coflow-engine` 不写导出目录、不渲染 CLI JSON、不依赖具体 provider。
- CLI 不重新实现 source resolve/load/model build/check。
- provider 不发现项目配置、不持有宿主层状态。
- `coflow-api` 不承载表格加载算法或导出遍历算法；这些分别属于 `coflow-loader-table-core` 和 `coflow-exporter-core`。
