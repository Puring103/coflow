# 设计缺陷与后续处理清单

本文档记录质量梳理过程中发现的设计级问题。这里的问题不一定需要在当前批次立即修复，但后续重构应持续消化。

## Rust LSP 与 VS Code 插件存在语义重复（已修复）

Rust LSP 已实现补全、hover、definition、document symbol、formatting 和 semantic tokens。VS Code 插件曾保留较多本地解析、符号收集和语义 fallback 逻辑。

影响：

- CFT/CFD 语义规则变更后，Rust 和 JS 两套实现可能出现行为漂移。
- 插件维护成本高，测试需要覆盖重复逻辑。
- 用户遇到 LSP 启动失败时，插件 fallback 可能给出与真实语言服务不同的结果。

处理结果：

- Rust LSP 作为唯一语义事实来源。
- VS Code 插件只保留 LSP 进程管理、协议转换、错误展示、配置发现和最小降级提示。
- completion、hover、document symbol 和 definition provider 不再使用本地语义 fallback。
- 插件单元测试覆盖 LSP 无响应时返回空结果，而不是返回 JS 本地解析结果。

## 部分生产源码文件过大

当前重点文件包括：

- `crates/coflow-lsp/src/lib.rs`
- `editors/vscode-coflow/src/extension.js`
- `crates/coflow-codegen-csharp/src/lib.rs`
- `crates/coflow-data-model/src/compiler.rs`
- `crates/coflow-cft/src/parser.rs`
- `crates/coflow-cft/src/schema/compiler.rs`
- `crates/coflow-codegen-csharp/src/emit.rs`
- `crates/coflow-checker/src/check/evaluator.rs`
- `crates/coflow-loader-excel/src/lib.rs`
- `crates/coflow-loader-cfd/src/lib.rs`

影响：

- 单文件承载过多职责，代码审查和定位问题成本较高。
- 模块边界不够清楚，难以建立更细粒度测试。

建议方向：

- LSP 拆分为 protocol、diagnostics、completion、hover、definition、semantic tokens、formatting、URI/path helpers。
- VS Code 插件拆分为 session、providers、fallback 或 failure UI、path/config helpers。
- codegen 拆分为 preflight、schema-to-IR、render、per-format runtime glue。
- data model compiler 拆分为 record validation、index build、reference/path resolution、diagnostic mapping。
- CFT parser/compiler 拆分为 lexer-adapter、declaration parser、expression parser、schema lowering 和 semantic checks。
- checker evaluator 拆分为表达式求值、内建函数、比较/集合操作和诊断构造。
- Excel/CFD loader 拆分为 source discovery、header/column resolution、value conversion、origin mapping 和 diagnostics。

## 错误码覆盖标准尚未统一到所有模块（持续推进）

CFT、cell value、`CfdErrorCode` 和 `CFD-TEXT-*` 已建立“负向触发 + 相邻合法输入不误报”
覆盖。Excel loader、pipeline/artifact/codegen 和 CLI 路径也已补充集中覆盖。

影响：

- 新增错误码时可能只证明能触发，不能证明不会误报。
- 不同模块的错误码测试强度不一致。

建议方向：

- 继续把同一标准扩展到 LSP 可发布的所有诊断路径。
- 对每个错误码记录：触发样例、相邻合法样例、期望位置和消息要点。
- 新增错误码时同步补充覆盖矩阵，避免只在端到端 happy path 中间接覆盖。

## 生成物和 lockfile 策略需要进一步明确（已修复）

普通生成物由 Coflow 完整接管，`coflow.enum.lock.json` 已从 C# 输出目录分离到
`coflow.yaml` 同级。

处理结果：

- `examples/*/generated/**` 继续作为普通生成物忽略。
- `examples/*/coflow.enum.lock.json` 可作为稳定枚举值输入提交。
- C# 输出目录替换不会删除 lockfile。
- 文档明确 `outputs.data.dir` 和 `outputs.code.dir` 不能放手写文件。

## 产物写入缺少原子提交语义（已修复）

数据导出和 C# codegen 曾逐文件创建或覆盖，失败时可能留下部分更新产物。

处理结果：

- 数据导出和 C# codegen 先写入同级 staging 目录，再替换目标输出目录。
- 写入 staging 失败时，旧输出目录保持不变。
- `coflow.enum.lock.json` 通过 staging 文件提交；提交阶段目录替换失败时尽力回滚 lockfile。

残余约束：

- Windows 上无法把“输出目录 + 同级 lockfile”两个路径做成单个 OS 级原子事务。
  当前实现保证 staging 完整写入后才提交，并在跨路径提交失败时做 best-effort rollback。

## Checker 内建函数契约仍需集中整理（已修复）

`coflow-checker` 的运行时分支已补充单参数函数的严格 arity 防御，并修复
`contains(null, value)` 被降级成普通 false 的问题。内建函数名称和 arity
曾分散在多个分支中维护。

处理结果：

- 新增 builtin registry，集中声明 `len`、`contains`、`unique`、`min`、`max`、
  `sum`、`keys`、`values` 和 `matches` 的名称与参数个数。
- evaluator dispatch 使用同一份 registry 做 arity 防御。
- 增加 registry 测试，避免新增或改名后运行期分支漂移。

后续方向：

- 继续推进到参数类型签名共享，让 CFT type checker 和 evaluator 在类型契约上也共享定义。
- 继续补充负向和相邻合法测试：nullable 集合、nullable 元素、dict key 类型、空集合。

## 多态路径引用下钻语义（已修复）

data model 的路径引用曾在穿过数组或字段后使用声明类型继续解析。如果字段声明为
父类或 abstract 类型，但实际值是子类，后续访问子类字段会被当作未知字段。

影响：

- 形如 `@DropTable.table.rewards[0].item` 的路径，在 `rewards: [Reward]` 且实际元素为
`ItemReward` 时，曾无法访问 `ItemReward.item`。

处理结果：

- 已在路径解析阶段读取当前值的实际 `RecordDraft.actual_type`，并按实际类型查找后续字段。
- 已补充回归测试覆盖 `[Reward]` 中实际 `ItemReward` 元素继续访问 `item` 字段的场景。

## Excel `@expand` 表头吞列规则风险（已修复）

Excel loader 曾对 `@expand` 字段按内层字段数量位置消费后续列，并忽略这些被消费列的
表头文本。

影响：

- 如果用户在 `@expand` 列后紧跟普通业务列，例如 `id, env, level`，`level` 可能被
当作 `env` 的子字段列消费，导致真实 `level` 列没有按预期映射。
- data-model/check 诊断目前主要定位到 expand 父列，不一定能定位到具体子列。

处理结果：

- 已对被 `@expand` 消费的后续 header 做约束：必须为空，且必须紧邻父列并按字段声明顺序消费。
- Excel 合并表头的后续单元格在读取时为空，因此是推荐的分组表头写法。
- 已为 merged-header 风格空表头、`@expand` 后接普通列错误场景、显式子字段 header 错误场景、相邻列不足场景补充测试。

补充处理：

- Excel origin 已记录 `@expand` 子字段到具体 Excel 列的映射。
- data-model 诊断路径如 `env.temperature` 会优先定位到子字段列，而不是只定位到 `env`
  父列。

## Source 扩展名大小写敏感（已修复）

pipeline 曾按扩展名字符串识别 `.xlsx`、`.xlsm`、`.xls` 和 `.cfd`。大小写不同的
`.XLSX` 或 `.CFD` 在目录扫描时会被忽略，在显式文件源中会被判定为不支持。

影响：

- Windows 用户从外部工具导出的文件名可能使用大写扩展名。
- 同一项目在不同平台或工具链下可能表现不一致。

建议方向：

- 已将 source kind 判断改为 ASCII lowercase 后匹配。
- 已补充目录源 `.XLSX` / `.CFD` 和显式 `.CFD` 文件源测试。
