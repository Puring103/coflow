# 项目架构

本页说明 Coflow 仓库的核心模块、运行时数据流和职责边界。它不是入门教程，而是给需要理解内部结构、扩展 Provider、维护 CLI/编辑器/LSP 的开发者使用。

## 数据处理流程

Coflow 的主线是数据处理：CFT 定义数据形状，数据源提供记录，Provider 把不同来源转成统一输入，DataModel 负责合并、补默认值和解析引用，check 通过后再导出数据和生成运行时代码。

```mermaid
flowchart TD
  Project["项目入口<br/>coflow.yaml / schema paths / sources"] --> Schema["CFT Schema<br/>类型 / 默认值 / check"]
  Project --> Sources["数据源<br/>Excel / CSV / CFD / Lark / 维度隐式 source"]
  Sources --> Providers["Loader Providers<br/>解析不同来源"]
  Providers --> Records["Input Records<br/>来源无关"]
  Schema --> Model
  Records --> Model["CfdDataModel<br/>合并 / 默认值 / 引用"]
  Model --> Check["coflow-checker<br/>业务校验"]
  Check --> Generation["Runtime generation<br/>可信模型 + 诊断 + 索引"]
  Generation --> Queries["ProjectQueries<br/>generation-bound read capability"]
  Generation --> Write["WriteProjectSession<br/>registry + revision + mutation"]
  Queries --> Outputs["输出与消费<br/>check / editor / JSON / MessagePack / C#"]
  Write --> Providers
```

`coflow.yaml`、路径解析、Provider registry 和宿主命令都服务于这条数据主线。runtime 内部拥有处理完成后的完整 generation；CLI、编辑器和自动化命令只通过 capability session 复用它，而不是各自重新实现 schema/data/check 管线。

内部 generation 保存共享运行时状态：

```text
Runtime generation
  project        # 项目配置、根目录和路径信息
  schema         # 编译后的 CFT schema
  model          # 构建后的 CfdDataModel
  diagnostics    # 结构化诊断
  sources        # source 索引
  records        # record 索引
  files          # 文件索引
```

拥有该 generation 的 session 不属于 public interface。`ProjectQueries` 提供只读 interface；`ReadOnlyProjectSession`、`BuildProjectSession` 和 `WriteProjectSession` 分别表达无副作用读取、允许维度生成的构建、以及持有 registry/revision 的 mutation 能力。`check` 到这里结束；`build`、`export` 和 `codegen` 会在 generation 有效后继续执行产物 preflight、写入不可变 artifact generation，并原子激活一个 manifest snapshot。

## 分层职责

```mermaid
flowchart TD
  Hosts["宿主层<br/>CLI / Editor / LSP"] --> Runtime["项目运行时<br/>coflow-project / coflow-runtime"]
  Runtime --> Core["核心模型<br/>coflow-cft / coflow-data-model / coflow-checker"]
  Runtime --> Registry["Provider 装配<br/>coflow-builtins / ProviderRegistry"]
  Registry --> Providers["Provider 实现<br/>loaders / writers / exporters / codegens"]
  Providers --> API["公共接口<br/>coflow-api"]
```

这张图按自上而下的调用关系阅读：宿主调用项目运行时，运行时使用核心模型和 Provider registry；具体 Provider 通过 `coflow-api` 的接口接入。

## Crate 边界

| Crate | 职责 |
| --- | --- |
| `coflow-api` | Provider traits、diagnostics、source locations、artifacts、writer contracts |
| `coflow-project` | 读取和校验 `coflow.yaml`、路径解析、schema 文件发现、项目初始化 |
| `coflow-runtime` | schema 编译、source resolve/load、DataModel、check、索引、维度文件注入 |
| `coflow-builtins` | 注册默认 Provider registry |
| 根 `coflow` crate | CLI 参数解析、命令编排、human/JSON 输出、artifact generation staging 和 manifest publication |
| `coflow-cft` | CFT parser、schema compiler、check 表达式静态类型检查 |
| `coflow-cfd` | 唯一 CFD syntax parser、canonical AST 和 source spans |
| `coflow-data-model` | record/object/value 模型、默认值、引用、索引和 DataModel 诊断 |
| `coflow-loader-table-core` | Excel/CSV/Lark 共享表格加载、表头协调和单元格值解析 |
| `coflow-loader-*` | 具体数据源 loader/writer |
| `coflow-exporter-core` | JSON/MessagePack 共享导出遍历规则 |
| `coflow-exporter-*` | 具体数据导出格式 |
| `coflow-codegen-csharp` | C# 运行时代码生成 |
| `coflow-checker` | CFT `check {}` 运行期执行 |
| `coflow-lsp` | CFT/CFD language server |
| `editors/cfd-editor/src-tauri` | 编辑器后端宿主，复用 runtime 和 writer |

## 关键模块

### `coflow-project`

`coflow-project` 只负责项目入口和路径：

- 发现 `coflow.yaml` / `coflow.yml`。
- 解析项目根目录。
- 校验 `schema`、`sources`、`outputs`、`dimensions` 配置形状。
- 展开 schema 文件列表。
- 初始化最小项目骨架。

它不加载数据、不构建 DataModel、不调用 exporter 或 codegen。

### `coflow-runtime`

`coflow-runtime` 是共享项目运行时。它把 project、schema、source、DataModel、diagnostics 和索引组织成不可被宿主拆开的 generation，并通过 capability session 暴露用途明确的 interface。

主要职责：

- 编译 CFT schema。
- 注入维度合成 type。
- resolve / preflight / load sources。
- 构建统一的 `CfdDataModel`。
- 建立 source、record、file 索引。
- 执行引用解析和 `coflow-checker`。
- 聚合结构化诊断。
- 规划并原子发布 source mutation transaction。

runtime 返回诊断，不负责最终的 CLI 输出格式，也不负责替换导出目录。

### `coflow-api`

`coflow-api` 是 Provider 和宿主之间的公共边界。它定义：

- loader / writer / exporter / codegen traits。
- Provider descriptor。
- 诊断结构和 source location。
- artifact 输出契约。
- write patch / write outcome 和 source transaction compensation contract。
- opaque `DecodedSourceOptions` 及 provider-owned option decoding contract。

共享表格算法、导出遍历算法和项目生命周期不放在 `coflow-api`，避免 API crate 变成实现集合。

`SourceConfig.options` 是 project adapter 的 raw 输入，不属于运行时 source contract。
`ResolvedSource` 只保存 provider 解码后的 opaque typed options；loader、writer 和
table manager 必须读取本 provider 的具体 option 类型，provider identity 或类型
不匹配会产生 contract diagnostic。provider option value 不通过 source index 或
Debug 输出暴露给 host。

### Mutation transaction

`WriteProjectSession` 是唯一 mutation interface。一次请求按以下顺序执行：

1. planner 解析所有操作，并用 pending-record overlay 折叠 `insert -> set/rename/delete` 等批内依赖。
2. 所有可执行的 provider preflight 在任何写入前完成。
3. 每个本地 path source 由 runtime 保存原始字节；每个远程 source 必须返回 provider-owned compensation handle，否则整批在写入前拒绝。
4. `writes::stage` 只执行 provider I/O，不 rebuild session。
5. 全部来源写入后构建一个候选 generation。加载、schema 或 DataModel 错误会触发 compensation；业务 `CHECK` 诊断属于可发布结果。
6. provider transaction 全部 commit 后替换旧 generation，并且整批只推进一次 revision。

stage、rebuild 或 commit 失败时，runtime 按逆序补偿远程来源并恢复本地字节，旧 generation 继续为查询提供一致视图，报告中的 `applied` 为空。远程 writer 若没有可靠补偿能力，必须显式声明 `Unsupported`；不能用“尽力回滚”伪装成原子写入。

## Provider Registry

Provider registry 持有 loader、writer、exporter 和 codegen。默认 registry 由 `coflow-builtins` 组装。

| 类别 | Provider id |
| --- | --- |
| loader/writer | `excel` |
| loader/writer | `csv` |
| loader/writer | `cfd` |
| loader/writer | `lark-sheet` |
| exporter | `json` |
| exporter | `messagepack` |
| codegen | `csharp` |

runtime 只依赖 registry 和 trait，不依赖具体 Provider crate 的实现细节。扩展新数据源、导出格式或代码生成目标时，应优先通过 Provider 接口接入。

本地文件和远程 URI 的 table operation 共用 runtime preparation interface：匹配
项目 source、选择 provider、复用 decoded options、校验 provider role，然后调用
`TableManager`。CLI 不包含 Excel/Lark 分支；新增远程表格 provider 不需要修改宿主。

表头同步的列身份、增删集合和旧列到新列的行投影由
`coflow-loader-table-core::writer::HeaderReconciliationPlan` 一次计算。CSV、Excel
和 Lark adapter 只执行该计划，不各自实现列匹配。重复的空表头槽位按出现次序
匹配，因此 `@expand` 字段在新增、删除或重排其他字段后仍保持原数据绑定。远程
表格同步会覆盖旧、新表头的最大矩形宽度，确保被删除的尾列不会继续残留数据。

Lark loader、writer 和 table manager 由 `lark_provider_roles` 创建，并共享一个
`LarkRemote`。该深模块统一拥有 tenant token、wiki URL 解析、sheet metadata、
HTTP method 标记和 token-expiry retry。凭证缓存按 app id 与 secret 的 SHA-256
指纹共同隔离，document metadata 也按凭证隔离；secret、指纹和 access token
不会进入 Debug 输出。每个远程请求最多在明确的 token-expired 响应后重试一次，
transport diagnostic 始终包含 HTTP method、operation 和 retry 阶段。

Excel source provider 的 read descriptor 接受 `.xlsx`、`.xlsm` 和 `.xls`，但
writer capability 与 table-operation descriptor 只对 `.xlsx` 开放。Runtime 查询
每个 `ResolvedSource` 的动态 capability，不再把 provider 级静态 capability 套用
到所有格式；writer 和 table manager 入口仍执行相同的格式预检。

## 数据模型边界

Loader 输出 source-neutral input records。它们只表达“某个来源读到了哪些记录和值”，不直接变成导出产物。

`coflow-exporter-core` 借用 `CompiledSchema` 的 field metadata，并把每个 table 转成 path-aware `begin/end/key/scalar` 事件流。JSON 和 MessagePack sink 直接顺序写入最终 table buffer，不构造跨 table value tree，也不为 scalar/child aggregate 分配临时编码 buffer；sink 错误由 core 补充 table、record key 与完整字段路径。

CFD loader 是 schema-guided lowering adapter，而不是另一套文本 parser。CFD
文本只由 `coflow-cfd` 解析一次，得到 canonical AST；`coflow-loader-cfd` 消费
该 AST，完成类型、字段、多态、引用和 dict key 转换。语言工具和数据加载因此
共享相同的 token、恢复规则和 source span。

DataModel 统一处理：

- 顶层 record key。
- 默认值。
- 必填字段。
- 字段类型匹配。
- 多态对象可赋值性。
- dict key 唯一性。
- `&Type` 记录引用。
- 继承索引。
- `@singleton` 约束。

因此 Excel、CSV、CFD 和飞书/Lark 的数据最终使用同一套规则。Provider 不应该各自实现业务校验。

## 宿主边界

### CLI

根 `coflow` crate 是 CLI 宿主，负责：

- 解析命令行参数。
- 调用 project / runtime。
- 将 diagnostics 渲染为 human 或 JSON。
- 编排 `check`、`build`、`export`、`codegen`。
- 执行 artifact preflight。
- 验证并封存不可变 artifact generation，再原子发布 active manifest。

active manifest 同时选择 data、code generation 和 `@idAsEnum` lock state。它的发布属于 CLI 宿主职责，不放进 runtime；旧 generation 不参与回滚写入，失败时只需保持旧 manifest 未变。根目录 `coflow.enum.lock.json` 是成功激活后的版本化镜像，不参与 active snapshot 选择。

### 编辑器

每个 `EditorSession` 持有一个 `WriteProjectSession`、wire diagnostics 索引和
`RevisionCoordinator`。编辑器后端复用 `coflow-runtime`：

- 通过 generation-bound `ProjectQueries` 读取项目。
- 展示 source/file/record 索引。
- 展示表格视图、记录视图和关系视图。
- 只通过 `WriteProjectSession` mutation interface 写回数据。
- 使用同一套 diagnostics。

reload 会先取得当前 revision ticket，在 session 锁外构建完整候选 generation，
再在短写锁内比较 ticket。只有基准 revision 仍然匹配的候选才能替换 session；过期
候选被丢弃并重新构建，因此较慢的文件 reload 不能覆盖较新的内部写入。

内部写入推进 revision，并记录每个实际写入路径的 SHA-256 内容指纹。文件 watcher
只有在路径和当前内容都匹配该指纹时才把事件归因于内部写入；之后发生的外部修改会
立即触发 reload，不依赖固定时间窗口。所有 snapshot、mutation outcome 和 diagnostics
都携带 revision；前端只接收最新 revision，并在异步 file/table/graph 查询返回时再次
核对 revision，防止旧缓存覆盖新状态。

编辑器 diagnostics 以 `(file, RecordCoordinate)` 建立索引。表格和关系视图按 record
直接读取相关诊断，不复制或扫描完整诊断列表。编辑器不应绕过 runtime mutation
interface 直接调用 writer 或修改 Provider 数据文件。

### LSP

LSP 是 schema-only/text language server，不要求数据源文件存在，重点提供 CFT/CFD 的
诊断、补全、hover、跳转、符号和语义高亮。`ValidationCore` 持有 open document、
单调 revision 和最近一次不可变 `ValidationSnapshot`；snapshot 将 schema build、
typed CFD definitions、diagnostics、document version 和 active URI 绑定在同一 revision。

文档变化只把不可变 `ValidationInput` 交给后台 worker。worker mailbox 会用较新的输入
替换尚未开始的旧输入；已在运行的旧 build 可以完成，但 `commit_snapshot` 只有在候选
revision 等于当前 revision 时才发布。需要 schema snapshot 的 feature request 会排队到
当前 revision 提交后再执行，因此旧 diagnostics、definition index 或 semantic state
不能与新文档混用。CFD definitions 和语言功能统一消费 `coflow-cfd` canonical AST，
LSP 不维护第二套 CFD parser。

## 维度与本地化

`@localized` 字段属于 `language` 维度。runtime 会：

1. 扫描 schema 中的维度字段。
2. 为每个字段注入合成 type。
3. 根据 `dimensions.language.out_dir` 注册隐式 source。
4. 维护维度数据文件。
5. 在 check 阶段按变体运行校验。

维度文件进入普通 source/model/check 流程，而不是单独的外部覆盖表。

## 产物安全

所有写产物的命令都遵循同一原则：

- 有诊断时不写产物。
- 写入前做 artifact preflight。
- 在同级 staging 目录写入、同步并回读验证。
- 把完成目录封存为不可变 generation。
- 只通过一个 active manifest 原子激活完整 snapshot。

这样可以避免半生成目录被运行时误读；任一文件系统操作失败时，active state 仍是旧或新完整 generation。artifact preflight 还会解析已存在祖先的真实路径，避免输出经 symlink/junction 落进项目根、schema 或 source 目录。

## 非职责

| 模块 | 不负责 |
| --- | --- |
| `coflow-project` | 不加载 source，不构建 DataModel |
| `coflow-runtime` | 不渲染 CLI 输出，不发布 artifact manifest |
| CLI | 不重新实现 source resolve/load/model/check |
| Provider | 不发现项目配置，不持有宿主状态 |
| `coflow-api` | 不承载表格加载算法或导出遍历算法 |
