# Coflow 语言与格式化边界重构计划

## 1. 目标和范围

本次重构收紧 `coflow-language` 的职责，并将 CFT/CFD 格式化能力迁移到独立的
`coflow-format` crate。重构直接切换到最终 API，不保留旧路径的兼容 re-export，也不保留两套
formatter 实现。

目标如下：

- `coflow-language` 只负责语言基础设施、CFT 语法与 schema 编译、schema-free CFD 语法、诊断和
  结构限制。
- `coflow-format` 负责 CFT/CFD 的规范源码输出，并复用 `coflow-language` 提供的源码与词法设施。
- `coflow-lsp` 只负责 LSP formatting 请求、文档状态校验和 `TextEdit` 转换。
- 根 `coflow` crate 只负责项目级格式化目标发现、`--check`、文件写入和命令输出。
- 格式化规则测试归 `coflow-format` 所有；LSP、CLI、runtime 只保留各自边界的集成测试。
- CFT、CFD 和 CFD 函数语法统一使用相同的 Unicode 标识符及基础字面量规则。
- 语言结构安全限制与 checker 执行预算分离，由 runtime 组合并传递默认配置。
- 源码位置类型与结构预算分离，公共 API 按职责组织，不再依赖 crate 根的大规模扁平 re-export。

非目标：不修改 CFT/CFD 的语言语义，不引入 schema-guided 格式化，不把 formatter 变成完整 AST
pretty printer，不改变 CLI 命令行为，也不改变 LSP 对外协议。

## 2. 最终职责和依赖拓扑

| crate | 保留职责 | 明确排除 |
| --- | --- | --- |
| `coflow-language` | source/span、共享词法规则、CFT lexer/parser/AST、schema compiler、schema-free CFD parser/AST、diagnostics、结构深度与节点限制 | 格式化策略、checker 执行预算、LSP `TextEdit`、项目文件发现与写入 |
| `coflow-format` | CFT/CFD lossless token 格式化、规范空白与布局、错误源码的稳定格式化 | LSP JSON、文件系统访问、项目配置、schema 检查 |
| `coflow-checker` | check 求值、量词迭代及其 `EvaluationLimits` 执行预算 | parser/compiler 的结构限制 |
| `coflow-lsp` | formatting capability、document snapshot、解析有效性策略、字节位置到 LSP range 转换、最小文本编辑 | 排版规则和格式化状态机 |
| 根 `coflow` | `coflow format` 的目标发现、去重、`--check`、原子写入和终端输出 | 语言排版规则 |
| `coflow-runtime` | 组合语言结构限制与 checker 执行预算、CFD 语义写入和规范 writer 输出 | 通用源码 formatter 和 LSP 适配 |

依赖方向固定为：

```text
coflow-format     -> coflow-language
coflow-checker    -> coflow-language
coflow-runtime    -> coflow-language + coflow-checker
coflow-lsp        -> coflow-language + coflow-runtime + coflow-format
coflow CLI        -> coflow-runtime + coflow-lsp + coflow-format
cfd-editor-core   -> coflow-runtime + coflow-lsp
```

`coflow-format` 不依赖 `coflow-runtime` 或 `coflow-lsp`。`coflow-language` 不反向依赖
`coflow-format`。`coflow-runtime` 可以在测试中使用 `coflow-format` 验证 writer 的规范输出，但生产
依赖中不引入 formatter。

## 3. `coflow-language` 最终内部结构

目标模块结构如下，具体文件可以按实现规模继续细分，但职责不能重新混合：

```text
coflow-language/src/
  lib.rs
  source/
    mod.rs              # Span 和源码范围基础类型
  lexical/
    mod.rs              # Unicode 标识符、字符分类、共享扫描原语
    trivia.rs           # 空白、换行、注释及原始范围
  diagnostics/
  limits/
  cft/
    syntax/             # lexer、parser、AST、recovery
    module/
    schema/             # compiler、声明、索引和执行计划
  cfd/
    parser/
    function/
```

`Span` 从 `limits` 移入 `source`。`limits` 只保留语言解析和编译所需的结构深度、节点预算、cursor
和对应的结构类别。所有 lexer、parser、diagnostic 和 schema 类型统一引用 `source::Span`。

语言层的限制收口为：

```rust
pub struct StructuralLimits {
    pub max_depth: u64,
    pub max_nodes: u64,
    pub max_analysis_steps: u64,
}
```

`max_depth` 和 `max_nodes` 限制输入结构；`max_analysis_steps` 限制 schema 继承、check index、value
dependency 等静态图分析产生的边访问和计算步骤。语言层不再包含含义宽泛的 `max_work`，也不再包含
`CheckEvaluation` 和 `QuantifierIteration`。静态分析必须使用显式去重的有界图遍历，并统一计入
`max_analysis_steps`，不能因为输入节点数有限就假设派生边和遍历次数自然足够小。

`coflow-checker` 定义并拥有：

```rust
pub struct EvaluationLimits {
    pub max_work: u64,
    pub max_iterations: u64,
}
```

`max_work` 限制表达式求值、内建操作和数据访问的累计工作量；`max_iterations` 单独限制量词和集合迭代，
避免调用方通过调整一种含义模糊的统一计数间接改变另一类限制。checker 内部预算和超限诊断随该类型
迁移，不再从 `coflow-language` 导入 checker 专用 `StructureKind`。

`coflow-runtime` 提供项目运行时使用的组合配置，分别把 `StructuralLimits` 传给语言、model 构建路径，
把 `EvaluationLimits` 传给 checker。两组限制使用独立默认值；内部安全上限无需作为项目配置公开。

共享词法层至少统一以下规则：

- 标识符首字符使用 `unicode_ident::is_xid_start`，后续字符使用
  `unicode_ident::is_xid_continue`，并统一处理 `_`。
- CFT 保留字判断只有一个实现。
- 字符串边界、转义扫描、注释边界、数字字面量和符号最长匹配使用共享扫描原语。
- 每个 token 和 trivia 保留原始 byte span；不得把 UTF-8 byte offset 与 LSP UTF-16 position
  混入语言层。

CFT 和 CFD 可以保留不同的 token enum 与 parser，函数体也保留专用 grammar；共享的是底层扫描
规则和 cursor，而不是强行合并不同文法。

公共 API 使用命名空间表达所有权。crate 根只导出主要入口和最常用结果类型，其余类型从
`cft`、`cfd`、`source`、`diagnostics`、`limits` 命名空间访问。迁移完成后删除旧的扁平 re-export。

## 4. Schema 编译流水线

schema 编译从共享可变 `SchemaCompiler` 改为显式阶段流水线。每个阶段接收不可变输入和上一个阶段的
完整产物，返回下一阶段产物及诊断；阶段产物构造完成后不再回写。目标数据流如下：

```text
CftModuleSet
  -> collect_symbols -> SymbolTable
  -> resolve_types -> ResolvedTypes
  -> resolve_values -> ResolvedValues
  -> validate_checks -> ValidatedSchema
  -> lower_schema -> CftSchema
```

阶段函数遵循以下约束：

- 输入通过共享引用传递，产物按所有权返回，不把所有中间状态集中在一个长期存活的可变对象中。
- 同一阶段内的局部缓存和诊断收集可以使用受限可变状态，但不得跨阶段暴露半初始化字段。
- 名称解析、类型推导、常量求值和依赖提取尽量实现为输入决定输出的函数。
- 错误恢复通过显式阶段结果携带产物和诊断，不依赖隐藏的全局 diagnostics vector。
- `AnalysisBudget` 作为显式参数传入静态图分析阶段，消耗 `max_analysis_steps`，不得藏在 compiler
  全局状态中。
- 阶段之间使用强类型产物表达已经建立的不变量，后续阶段不能访问尚未验证的原始 map。
- 删除为绕过整体 `&mut self` 借用而复制 key、逐项 clone 的 `each_type`、`each_enum` 模式。

lexer 和 parser 保留封装 cursor、lookahead、recovery 与结构预算的局部可变对象。它们本质上是顺序状态
机，不要求改写为递归复制输入的纯函数。formatter 的 token 到布局转换使用不可变 token 输入和显式
layout state；文件访问、LSP range 和项目状态不得进入这些转换函数。

## 5. `coflow-format` 设计

`coflow-format` 提供两个稳定入口：

```rust
pub fn format_cft(source: &str) -> String;
pub fn format_cfd(source: &str) -> String;
```

formatter 使用保留 trivia 和原始 span 的 token 流，不重新实现字符串、注释、标识符、数字或函数
边界扫描。格式化过程只决定空白、换行和缩进，不负责语法或语义合法性。

必须保持以下不变量：

- 不改变非 trivia token 的文本、顺序和数量。
- 不改变注释内容。
- 对有效输入输出规范布局。
- 对可识别的未完成输入输出稳定布局；无法安全判断结构时保留原文局部布局。
- 输出使用 LF，并以一个换行结束。
- 输出幂等：`format(format(source)) == format(source)`。
- 格式化前后重新解析的有效源码具有等价语法结构。
- formatter 不读取文件、不加载项目、不编译 schema，也不产生 LSP 类型。

当前 `formatting.rs` 中的行折叠、结构展开、delimiter、generic continuation、annotation 和 function
body 状态应按 token/结构职责拆分。迁移结束后删除字符级 `FunctionBodyTracker` 及重复的字符串、注释
和 delimiter 扫描逻辑，不能把旧实现整体搬入新 crate 后长期保留。

## 6. 宿主边界

### 6.1 LSP

`coflow-lsp` 直接依赖 `coflow-format`。`textDocument/formatting` 的处理顺序固定为：

1. 获取当前 document snapshot，而不是重新读取磁盘。
2. 按文件类型执行现有的格式化允许策略。
3. 调用 `coflow_format::format_cft` 或 `coflow_format::format_cfd`。
4. 将源码差异转换为不重叠、位置正确的 LSP `TextEdit`。
5. 对规范输入返回空 edits；未知文档和不支持的扩展名保持现有协议行为。

`formatting_edits`、UTF-16 range 转换和协议错误处理继续留在 `coflow-lsp`，不得下沉到
`coflow-format`。

### 6.2 CLI

根 crate 直接依赖 `coflow-format`。`format_command` 保留项目解析、目标发现、真实路径去重、生成目录
排除、`--check`、原子写入和报告；只替换 formatter 的导入路径。CLI 不经由 LSP 调用格式化。

### 6.3 编辑器

编辑器继续通过内嵌 `coflow-lsp` 请求格式化，不直接依赖 `coflow-format`。这样编辑器只维护一种语言
服务调用路径，文档 generation、revision 和 edits 应用仍由 editor core 管理。

### 6.4 Runtime writer

runtime writer 继续负责 schema-guided CFD 精确写入，并直接生成符合规范的局部文本。writer 的生产
代码不调用通用 formatter，以免一次字段修改重排整个文件。writer 测试可以通过
`coflow-format` dev-dependency 断言完整输出已经规范化。

## 7. 测试归属和迁移

测试必须先于实现替换迁移，形成当前行为基线。

### `coflow-format`

从 `coflow-lsp` 迁移所有直接调用 `format_cft`、`format_cfd` 并断言排版结果的测试，同时迁移
`coflow-language` 中现有的 formatter 单元测试。测试按 CFT、CFD 和性质测试组织，至少覆盖：

- strings、escapes、comments 和 delimiter isolation；
- type、enum、check、annotation、default value 和 function type；
- CFD 顶层记录、group、polymorphic object、array、dict 和 function body；
- 多行泛型、连续运算符、续行缩进和空行策略；
- Unicode XID 标识符；
- 当前支持的未完成和可修复源码；
- 幂等性；
- 非 trivia token 保持；
- 有效输入的 parse-format-parse 结构等价。

测试夹具必须保留重构前的期望输出，不能在迁移实现时批量修改 expected 来掩盖行为变化。确需改变格式
契约时，应先更新 `04-source-formatting.zh-CN.md`，再单独修改对应测试。

### `coflow-lsp`

只保留以下测试：

- capability 和 protocol response；
- dirty document 使用内存 snapshot；
- 未知 URI、不支持扩展名和无变化文档；
- `TextEdit` 不重叠、应用后等于 formatter 输出；
- UTF-8 源码到 UTF-16 LSP range 的转换。

### 根 CLI、editor 和 runtime

- CLI 测试目标发现、`--check`、不写模式、原子写入和输出状态。
- editor 测试内嵌 LSP 调用、revision 和 edits 应用，不重复断言完整格式规则语料。
- runtime writer 测试局部写入结果可重新解析且已经满足规范格式。

## 8. 实施阶段

### 阶段 A：冻结行为与统一词法规则

1. 为 CFT、CFD、函数体和 record key 增加 Unicode XID 一致性测试。
2. 把标识符和基础扫描规则收口到 `coflow-language::lexical`。
3. 将 `Span` 移入 `source`，一次性更新全部引用并删除 `limits::Span` 旧路径。
4. 确认现有 parser、compiler、LSP 和 runtime 行为不变。

完成条件：不存在 `is_alphabetic/is_alphanumeric` 实现的语言标识符判断；`Span` 只有一个定义和一个
正式公共路径。

### 阶段 B：拆分结构限制与执行预算

1. 将 `StructuralLimits` 收口为 `max_depth`、`max_nodes` 和 `max_analysis_steps`，同步 parser、schema
   compiler 和 model 构建调用点。
2. 在 `coflow-checker` 新建 `EvaluationLimits` 和 checker 私有执行预算，迁移表达式、内建操作、数据
   访问和量词的计数逻辑。
3. 将量词累计次数从通用 `max_work` 中分离，由 `max_iterations` 独立限制。
4. 删除 `coflow-language` 中的 `CheckEvaluation`、`QuantifierIteration`、`BudgetAxis::Work` 和 checker
   专用错误构造。
5. 将 schema dependency 和 check index 构造改为显式去重的有界图遍历，统一消耗
   `max_analysis_steps`，并增加大图、重复边、循环依赖和预算边界测试。
6. 由 `coflow-runtime` 组合两组默认限制；删除 runtime 和 checker 对语言层 limits 的兼容 re-export。
7. 拆分现有 checker budget 测试，分别验证深度、节点、总工作量和迭代次数达到边界前后的一致行为。

完成条件：language 不包含求值预算概念；checker 不复用 `StructuralBudget`；runtime 调用路径明确传递两组
限制；所有超限诊断仍能定位到对应源码或数据位置。

### 阶段 C：函数化 schema 编译流水线

1. 为 symbols、resolved types、resolved values、validated checks 和 lowered schema 定义阶段产物。
2. 将 `SchemaCompiler::compile` 拆成按所有权传递阶段产物的顶层流水线。
3. 将 diagnostics 和 `AnalysisBudget` 作为显式阶段输入输出传递。
4. 迁移名称解析、alias、inheritance、constants、defaults 和 checks，删除对全局 compiler map 的隐式
   读写。
5. 删除 `each_type`、`each_enum` 及为释放整体可变借用产生的 key snapshot 和逐项 clone。
6. 为每个阶段增加输入不变量、输出不变量、错误累计和预算耗尽测试。

完成条件：不存在持有全部编译阶段状态的 `SchemaCompiler`；每个阶段可以独立测试；失败路径不会向后续
阶段暴露半初始化数据；同一输入和限制始终产生相同 schema 与诊断顺序。

### 阶段 D：建立 crate 与迁移测试所有权

1. 新建 workspace member `crates/coflow-format`。
2. 将完整格式化行为测试迁移到新 crate，保持期望输出不变。
3. 先以行为等价方式迁移 formatter，切换 CLI 和 LSP 依赖。
4. 删除 `coflow-language::format_cft`、`coflow-language::format_cfd` 及其 formatting 模块。
5. 删除 LSP 对 formatter 的 re-export；测试直接从正确 crate 导入。

这一阶段允许新 crate 暂时承载旧算法，但同一提交序列内必须继续完成阶段 E，不把该中间状态作为重构
终点。

完成条件：全仓库不存在从 `coflow-language` 导入 formatter；CLI 不依赖 LSP 获取格式化能力；所有既有
格式化测试在新归属下通过。

### 阶段 E：替换重复扫描器

1. 在 `coflow-language` 中提供保留 trivia/span 的 CFT 与 CFD tokenization API。
2. 按 token 流重写 `coflow-format` 的空白、换行、缩进和结构布局。
3. 用共享词法 cursor 改造 CFD function validator，保留函数专用 parser。
4. 删除 formatter 和 function validator 中重复的字符串、注释、标识符、数字和符号扫描代码。
5. 增加 malformed corpus、幂等性和 parse-format-parse 性质测试。

完成条件：格式化器不再通过逐字符猜测字符串、注释、函数签名或泛型边界；语言词法规则只有一个
权威实现；旧 formatter 状态机全部删除。

### 阶段 F：公共 API 和文档收口

1. 按 `cft`、`cfd`、`source`、`diagnostics`、`limits` 整理公共路径。
2. 删除 crate 根不属于主要入口的批量 re-export，并更新所有 workspace 调用方。
3. 更新 `AGENTS.md` 的 crate boundary 描述。
4. 更新 `docs/language-design/04-source-formatting.zh-CN.md`，将核心所有者改为
   `coflow-format`，保留最终格式契约和宿主接入方式。
5. 检查其他内部架构文档，不保留 `coflow-language` 拥有 formatter 的陈述。

完成条件：文档只描述最终架构；不存在兼容模块、弃用 API、旧导入路径或重复职责描述。

## 9. 验证与静态审计

每个阶段完成后从仓库根运行普通开发检查：

```powershell
cargo check --workspace
cargo test --workspace
```

重构完成后的静态审计：

```powershell
rg -n "coflow_language::.*format_|coflow_language::\{[^}]*format_|pub.*format_cft|pub.*format_cfd" . --glob '!target/**'
rg -n "is_alphabetic|is_alphanumeric" crates/coflow-language/src --glob '*.rs'
rg -n "limits::Span|pub use crate::limits::Span" . --glob '!target/**'
rg -n "CheckEvaluation|QuantifierIteration|BudgetAxis::Work|max_work" crates/coflow-language --glob '*.rs'
rg -n "StructuralBudget" crates/coflow-checker --glob '*.rs'
rg -n "fn each_type|fn each_enum|struct SchemaCompiler" crates/coflow-language/src/schema --glob '*.rs'
cargo tree -p coflow-format
cargo tree -p coflow-lsp
```

审计结果必须满足：

- formatter 公共入口只由 `coflow-format` 提供；
- `coflow-format` 的生产依赖中没有 runtime、LSP、CLI 或 editor；
- LSP formatting 只包含协议适配和 edits 生成；
- CLI 直接依赖 `coflow-format`；
- 语言标识符规则统一使用 Unicode XID；
- `Span` 不再属于 `limits`；
- language 的 `StructuralLimits` 分别限制深度、节点和静态分析步骤，checker 的 `EvaluationLimits` 独立
  限制求值工作量和迭代；
- runtime 明确组合并分别传递两组限制；
- schema compiler 使用强类型阶段产物和显式诊断、预算传递，不存在全阶段共享可变状态；
- 格式化行为测试位于 `coflow-format`，协议测试位于 `coflow-lsp`；
- workspace check 和 test 全部通过。

## 10. 完成定义

只有同时满足以下条件，本次重构才算完成：

- `coflow-format` 已成为唯一格式化核心，旧实现和旧 API 已删除。
- CLI、LSP、editor 和 runtime 的依赖方向符合最终拓扑。
- formatter 基于共享 lossless token/source 设施，不重复维护语言词法规则。
- CFD function validator 与 CFT/CFD parser 使用统一的标识符和基础扫描规则。
- 语言结构限制与 checker 执行预算已经拆分，不存在跨 crate 复用含义混杂的预算类型。
- schema 编译由可独立测试的函数化阶段组成，不存在半初始化的全局 compiler state。
- `coflow-language` 的 source、limits、CFT、CFD 和 schema 职责可以从模块路径直接识别。
- 测试已按所有权迁移，既有格式化行为没有未经设计文档确认的变化。
- 内部设计文档和 `AGENTS.md` 已同步到最终架构。
- `cargo check --workspace` 与 `cargo test --workspace` 通过。

## 11. 实施进度

截至 2026-09-04，以下工作已经完成：

- `Span` 已迁移至 `coflow-language::source`，标识符统一使用 Unicode XID 规则，公共 API 已按
  `cft`、`cfd`、`source`、`diagnostics`、`limits` 和 `lexical` 命名空间收口。
- language 结构限制与 checker 执行限制已经分离；runtime 分别组合并传递 `StructuralLimits` 和
  `EvaluationLimits`，工作量与迭代次数使用独立预算。
- schema 编译已建立 `SymbolTable`、`ResolvedTypes`、`ResolvedValues` 和 `ValidatedSchema` 阶段产物，
  删除 `SchemaCompiler`、`each_type`、`each_enum` 和阶段间 `DerefMut`。
- `coflow-format` 已加入 workspace 并成为 CFT/CFD 格式化入口；CLI 和 LSP 已直接依赖该 crate，
  language 中的旧 formatter 与 LSP formatter re-export 已删除。
- formatter 与 CFD function validator 已使用共享 lossless token，格式化行为测试已迁移至
  `coflow-format`，并覆盖 token/comment 保持、幂等性和 parse-format-parse 性质。
- crate 边界说明和源码格式化设计文档已同步到当前架构。

- CFT lexer、schema-free CFD parser、formatter 与 function validator 已共同使用
  `coflow-language::lexical` 提供的数字边界、字符串转义和成对分隔符扫描原语；函数外壳解析基于无损
  token 的 span 定位，不再单独逐字符跳过字符串、注释和嵌套括号。
- schema 编译入口已收口为 `collect_symbols`、`resolve_types`、`resolve_values`、`validate_checks` 和
  `lower_schema` 阶段函数；每个阶段只发布构造完成的产物或诊断，并已覆盖符号表、继承字段、常量与
  默认值、check 分析产物等阶段不变量。

本计划所列重构工作已全部完成。

当前检查点已通过：

```powershell
cargo check --workspace
cargo test --workspace
git diff --check
```
