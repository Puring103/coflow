# 设计缺陷与后续处理清单

本文档记录质量梳理过程中发现的设计级问题。这里的问题不一定需要在当前批次立即修复，但后续重构应持续消化。

## Rust LSP 与 VS Code 插件存在语义重复

当前 Rust LSP 已实现补全、hover、definition、document symbol、formatting 和 semantic tokens。VS Code 插件中仍保留较多本地解析、符号收集和语义 fallback 逻辑。

影响：

- CFT/CFD 语义规则变更后，Rust 和 JS 两套实现可能出现行为漂移。
- 插件维护成本高，测试需要覆盖重复逻辑。
- 用户遇到 LSP 启动失败时，插件 fallback 可能给出与真实语言服务不同的结果。

建议方向：

- Rust LSP 作为唯一语义事实来源。
- VS Code 插件只保留 LSP 进程管理、协议转换、错误展示和最小降级提示。
- 在移除 fallback 前，先补齐 LSP 行为测试，避免编辑器能力回退。

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

## 错误码覆盖标准尚未统一到所有模块

CFT、`CfdErrorCode` 和 `CFD-TEXT-*` 已建立“负向触发 + 相邻合法输入不误报”
覆盖。但同一标准还没有统一覆盖到所有错误码体系。

影响：

- 新增错误码时可能只证明能触发，不能证明不会误报。
- 不同模块的错误码测试强度不一致。

建议方向：

- 为 cell value、Excel loader、pipeline/artifact/codegen 逐步建立错误码覆盖清单。
- 对每个错误码记录：触发样例、相邻合法样例、期望位置和消息要点。

## 生成物和 lockfile 策略需要进一步明确

普通生成物已被忽略，但 `examples/humanpark/generated/csharp/coflow.enum.lock.json` 作为 `@keyAsEnum` 稳定值 lockfile 被跟踪。

影响：

- “generated 目录忽略但其中有跟踪文件”的策略容易造成维护混淆。
- 后续新增示例或变更生成路径时可能误删 lockfile。

建议方向：

- 在示例或项目文档中明确：普通生成文件不提交，`coflow.enum.lock.json` 可作为稳定枚举值输入提交。
- 长期可以考虑把 lockfile 输出位置从普通生成目录中分离出来。

## 产物写入缺少原子提交语义

当前数据导出和 C# codegen 已通过 manifest 降低误删/误覆盖风险，但写入流程仍是
逐文件创建或覆盖。C# codegen 还会在写 `.cs` 文件前更新 `coflow.enum.lock.json`。

影响：

- 磁盘满、权限变化、进程中断或部分文件被占用时，输出目录可能留下部分更新的产物。
- `coflow.enum.lock.json` 可能已经加入新枚举值，但对应 `.cs` 文件写入失败。
- 当前测试主要覆盖 preflight 诊断不会写入，尚未覆盖运行时 I/O 失败后的状态一致性。

建议方向：

- 使用临时目录生成完整产物，成功后再进行目录级替换或原子 rename。
- 将 lockfile 更新纳入同一提交步骤，避免 lockfile 与生成代码分离更新。
- 增加写入失败模拟测试：目标文件被目录占用、只读文件、部分写入失败后旧输出是否保留。

## Checker 内建函数契约仍需集中整理

`coflow-checker` 的运行时分支已补充单参数函数的严格 arity 防御，并修复
`contains(null, value)` 被降级成普通 false 的问题。但内建函数契约仍分散在
CFT type checker 和 checker evaluator 两处。

影响：

- 新增内建函数时需要同时维护 CFT 类型检查和运行期求值规则。
- `CFD-CHECK-EVAL-TYPE` 的覆盖仍需要系统化，不能只依赖现有场景测试。

建议方向：

- 为每个内建函数声明参数个数和参数类型，运行时严格校验。
- 尽量让 CFT type checker 和 checker evaluator 共享同一份函数签名定义。
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

- 已对被 `@expand` 消费的后续 header 做约束：必须为空，或显式写成预期子字段名。
- 已为显式子字段 header 合法场景、`@expand` 后接普通列错误场景、相邻列不足场景补充测试。

后续方向：

- 在 origin 中记录 expand 子字段到具体 Excel 列的映射。

## Source 扩展名大小写敏感（已修复）

pipeline 曾按扩展名字符串识别 `.xlsx`、`.xlsm`、`.xls` 和 `.cfd`。大小写不同的
`.XLSX` 或 `.CFD` 在目录扫描时会被忽略，在显式文件源中会被判定为不支持。

影响：

- Windows 用户从外部工具导出的文件名可能使用大写扩展名。
- 同一项目在不同平台或工具链下可能表现不一致。

建议方向：

- 已将 source kind 判断改为 ASCII lowercase 后匹配。
- 已补充目录源 `.XLSX` / `.CFD` 和显式 `.CFD` 文件源测试。
