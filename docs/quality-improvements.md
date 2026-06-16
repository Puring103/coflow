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

### 明确生成目录 lockfile 例外

`.gitignore` 中显式保留 `examples/*/generated/csharp/coflow.enum.lock.json`，避免普通生成物忽略策略与被跟踪 lockfile 策略冲突。

### 修复数据导出旧表文件残留

`coflow-pipeline` 在写出 JSON 或 MessagePack 数据前会根据 `coflow.data.manifest.json`
清理上一轮生成、但本轮不再生成的 `.json` / `.msgpack` 表文件。这样删除或
重命名 schema 表后，旧数据文件不会继续留在输出目录并被消费者误用。

新增回归测试覆盖：

- stale `.json` 文件会被删除。
- stale `.msgpack` 文件会被删除。
- 非 Coflow 数据表扩展的旁路文件会保留。

### 增加产物 manifest，避免误删或覆盖非 Coflow 文件

数据导出和 C# codegen 现在分别维护 `coflow.data.manifest.json` 与
`coflow.csharp.manifest.json`。写入前只清理上一轮 manifest 中存在、但本轮
不再生成的产物；如果输出目录中存在未被 manifest 管理的 `.json`、`.msgpack`
或 `.cs` 文件，命令会拒绝写入，而不是按扩展名直接删除或覆盖。

新增回归测试覆盖：

- 有 manifest 的旧数据表文件会被清理，并刷新 manifest。
- 未被 manifest 管理的 `.json` 文件会阻止导出，原文件保留。
- 未被 manifest 管理的 `.cs` 文件会阻止 C# codegen，原文件保留。

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

`coflow-loader-excel` 现在要求 `@expand` 后续被消费的相邻列表头必须为空，
或显式写成预期子字段名。如果相邻列写了其他业务表头，会在表头阶段报告
`EXCEL-COLUMN`，避免普通字段列被静默当作展开子字段读取。

新增回归测试覆盖：

- merged-header 风格的空子列表头继续可用。
- 显式写出 `temperature`、`diffusion` 等子字段表头时可正常加载。
- `id, env, level` 这类会吞掉 `level` 的布局会被拒绝，并定位到冲突表头列。
- `@expand` 相邻列不足仍会报告表头错误。
