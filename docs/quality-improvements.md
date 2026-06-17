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

### 修复数据源扩展名大小写敏感

pipeline 现在以大小写不敏感方式识别 `.xlsx`、`.xlsm`、`.xls` 和 `.cfd` 数据源。
这样显式文件源和目录源都可以加载 `.XLSX`、`.CFD` 等来自 Windows 或外部工具的
常见大写扩展名。

新增回归测试覆盖：

- 目录源中的 `.XLSX` workbook 会被加载。
- 目录源中的 `.CFD` 文件会被加载。
- 显式 `file: data/SINGLE.CFD` 会被加载。

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
