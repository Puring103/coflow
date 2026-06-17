# 质量提升记录

本文档记录已经完成的主要质量提升修改，便于后续审查和持续推进。

## 2026-06-17

### 建立项目质量要求文档

新增 `docs/project-quality-requirements.md`，统一记录项目在代码质量、架构、文档、网页、测试、错误码覆盖、工程门禁和潜在 bug 排查方面的要求。

### 对齐 CI 与本地门禁

CI 增加 `cargo check --workspace`，与本地提交前要求保持一致。

### 修复 VS Code 插件 README 漂移

更新 `editors/vscode-coflow/README.md`：

- 名称从 CFT-only 调整为 Coflow language support。
- 本地打开目录修正为 `editors/vscode-coflow`。
- 语言服务命令修正为 `coflow lsp`。
- 配置项前缀修正为 `coflow.diagnostics.*`。
- 描述补充 `.cfd` 支持。

### 统一测试依赖版本

将根 crate 和 `coflow-pipeline` 的 `rust_xlsxwriter` dev-dependency 从 `0.64` 升级到 `0.95`，与 `coflow-loader-excel` 测试依赖保持一致，减少重复依赖树。

### 清理本地工具配置

删除已跟踪的 `.claude/settings.local.json`，并在 `.gitignore` 中忽略 `.claude/`。

### 明确生成物与 enum lockfile 边界

`.gitignore` 继续忽略 `examples/*/generated/**` 普通生成物，并显式保留
`examples/*/coflow.enum.lock.json`。`@keyAsEnum` 稳定值 lockfile 不再放在
C# 输出目录中，而是放在 `coflow.yaml` 同级，避免生成目录整体替换时误删
需要提交的稳定输入文件。

### 数据导出改为完整接管输出目录

`coflow-pipeline` 在写出 JSON 或 MessagePack 数据时先写入同级 staging 目录，
全部文件成功写入后再替换目标输出目录。目标目录由 Coflow 完整接管，旧 `.json`、
`.msgpack`、sidecar 文件、子目录和人工文件都会被移除，不再维护
`coflow.data.manifest.json`。

新增回归测试覆盖：

- stale `.json` 文件会被删除。
- stale `.msgpack` 文件会被删除。
- 非 Coflow 数据表扩展的旁路文件也会随目录替换删除。
- 不写入 `coflow.data.manifest.json`。

### C# codegen 改为完整接管输出目录

C# codegen 同样先写入同级 staging 目录，再替换目标输出目录。输出目录由
Coflow 完整接管，不再维护 `coflow.csharp.manifest.json`，也不会尝试保留
目录中的手写 `.cs` 或其他文件。手写扩展代码应放在生成目录之外，依赖生成类的
`partial` 扩展能力。

`coflow.enum.lock.json` 已移动到 `coflow.yaml` 同级，并作为单独的 staging 文件
提交。codegen preflight 有诊断时不读写 lockfile；staging 失败时旧输出目录和
旧 lockfile 保持不变；提交阶段目录替换失败时会尽力回滚已替换的 lockfile。

新增回归测试覆盖：

- stale `.cs` 文件和输出目录 sidecar 文件会被删除。
- C# 输出目录中不写 `coflow.csharp.manifest.json`。
- lockfile 写在项目配置同级，不在 C# 输出目录中。
- lockfile 位置同步覆盖 pipeline 与 CLI codegen 测试。

后续主动审查发现 `examples/humanpark/generated/csharp/coflow.enum.lock.json`
仍以旧位置被版本库追踪。该文件已迁移为
`examples/humanpark/coflow.enum.lock.json`，并新增仓库卫生测试，防止
`examples/*/generated/**` 下的生成产物重新进入版本管理。

### 明确数据源扩展名保持大小写敏感

项目数据源扩展名是大小写敏感的：只有精确小写 `.xlsx`、`.xlsm`、`.xls`
和 `.cfd` 会被识别。目录源会忽略 `.XLSX`、`.CFD` 等大小写不匹配的文件；
显式 `file` 指向这类文件时会报告 unsupported extension，避免跨平台规则漂移。

新增回归测试覆盖：

- 目录源中的 `.XLSX` workbook 会被忽略。
- 目录源中的 `.CFD` 文件会被忽略。
- 显式 `file: data/IGNORED.CFD` 会报告 unsupported extension。

### 修复 `contains(null, value)` 被误判为普通校验失败

`coflow-checker` 现在会把 `contains` 的非集合运行时输入报告为
`CFD-CHECK-EVAL-TYPE`，不会把 nullable collection 的 `null` 值当作
`false` 继续执行。这样错误的 check 规则或缺少空值 guard 的规则不会被降级为
普通 `CFD-CHECK-FAILED`。

同时，checker evaluator 对单参数内建函数和 enum 构造器增加严格 arity 防御，
避免未来绕过 CFT type checker 的调用静默忽略额外参数。

新增回归测试覆盖：

- nullable array 有值时，`contains(items, 1)` 正常通过。
- nullable array 为 `null` 时，触发 `CFD-CHECK-EVAL-TYPE`。
- `contains(null, value)` 不会产生普通 `CFD-CHECK-FAILED`。

### 修复多态路径引用无法访问子类字段

`coflow-data-model` 的路径引用现在会在字段下钻时读取当前值的实际记录类型，
再按实际类型查找字段元数据。这样当字段声明为父类或 abstract 类型，但实际值为子类时，
路径可以继续访问子类字段。

新增回归测试覆盖：

- `rewards: [Reward]` 中实际元素为 `ItemReward` 时，
  `@DropTable.table_1.rewards[0].item` 可以正确解析到 `ItemReward.item`。
- 解析结果继续保持目标字段类型校验，确保引用到的 `Item` 能赋给目标字段。

### 修复 Excel `@expand` 静默吞掉后续业务列

`coflow-loader-excel` 现在要求 `@expand` 后续被消费的相邻列必须连续，且表头
必须为空。如果相邻列写了任何非空表头，会在表头阶段报告 `EXCEL-COLUMN`，
避免普通字段列被静默当作展开子字段读取。Excel 合并表头的非左上角单元格
读取为空，因此是合法的 `@expand` 分组表头形式。

新增回归测试覆盖：

- merged-header 风格的空子列表头继续可用。
- 显式写出 `temperature`、`diffusion` 等子字段表头时会被拒绝。
- `id, env, level` 这类会吞掉 `level` 的布局会被拒绝，并定位到冲突表头列。
- `@expand` 相邻列不足仍会报告表头错误。

### 建立 `CfdErrorCode` 双向覆盖并清理不可达错误码

新增 `crates/coflow-checker/tests/error_coverage.rs`，对 `CfdErrorCode` 建立机械覆盖：
每个剩余错误码都必须有一个负向触发样例，以及一个相邻合法输入不误报样例。
测试还会解析 `coflow-data-model/src/diagnostic.rs` 中的枚举定义，防止新增错误码后漏补覆盖。

同时清理两个当前公共路径不可达的 CFD 运行期错误码：

- 移除 `RefTargetHasNoId`。当前 record key 由数据源提供，引用解析实际只会报告目标找不到。
- 移除 `CheckInvalidRegex`。非法 `matches` 正则字面量已经由 CFT type checker 在编译期报告；
  checker 内保留的防御分支降级为通用 `CheckEvalTypeError`。

规格同步更新：

- `CFD-REF-001` 现在对应实际可达的 `RefTargetNotFound`。
- `CFD-CHECK-*` 列表移除不可达的运行期正则错误码。

### 补齐 `CFD-TEXT-*` 错误码双向覆盖

`coflow-loader-cfd` 新增集中测试，覆盖所有 `.cfd` 文本加载错误码的负向触发和相邻合法输入。

覆盖的错误码包括：

- `Syntax`
- `UnknownType`
- `AbstractObjectType`
- `ObjectTypeMismatch`
- `UnknownField`
- `DuplicateField`
- `ReservedIdField`
- `TypeMismatch`
- `InvalidEnumVariant`
- `ReferenceNeedsMarker`

每个错误码都有对应的合法相邻输入，确认 parser 不会把正常记录、合法多态记录、
合法字段、合法 enum、合法引用等场景误报为错误。

### 补齐 CFT 错误码双向覆盖

`coflow-cft` 的错误码覆盖测试从“每个错误码都能触发”升级为双向覆盖：

- 每个 `CftErrorCode` 都保留负向触发样例。
- 每个样例都增加贴近原错误的合法相邻输入。
- 测试会确认这些合法输入可以完成对应阶段解析和编译，避免 lexer、parser、
  schema compiler 或 type checker 把近似合法场景误报为错误。

覆盖范围包括 CFT lexer、syntax parser、schema compiler 和 check 表达式 type checker。

### 补齐 cell value 错误码双向覆盖

`coflow-cell-value` 新增集中错误码覆盖矩阵，解析 `CellValueErrorCode` 枚举并要求每个错误码都有：

- 一个负向触发样例。
- 一个贴近该错误的合法相邻输入。

覆盖范围包括声明类型解析、未知类型、对象字段、嵌套边界、类型不匹配、多态对象、
enum、字符串引用歧义和记录引用标记提示。这样新增单元格错误码时，测试会强制补齐
负向触发和正向不误报两个方向。

### 修复 Excel `@expand` 子字段诊断定位

Excel loader 的 origin 映射现在记录 `@expand` 子字段到实际 Excel 列的关系。
当 data model 对 `env.temperature` 这类展开子字段报告 `MissingRequiredField`
或类型错误时，诊断会定位到子字段所在列，而不是只定位到 `env` 父列。

新增回归测试覆盖：

- `@expand env` 的 `temperature` 子列为空时，`CFD-DATA-006` 定位到
  `temperature` 子列。

### 简化项目介绍网页

将 `docs/spec/11-project-architecture.html` 从复杂架构图页面调整为四板块介绍页：

- 项目介绍以及想解决的问题。
- 项目如何解决这些问题。
- 项目的核心架构。
- 简单的示例。

页面内容与当前 CLI、CFT、Excel/CFD、data model、checker、exporter 和 C# codegen
实现保持对应，作为开发文档之外的轻量项目入口。

### 拆分 C# codegen 入口文件测试模块

`crates/coflow-codegen-csharp/src/lib.rs` 曾因内嵌大量 unit tests 超过 1500 行。
现在生产入口只保留公开 API 和模块声明，原有 34 个 C# codegen 行为测试移动到
`crates/coflow-codegen-csharp/src/tests.rs`，仍作为 crate 内部测试运行并保留对私有
IR/render helper 的覆盖。

这样 `lib.rs` 不再进入超大生产源码治理清单，同时不改变 C# codegen 的公开 API、
生成语义或测试覆盖范围。

### 修复 LSP 空路径 `file://` URI 处理

LSP 的 `path_from_file_uri` 以前会把 `file://`、`file://localhost` 和
`file://server` 这类没有 path 部分的 URI 解码成空 `PathBuf`。这会让 malformed
`didOpen` 通知进入打开文档状态，并触发多余的诊断发布。

现在 URI 解析会拒绝空路径的 file URI，同时保留正常本地路径、Windows drive、
localhost 路径和 UNC 路径处理。新增回归测试覆盖解析器本身，以及 LSP handler
忽略这些 malformed `didOpen` 通知、不产生额外输出的行为。

### 修复 VS Code diagnostics 开关作用域

VS Code 插件的 `coflow.diagnostics.enabled` 以前只在文档打开、修改和保存事件中
阻止主动校验。若用户在关闭 diagnostics 后触发 hover、completion、definition 等
LSP-backed 语言功能，共用的 LSP session 仍会接收并写入 `publishDiagnostics`。

现在插件在处理 `textDocument/publishDiagnostics` 和本地 language-server failure
diagnostic 时按诊断 URI 重新读取 `coflow.diagnostics.enabled`，只过滤诊断发布，
不关闭补全、hover、definition、formatting 和 semantic tokens。新增插件单元测试
覆盖全局关闭、资源作用域关闭和本地失败诊断三种场景。

### 修复 LSP malformed `didClose` 诊断清理

LSP 的 `didOpen` 和 `didChange` 已会忽略 malformed file URI，但 `didClose` 以前即使
URI 不能解析，也会对该原始 URI 发布空 diagnostics 并重新校验项目。这会让无效 URI
通知产生额外 LSP 输出，和其它文档同步通知行为不一致。

现在 `didClose` 只有在 URI 能解析为 file path 时才移除打开文档、清空对应诊断并触发
重新校验；malformed URI 会被直接忽略。回归测试覆盖 `not-a-file-uri` close 通知不产
生额外消息。

### 将 VS Code 插件单测纳入 CI

项目质量要求包含 VS Code 插件客户端适配，但 CI 以前只执行 Rust workspace 的
check、fmt、clippy 和 test。已有的 `editors/vscode-coflow/test/extension-unit.test.js`
没有纳入自动门禁，插件行为回归只能靠手动运行发现。

现在 GitHub Actions 会安装 Node 22 并运行 VS Code 插件单测，覆盖插件配置解析、
LSP 结果转换、无本地语义 fallback、diagnostics 开关和 semantic token legend 等
客户端适配边界。

### 明确混合目录 source 的 Excel/CFD 配置语义

主动排查项目配置边界时确认 `examples/rpg` 使用同一个目录 source 同时加载
Excel workbook 和 CFD 文本文件。该模式下 `sheets` 只作用于目录内的 Excel
workbook，目录内的 CFD 文件仍按文本中的记录类型加载；只有单个 `.cfd` 文件
source 不应配置 `sheets`。

新增 pipeline 回归测试覆盖带 `sheets` 的混合目录 source 可以同时加载 Excel
和 CFD 文件，避免后续把 RPG 示例的合法配置误判为非法组合。README 和 CLI
规格同步补充该语义。

### 明确 schema 和 LSP 扩展名保持大小写敏感

schema 目录递归发现只接受精确小写 `.cft` 文件，VS Code 插件的本地 schema
扫描也遵守同一规则；大写或混合大小写的 `.CFT`、`.Cft` 文件会被忽略。
显式 `schema: path/to/file` 也必须指向精确小写 `.cft` 文件，否则在
schema-only 配置诊断阶段报告 unsupported extension。CFD LSP 能力同样只对
精确小写 `.cfd` 启用，`.CFD` 不会触发 CFD 语义能力。

新增回归测试覆盖：

- `coflow-project` 忽略 `MAIN.CFT` 和 `EXTRA.Cft`。
- `coflow-project` 拒绝显式 `schema: schema/MAIN.CFT`。
- VS Code schema 目录扫描忽略 `ignored.CFT`。
- LSP 对 `data.CFD` 的 CFD definition 请求返回空结果。

项目管线和 CLI 规格同步说明 schema 目录发现规则。

### 收敛并移除旧 `coflow cft lsp` 入口

CLI 已提供 root `coflow lsp` 作为 Coflow language server 入口，VS Code 插件也
默认使用该命令，但 CLI 规格和 README 常用命令仍主要展示旧的
`coflow cft lsp`。现在 CLI 规格只保留 `coflow lsp [CONFIG_OR_DIR]` 入口，
README 常用命令改为 `cargo run -- lsp examples/rpg`，并从 CLI 中移除旧的
`coflow cft lsp` 兼容子命令，避免两个入口长期漂移。
