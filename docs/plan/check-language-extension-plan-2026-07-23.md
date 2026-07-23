# Check 语言扩展实现计划

日期：2026-07-23

## 1. 目标与边界

本计划增强 CFT `check {}` 的诊断表达、字符串处理、常用校验函数、nullable 访问、集合遍历和跨记录校验能力，同时保持语言声明式、可静态分析、可增量执行和有界求值。

本轮不考虑引用图环检测、项目级图约束、计数量词、通用循环、递归函数、动态反射、数据修改或完整 lambda/map/filter/reduce 体系。计数量词必须经过独立完整设计后才能进入后续实施计划。

现有语法和行为必须保持兼容：

```cft
check {
  price > 0;
  tags.isUnique();
  all reward in rewards { reward.count > 0; }
}
```

顶层跨记录规则的目标形态：

```cft
check ItemIntegrity {
  records(Item).len() > 0:
    "项目中至少需要配置一个物品";

  all item in records(Item) {
    item.price > 0:
      f"物品 {item.id} 的价格必须大于 0";
  }
}
```

目标形态：

```cft
check {
  level > 0:
    f"怪物 {id} 的等级必须大于 0，当前为 {level}";

  id == f"{category}_{level}":
    f"ID 应为 {category}_{level}，当前为 {id}";

  (cooldown ?? 0.0) >= 0.0:
    f"冷却时间不能为负数，当前为 {cooldown}";

  weights.sum().approxEqual(1.0, 0.0001):
    f"权重总和必须为 1，当前为 {weights.sum()}";
}
```

## 2. 总体实施原则

1. 按可独立合并的阶段实施，每个阶段均通过 workspace 必需检查。
2. parser AST、schema AST、类型检查、lowering、依赖计划、checker、LSP 和文档必须同步更新。
3. condition 返回 false 且存在自定义消息时，自定义消息完全替换自动生成的可读解释；错误码、severity、源码/数据位置、related locations 和执行上下文使用独立结构化字段保存，不拼接回自定义消息。
4. 新表达式必须计入 `CheckAst` 结构预算和 `CheckEvaluation` 工作预算。
5. 短路或延迟求值分支只收集实际读取的数据依赖。
6. 不通过添加宽松隐式类型转换来提升便利性。
7. 保持现有“裸 bool 表达式即校验条件”的语义，不增加 `assert`、`require` 等语句关键字。
8. 不引入 `let` 或其他局部声明，避免把 check block 扩展成有声明顺序的程序。

### 2.1 必须随功能完成的重构

本计划不接受“新路径旁挂旧路径”的长期实现。下列重构属于功能交付本身，不作为后续清理项：

- syntax AST 与 schema AST 各自建立统一的 check expression/statement visitor/walk 设施，分别供 parser-side 工具/LSP 与 schema-side 预算/依赖分析使用；允许各 consumer 保留自己的状态与结果，但不得各自维护容易漏 variant 的手写递归骨架。新增 AST variant 时，编译期 exhaustive match 或 visitor 完整性测试必须使所有 consumer 同时暴露缺口。
- syntax visitor 的迁移范围明确包括 semantic tokens、definition/hover 定位、completion 的 enclosing scope/binding 收集、formatting 和 document symbols；consumer 可以在 hook 中决定 token 分类或维护 offset 范围，但不得保留 `CheckStmt`/`CheckExprKind` 的子树递归。parser 本身按优先级构造 AST，不属于 visitor consumer。schema visitor 必须支持可失败回调和量词 body 的 enter/exit scope hook，使预算、静态依赖和 record-set 收集都复用同一结构遍历。
- 把 scalar 的稳定文本表示集中为一个 checker/value-formatting helper，自动诊断、f-string 和量词 context display 共用它；不得在三个 renderer 中分别定义 float、enum、null 和字符串的格式。
- builtin 的名称、receiver、参数、返回类型、文档键和运行期实现标识由单一 registry metadata 驱动。schema 类型检查、LSP completion/hover 和 checker dispatch 不再维护彼此独立的名称表；添加注册表完整性测试，保证每个公开 builtin 恰有一个运行期实现和文档项。
- structured diagnostic context 只在 checker 内部逻辑诊断到 `coflow-api::Diagnostic` 的映射边界转换一次。CLI、JSON、LSP 和 editor 消费 canonical 字段并各自渲染，不允许 runtime 或 host 再解析 message、拼接后又拆分 context。
- check type analyzer 在已有的 typed expression traversal 中直接收集每条根 statement 的 dimension，并 lower 到 canonical `CftSchemaCheckBlock` schedule；替换并删除旧的 `dimension_checks` 近似 walker 和中间 `CheckDependencyPlan`。`TypedCheckPlan` 只负责 owner/继承调度索引和查询，不继续隐藏另一套表达式语义分析。
- `CheckExecutionId`、path-level dependency 和 `CheckChangeSet` 落地时，原有 record-only root、coordinate-only `reads_from` 和 changed-coordinate 增量入口必须原位迁移或在同一阶段删除。若公开 API 需要兼容适配器，适配器只能位于 API 边界，必须有弃用说明、明确移除版本和适配行为测试；内部不得同时维护两份 snapshot/dependency state。
- 顶层 check 与 type-local check 共用 parser statement grammar、表达式类型系统、evaluator、预算、diagnostic pipeline 和 snapshot engine；差异只由显式 `CheckScope`、execution identity 和 source/data root 表达，禁止复制一套 top-level evaluator。
- schema source span 到 canonical file location 的转换统一使用 runtime module source catalog；CLI、LSP 和 editor 不得各自根据 check 名称或模块名猜路径和行列。
- 所有临时兼容字段、转换 helper 和 feature flag 必须在对应阶段的提交说明中列出。最终阶段执行 `rg` 审计旧类型、旧字段和 TODO 标记，计划内的临时项必须归零。

### 2.2 分阶段兼容与提交纪律

- 每个阶段先修改内部 canonical model，再迁移所有 producer/consumer，最后删除旧 model；不得为了让中间提交编译而永久保留双写。
- 对公开 Rust API 或 serde wire shape 的变化，先补兼容测试再改实现。新增字段使用有默认值的向后兼容反序列化；确需破坏性 Rust API 变更时在同一提交迁移 workspace 内全部调用方，并在 schema API 文档中记录。
- parser AST 和 schema AST 可以保持层次边界，但字段和 variant 必须一一可追踪；lowering 是唯一转换点，不允许 checker 兼容 syntax AST 或通过字符串反向识别特殊形式。
- 每个阶段必须是可独立验证、可独立 review 的提交。跨阶段基础重构应单独先提交并保持行为不变，功能提交不得夹带无法由测试证明的无关重写。
- 不以遗留 `TODO`、注释掉的分支、未使用兼容类型、重复 renderer/visitor 或“先全量失效以后再优化”作为阶段完成状态。确实超出本计划的能力必须在“非目标”中明确，而不是留在实现里。

### 2.3 重构取舍、所有权与完成证据

“不留技术债”不等于无边界地重写。纳入本计划的重构必须至少满足以下一项：消除本轮功能会放大的重复语义、建立跨 crate 的唯一事实来源、使增量执行保持正确、让预算能够覆盖新增工作，或删除已被 canonical model 替代的旧路径。纯命名调整、与 check 无关的通用框架化以及无法由本轮测试证明收益的抽象不进入本计划。

模块所有权固定如下，实施中不得通过反向依赖绕开边界：

| 能力 | 唯一所有者 | 其他模块的职责 |
| --- | --- | --- |
| syntax AST 遍历 | `coflow-cft::syntax` | parser-side 工具和 LSP 提供 visitor 状态，不复制递归 |
| typed/schema AST 遍历及静态依赖计划 | `coflow-cft::schema` | checker 只消费 lowered plan，不重新推导 schema 类型 |
| builtin identity、签名元数据和文档元数据 | `coflow-cft::schema` | checker 按稳定 ID 执行，LSP 只投影展示信息 |
| 求值值语义、预算收费和稳定 scalar 文本 | `coflow-checker` | runtime/host 不重新实现 check value formatting |
| check execution、动态依赖和 snapshot | `coflow-checker` | runtime 组装 model/change set 并映射诊断 |
| schema module source catalog 与 canonical 诊断映射 | `coflow-runtime` | CLI/LSP/editor 消费 canonical location，不猜测路径 |
| provider-neutral diagnostic/wire contract | `coflow-api` | host 只渲染，不反解析 message |

每个基础重构提交都必须提供三类证据：行为等价回归测试、新旧 consumer 已全部迁移的静态检索结果、旧类型/入口已删除的 diff。若确需短暂兼容层，只能在同一阶段的连续提交中存在，阶段最终提交前必须删除；跨阶段保留必须升级为计划中的显式公开兼容项，写明所有者、调用方、删除版本和测试，不能只留注释。

新增抽象还需满足以下约束：

- visitor/walker 使用 exhaustive enum dispatch，不能用默认分支吞掉未来 AST variant；需要跳过子树时由 visitor 明确返回控制结果。
- builtin registry 保存声明性元数据和稳定执行 ID，但不把复杂类型规则塞进字符串或回调闭包；共享的 receiver/element capability 由 typed helper 表达，特殊规则仍以 exhaustive ID dispatch 实现并有完整性测试。
- path dependency 使用规范化的 typed path segment，不以拼接字符串表示字段、array index 或 dict key；ancestor/descendant overlap 只有一个实现，并由 mutation、snapshot invalidation 共用。
- runtime mutation 影响以单一 canonical `MutationImpact` 保存：`records: BTreeMap<RecordCoordinate, ChangedPaths>`、record-set membership delta、受影响文件和结构变化原因。现有索引/生成所需的 record coordinate set 必须从该 map 的 key 投影，不能继续把 `changed_records` 与 path map 双写为两个事实来源。字段写入（包括 spread 后解析到实际 source record 的写入）产生规范化精确路径；插入、删除、rename、provider 无法报告细节的外部重写和 full reload 显式产生 `ChangedPaths::All`。reorder 只有在记录集合或数据语义确实变化时影响 check，不以 UI 顺序变化伪造字段路径。
- `MutationImpact` 到 `CheckChangeSet` 的转换是 runtime/checker 唯一边界转换，并由 schema assignability 一次性扩展 membership ancestor；load、session rebuild、preflight/staging 不得各自重建另一份 checker change set。转换测试必须覆盖同一 record 多次写入的 path union、`All` 吸收 `Paths`、rename 的 old/new identity、派生类型 membership 和 source-target 坐标重定向。
- snapshot 不缓存可从 canonical schema/model 低成本且确定性重建的派生状态；必须缓存的计划或索引要绑定 schema/model generation，并测试 generation 变化后的失效。
- 集合关系、record enumeration 和依赖匹配必须同时说明正确性复杂度与预算复杂度。实现不得留下无预算保护的隐藏二次扫描；若采用二次扫描是为了支持不可哈希的合法值，需有边界测试和注释说明，否则应使用受预算约束的 set/index。
- 不为尚未纳入范围的环检测、动态正则、计数量词或通用查询预留半成品 trait、AST variant、feature flag 或空 dispatch 分支。

阶段 review 使用统一完成清单：canonical model 已确定；所有 producer/consumer 已迁移；旧路径已删除；正向、负向、预算和兼容测试已覆盖；LSP/文档已同步或明确排入同一功能的后续提交；`git diff --check` 和该阶段定向测试通过。只有全部满足才进入下一阶段。

### 2.4 阶段依赖与禁止的临时实现

功能可以拆成多个可编译提交，但不能越过下列架构前置条件：

- f-string 进入 checker 前，scalar stable formatter 必须成为唯一的标量显示入口；进入所有 AST consumer 前，统一 visitor 必须就位。允许先提交 lexer/parser AST，但不得为赶进度在 budget、dependency 或 LSP 中新增一次性递归器。
- 新 builtin 进入公开 schema/LSP 前，必须先迁移到稳定 `BuiltinId` registry；`records(Type)` 是带静态 type operand 和 scope 限制的 schema special form，不注册为接收普通运行期参数的 builtin，也不走字符串类型名反射。
- 双 binding 在 parser 之后必须立即 lower 为一个带集合种类的 canonical binding layout。类型检查、dimension/dependency 分析、evaluator 和 diagnostic context 只消费该 layout，不允许分别通过 binding 数量重新猜 array/dict 语义。
- 顶层 check 的 parser/schema 声明可以先独立落地，但进入 checker 前必须已有统一的 `CheckScope`、`CheckExecutionId`、schema/data label 和 snapshot root。不得用 `current_type: Option<_>`、虚构 record id 或特殊空记录作为长期作用域/执行身份；显式 scope 至少区分 type-local 与 top-level，并集中定义裸字段、隐式 `id`、const、enum 和 binding 的可见性。
- `records(Type)` 进入 evaluator 前，data model 的稳定 assignable-record query、record-set dependency 和 membership change production 必须同时具备。不得先用全模型临时扫描加“任何修改全量重跑”作为可合并实现。
- path-level read dependency 进入 snapshot 前，mutation 侧必须使用同一套规范化 path 与 overlap 判定；两侧不能分别实现祖先/后代匹配。无法提供精确路径的正式入口显式产生 `ChangedPaths::All`，不能用空集合、根路径或哨兵字符串暗示全量变化。
- 顶层执行进入 runtime 前，type-local 执行必须已迁移到同一 runner/snapshot pipeline。两种 scope 允许在 root 枚举与诊断根位置上分支，不允许在 statement/expression evaluator、预算或动态依赖收集上分叉。

每一项前置重构使用“行为保持提交 + 功能提交”的方式落地；如果两者因 Rust 类型迁移无法合理拆开，则放在同一提交中，但必须同时包含旧路径删除和覆盖旧行为的回归测试。任何临时 adapter 都要在创建它的 patch 中标注用途，并在同一阶段退出条件中删除；计划不接受把内部 adapter 延后到最终审计才处理。

各阶段退出时除测试外执行针对性的静态审计：检查旧 symbol/file 是否仍被引用，检查新 enum 是否存在 wildcard/default dispatch，检查同一语义是否出现第二份名称表、formatter、walker、path overlap 或 snapshot state。审计命令及合理保留的命中写入提交说明或 PR checklist，使“不留技术债”有可复核证据。

## 3. 阶段一：表达式语句自定义消息

### 3.1 语法

```ebnf
check_expr_stmt = expression [ ":" check_message ] ";" ;
check_message   = STRING | FORMATTED_STRING ;
```

阶段一只实现静态字符串，格式化字符串在阶段二加入：

```cft
check {
  price > 0: "价格必须大于 0";
}
```

`when` 条件为假表示跳过，不是校验失败，因此 `when` 自身不接受消息。消息可用于 `when` 和量词内部的普通条件语句。

### 3.2 AST 与 schema

把 `CheckStmt::Expr(CheckExpr)` 调整为：

```rust
CheckStmt::Expr {
    condition: CheckExpr,
    message: Option<CheckMessage>,
    span: Span,
}
```

`CftSchemaCheckStmt` 做对应调整。消息使用独立的 `CheckMessage`/`CftSchemaCheckMessage` 表示并保留 span。阶段一的 message 只有静态字符串，不参与类型检查或运行期求值；阶段二加入格式化消息后才具有插值表达式类型和求值错误。

### 3.3 执行语义

- condition 为 `true`：不求值 message。
- condition 为 `false` 且没有自定义消息：保持现有自动诊断和 actual/expected 解释。
- condition 为 `false` 且存在自定义消息：`diagnostic.message` 只使用自定义消息，不追加表达式、actual/expected、dimension、`when` 或量词文本。
- condition 求值错误：忽略自定义消息并报告原始错误，不能用业务消息隐藏 null access、索引越界、字典缺 key、类型错误或预算错误。
- 阶段二的格式化 message 求值错误：报告消息表达式自身的真实求值错误。
- 保留当前针对比较、`contains`、`isUnique`、`matches` 等生成的专用错误码，以及 severity、primary location、逻辑数据路径和 related locations。
- `when`、量词、顶层 check 名称和 dimension round 使用结构化 diagnostic contexts 保存。`coflow-api` 作为 canonical diagnostics 所有者增加向后兼容的 `contexts` 字段，checker 内部诊断使用对应的 provider-neutral context enum，runtime 负责映射。CLI、人类输出和编辑器单独渲染 context，不修改 `diagnostic.message`；JSON 直接输出结构化 context。
- checker/runtime 内部 context 使用强类型 enum；canonical wire DTO 使用稳定 `kind: String` 加明确定义的可选字段，并添加 `#[serde(default, skip_serializing_if = "Vec::is_empty")]`。未知 kind 在反序列化时保留而不是报错；没有 context 的既有 JSON 字节形态保持不变。`FlatDiagnostic` 和 editor wire DTO 显式携带 contexts，不把它们预拼进 message。
- context kind 第一版固定为 `check`（顶层 check name）、`when`（条件表达式）、`quantifier`（kind、binding 及 index/key display）和 `dimension`（dimension、variant）；新增 kind 必须走兼容 wire 设计，不能退化成无结构的自由文本数组。
- `all` 中失败表达式保留各自的自定义消息和失败元素路径。
- `any` 的候选失败属于试算过程：全部候选均不匹配时继续使用量词级系统汇总，不暴露候选内部的自定义消息；候选中的真实求值错误仍立即报告。
- `none` 失败来自某个候选 body 成功，不存在内部失败消息，因此继续使用量词级系统汇总并定位到匹配元素。

建议输出：

```text
价格必须大于 0
```

### 3.4 相关代码

- `crates/coflow-cft/src/syntax/parser/check.rs`
- `crates/coflow-cft/src/syntax/ast.rs`
- `crates/coflow-cft/src/schema/declarations.rs`
- `crates/coflow-cft/src/schema/compiler/checks.rs`
- `crates/coflow-cft/src/schema/compiler/lower.rs`
- `crates/coflow-cft/src/schema/plans/typed_checks.rs`
- `crates/coflow-cft/src/schema/compiler/checks.rs`
- `crates/coflow-checker/src/engine/statements.rs`
- `crates/coflow-checker/src/diagnostics/mod.rs`
- `crates/coflow-checker/src/diagnostics/explanations.rs`
- `crates/coflow-api/src/diagnostics.rs`
- CLI/JSON 与 editor diagnostic context renderer/wire DTO
- `crates/coflow-lsp/src/semantic_tokens.rs`
- `crates/coflow-lsp/src/formatting.rs`

本阶段先完成 diagnostic pipeline 重构再接入自定义消息：删除 checker 中把 `when`、量词和 dimension 文本追加到 message 的旧路径；新增 context 后由 human renderer 恢复等价的默认可读输出。`CheckMessage` 是 condition 附属语法节点，不把 message 伪装成普通 statement，也不为静态字符串和格式化字符串建立两套 statement variant。

### 3.5 测试

- 旧语法仍可解析、编译和执行。
- true 条件不产生诊断，也不求值消息。
- false 条件的最终 message 只包含自定义内容，不残留自动表达式或 actual/expected 解释。
- 自定义消息不改变原错误码、severity、primary location、逻辑数据路径或 related locations。
- condition 求值错误不会被自定义消息覆盖；格式化 message 求值错误在阶段二测试。
- `all` 保留内部自定义消息；`any`/`none` 按量词级汇总规则处理，真实求值错误不被试算吞掉。
- 缺少消息、消息不是字符串字面量、字符串转义错误、缺少分号时错误恢复稳定。
- `when`、量词、check name 和 dimension round 写入结构化 contexts，最终 message 仍与自定义文本完全相同。
- canonical/API JSON、CLI human renderer、editor flat/wire view 都覆盖 contexts 的兼容序列化与展示。
- 增量快照能够稳定保存和重新渲染消息。

## 4. 阶段二：格式化字符串

### 4.1 语法与兼容性

只有 `f"..."` 启用插值；普通字符串永远按字面值处理：

```cft
f"物品 {id} 的价格为 {price}"
f"字面花括号：{{name}}"
```

格式化字符串是通用 check 字符串表达式，不局限于诊断消息：

```cft
id == f"{category}_{level}";
```

词法边界固定如下：

- `f` 必须与开引号直接相邻；`f "..."` 不是格式化字符串。
- 插值使用 `{ expression }`，允许现有 check expression、普通字符串、字段/索引访问和方法调用。
- 第一版禁止插值中嵌套格式化字符串，避免双层插值恢复语义。
- 格式化字符串沿用普通字符串的单行和转义规则；插值内不允许换行或 `#` 注释。
- lexer 进入 formatted-string 模式后产出 text/interpolation 边界 token；插值部分复用正常 check tokenization，并在字符串/括号/方括号嵌套之外的首个 `}` 结束。
- `{{`、`}}` 仅在 text 模式表示字面花括号。
- 未闭合插值优先恢复到当前字符串结尾，不能吞掉后续 check statement 或 type item。

### 4.2 表示形式

AST 和 schema AST 保存结构化片段：

```rust
enum FormatSegment {
    Text(String),
    Expr(CheckExpr),
}
```

不得在 checker 运行时重新扫描普通字符串。结构化表示使插值表达式能够参与名称解析、类型检查、结构预算、数据依赖收集和增量失效分析。

### 4.3 类型与求值规则

第一版允许格式化：

- `null`
- `bool`
- `int`
- `float`
- `string`
- enum

object、array、dict 不可直接格式化；引用对象应显式取 `id` 或字段：

```cft
f"{item.id}" # 合法
f"{item}"    # 静态类型错误
```

其他规则：

- `{{`、`}}` 表示字面花括号。
- 插值表达式失败时报告原始求值错误，不替换为空字符串。
- 输出长度计入 check evaluation work budget。
- float 和 enum 使用 checker 现有稳定诊断格式，避免两套显示规则。
- message 只在 condition 为 false 时求值。
- `matches` 在动态正则模板阶段完成前继续拒绝 formatted-string pattern，不能把它当作普通运行期字符串绕过 schema 正则验证。

### 4.4 相关代码

- `crates/coflow-cft/src/syntax/lexer/tokens.rs`
- `crates/coflow-cft/src/syntax/lexer/mod.rs`
- `crates/coflow-cft/src/syntax/parser/check_primary.rs`
- `crates/coflow-cft/src/syntax/parser/budget.rs`
- `crates/coflow-cft/src/syntax/ast.rs`
- `crates/coflow-cft/src/schema/declarations.rs`
- `crates/coflow-cft/src/schema/compiler/checks.rs`
- `crates/coflow-cft/src/schema/compiler/lower.rs`
- `crates/coflow-checker/src/engine/expressions.rs`
- `crates/coflow-checker/src/engine/evaluator.rs`
- `crates/coflow-checker/src/diagnostics/mod.rs`
- `crates/coflow-lsp/src/state.rs`
- `crates/coflow-lsp/src/semantic_tokens.rs`
- `crates/coflow-lsp/src/completion.rs`
- `crates/coflow-lsp/src/definition.rs`
- `crates/coflow-lsp/src/formatting.rs`

本阶段同时提取两项公共设施：syntax/schema check AST visitor，以及 scalar stable formatter。lexer 只负责产生结构化 token，parser 只负责形成 segment AST，lowering 只做同构转换；类型检查、依赖分析和 evaluator 不得重新扫描源字符串。完成后删除任何临时的 formatted-string 专用递归 walker 和重复 value renderer。

### 4.5 测试

- 空字符串、单/多插值、字段链、const、enum、量词变量。
- `f` 邻接、`{{`、`}}`、字符串转义、插值内普通字符串/括号/索引、嵌套 f-string 拒绝、未闭合花括号和空 `{}`。
- 未知名称及不可格式化类型。
- 插值 AST 深度预算和输出长度预算。
- 未执行的消息不产生错误或依赖。
- 引用字段改变后相关 check 被增量重跑。
- LSP 插值内部 completion、definition 和 semantic token。

## 5. 阶段三：低风险内置函数

当前内置函数注册表位于 `crates/coflow-cft/src/schema/check_builtins.rs`，现有函数为：

```text
len contains isUnique min max sum keys values matches
```

### 5.1 第一批函数

| 方法 | receiver / 参数 | 返回值 | 说明 |
| --- | --- | --- | --- |
| `string.len()` | string | int | 扩展现有 `len` |
| `string.contains(x)` | string, string | bool | 扩展现有 `contains` |
| `string.startsWith(x)` | string, string | bool | 前缀判断 |
| `string.endsWith(x)` | string, string | bool | 后缀判断 |
| `string.isBlank()` | string | bool | 空串或全为空白 |
| `number.abs()` | int / float | 原类型 | 绝对值 |
| `float.isFinite()` | float | bool | 排除 NaN 和 infinity |
| `float.approxEqual(other, epsilon)` | float, float, float | bool | 浮点近似相等 |
| `dict.containsKey(key)` | dict, key | bool | 明确的 key 判断 |
| `dict.containsValue(value)` | dict, value | bool | value 判断 |

`dict.contains(x)` 保持既有“查 key”语义，新代码推荐 `containsKey`。

需要在实施前固定以下细节：

- `string.len()` 建议返回 Unicode scalar count，而不是 UTF-8 byte count，并写入公开文档。
- `isBlank()` 按 Rust/Unicode whitespace 语义实现。
- `approxEqual` 使用 `abs(receiver - other) <= epsilon`；epsilon 必须非负且有限，NaN 永远 false。
- `approxEqual` 是绝对误差比较；有限操作数相减溢出为 infinity 时返回 false，不产生非有限临时值诊断。相对误差如有需求必须使用独立函数设计，不能暗中改变该语义。
- `abs(i64::MIN)` 返回有位置的溢出诊断，不能 panic。
- data model 已拒绝非有限持久化 float；`isFinite()` 主要用于检查除法、幂等表达式产生的临时结果，文档不得暗示字段可合法存储 NaN/infinity。
- nullable receiver 继续要求调用前显式处理 null，除非后续使用 `?.`。
- `containsValue` 逐项使用与 `==` 相同的静态兼容性和运行语义，按检查的元素数收费。

### 5.2 第二批集合函数

| 方法 | 说明 |
| --- | --- |
| `isSorted()` | 非递减排序 |
| `isStrictlySorted()` | 严格递增排序 |
| `intersects(other)` | 至少一个共同元素 |
| `isDisjoint(other)` | 没有共同元素 |
| `isSubsetOf(other)` | 子集关系 |
| `isSupersetOf(other)` | 超集关系 |

集合关系函数采用数学集合语义，重复次数不影响结果：`[1, 1]` 与 `[1]` 互为子集。第一版支持 int、bool、string、enum 及其 nullable 形式，null 作为普通集合元素参与相等关系；float、object、record reference、array 和 dict 不支持集合关系。

排序函数采用序列语义并检查原始顺序：

- 支持非 nullable int、bool、string 和 enum 数组；nullable element type 在 schema 编译期拒绝。
- bool 顺序固定为 `false < true`。
- string 使用 Rust `str::cmp` 的 UTF-8 词典序；由于合法 UTF-8 的编码顺序保持 Unicode scalar 顺序，文档表述为 Unicode scalar lexicographic order，不承诺 locale collation 或 grapheme order。
- enum 按已解析的底层数值排序，同值 variant 视为相等。
- `isSorted` 允许相邻相等，`isStrictlySorted` 不允许。

集合关系实现按 lhs 与 rhs 实际扫描/建索引的元素数收取 work budget，并按临时集合元素数收取结构预算；不得用未计费的复制、排序或哈希构建绕过预算。

### 5.3 相关代码

- `crates/coflow-cft/src/schema/check_builtins.rs`
- `crates/coflow-cft/src/schema/compiler/check_functions.rs`
- `crates/coflow-checker/src/operations/builtins.rs`
- `crates/coflow-checker/src/diagnostics/explanations.rs`
- `crates/coflow-lsp/src/completion.rs`
- `crates/coflow-lsp/src/documentation.rs`

实施内置函数前先把现有函数迁入带稳定 `BuiltinId` 的 registry metadata；checker 按 `BuiltinId` dispatch，LSP 从同一 metadata 生成签名和文档关联。迁移必须保持现有函数行为和错误码，并删除按函数名散落匹配的旧分发表。集合函数需要的比较/相等/hash 能力通过明确的 typed value capability helper 提供，不在每个函数中重复推断支持类型。

### 5.4 测试矩阵

每个函数覆盖：正常值、边界值、nullable receiver、错误 receiver 类型、参数类型、arity、空集合/字符串、预算和精准失败诊断。新增或细分错误码时同步更新 error coverage 测试和诊断码索引。

## 6. 阶段四：安全 nullable 操作

### 6.1 语法

```cft
target?.level
values?[index]
value ?? fallback
```

### 6.2 类型与运行规则

- `nullable<T>?.field` 返回 nullable field type。
- `nullable<collection>?[index]` 返回 nullable element/value type。
- 非 nullable receiver 使用 `?.` 或 `?[...]` 给出静态错误。
- `nullable<T> ?? fallback` 要求 fallback 可赋给 `T`，结果为非 nullable `T`。
- `??` 右结合，优先级低于 `||`；涉及比较时推荐显式括号：`(cooldown ?? 0.0) >= 0.0`。
- 安全访问只传播 receiver 的 null，不吞索引越界、字典缺 key、未解析引用或类型错误。
- `??` rhs 短路，未执行 rhs 不计求值工作，也不收集数据依赖。

需要新增 token、parser precedence、AST/schema expression variants、inferred type 规则、lowering、runtime short-circuit、表达式渲染、semantic token 和 formatter 支持。

测试 null/非 null 分支、嵌套引用、短路依赖、数组越界、字典缺 key、运算符优先级、dimension overlay 及预算。

## 7. 阶段五：量词 binding 增强

保留现有单 binding：

```cft
all item in items { ... }
```

增加第二 binding。array 沿用“元素、索引”的阅读顺序，dict 使用惯用的“key、value”顺序，不强求两种集合采用同一抽象顺序：

```cft
all item, index in items {
  item.count > 0: f"第 {index} 项无效";
}

all key, value in resistances {
  0.0 <= value <= 1.0: f"{key} 的值 {value} 无效";
}
```

类型规则：

- array 第一 binding 为 element type，第二 binding 为 `int` index。
- dict 第一 binding 为 key type，第二 binding 为 value type。
- 单 binding 遍历 dict 时继续保持现有 entry `.key/.value` 语义。
- 两个 binding 都参与保留名、重名、作用域和 LSP 分析。

AST/schema 的 quantifier binding 从单值改为显式 binding list，但 parser 只接受 1 或 2 个 binding。checker context 同时保存 display binding 和 index/key/value 的数据位置，确保自定义消息和失败路径使用相同迭代项。

内部不使用两个 `Option<String>` 或 array/dict 特判字段长期表达 binding，而使用带集合种类验证后的显式 binding descriptor。type analyzer 负责一次性把语法 binding 降低为 array/dict 的 typed binding layout；evaluator、dependency planner、diagnostic context 和 LSP 都消费该 layout，避免各自重新解释“第一个/第二个参数”的含义。旧单 binding 在 lowering 时进入同一 layout，不保留第二条执行路径。

相关实现至少覆盖 `syntax/parser/check.rs`、AST/schema declarations、check type analyzer、lowering、dimension/dependency plan、checker quantifier operations/statements、diagnostic renderer，以及 LSP state/completion/semantic tokens/formatting。

测试除正常 array/dict 外，还必须覆盖空集合、嵌套量词、binding 重名、与外层 binding 冲突、错误 binding 数量、array index 从 0 开始、enum dict key、message 插值、dimension binding type 传播、量词预算和旧单 binding schema API 兼容迁移。

计数量词不属于本计划。后续如需 `exactly/atLeast/atMost`，必须先独立定义 body 多语句匹配、空集合、计数表达式、失败聚合、元素定位和预算语义。

## 8. 阶段六：命名顶层 check 与全类型记录访问

### 8.1 语法

类型内 check 保持现有匿名语法和隐式当前记录：

```cft
type Item {
  price: int;

  check {
    price > 0;
  }
}
```

顶层 check 必须命名：

```cft
check ItemIntegrity {
  records(Item).len() > 0:
    "项目中至少需要配置一个物品";

  all item in records(Item) {
    item.price > 0:
      f"物品 {item.id} 的价格必须大于 0";
  }
}
```

建议语法：

```ebnf
top_level_check = "check" IDENT "{" check_stmt* "}" ;
record_set_expr = "records" "(" TYPE_NAME ")" ;
```

顶层 check 名称在项目内唯一，并使用独立的 check namespace，不与 const、enum 或 type 共用全局值/类型命名空间。名称作为 schema、增量快照、诊断和 LSP symbol 使用的稳定 identity。

`records(Type)` 外观类似调用，但属于 compiler 特殊形式：参数必须是静态 type name，不是运行时表达式或字符串。这样不引入 generic expression 语法，也不把 type namespace 普遍转换成普通值。

`records` 是仅在顶层 check 的 call position 生效的上下文关键字，不加入全局保留标识符集合；既有同名字段/type 不受影响。`records(Type)` 在 type-local check 中静态拒绝，避免每条记录启动一次全局扫描并产生隐式 O(n²) 行为。

### 8.2 类型和作用域语义

顶层 check 没有隐式当前记录，因此不能直接使用裸 `id` 或类型字段。它可以访问：

- const 和 enum variant；
- `records(Type)`；
- `when` 和量词 binding；
- 从 binding 或引用对象继续访问的字段；
- 普通 check 运算符和内置函数。

错误和正确示例：

```cft
check ItemIntegrity {
  price > 0; # 错误：顶层 check 中没有隐式 Item

  all item in records(Item) {
    item.price > 0; # 正确
  }
}
```

`records(BaseType)` 返回声明类型为 `BaseType` 或其任意子类型的全部顶层记录，概念返回类型为 `[&BaseType]`。这保证 abstract base type 可以用于跨多态记录校验。第一版不提供 `recordsExact(Type)`。

`records(Type)` 只枚举项目数据模型中的顶层记录，不枚举内联 object。singleton 如果存在对应记录，则作为一个成员返回。返回顺序必须稳定，建议采用 data model 的稳定 type/key 顺序，确保诊断和测试可重复。

`coflow cft check`、schema 和 codegen 只编译及类型检查顶层规则，不执行数据遍历；project check/build/export 和会加载完整项目数据的 data 命令按现有 check 执行边界运行顶层规则。

### 8.3 Schema 与 API 表示

AST 顶层 item 增加命名 check 定义；schema 增加带稳定名称、module/span 和 statements 的 top-level check 集合。schema API 需要提供：

- 按名称查询顶层 check；
- 稳定迭代全部顶层 check；
- 查询 check 静态引用的 record-set 类型；
- 区分 type-local check 与 top-level check。

`records(Type)` 在 schema expression 中应使用专用 variant，而不是保留为普通字符串函数调用：

```rust
CftSchemaCheckExprKind::Records {
    type_name: TypeName,
}
```

这样 checker、依赖计划和 API consumer 不需要通过函数名反向识别集合依赖。

`type_name` 在 lowering 后必须是 schema 已解析的稳定类型 identity，而不是等待 checker 再按文本查找的名称。syntax AST 保留用户书写及 span 用于 definition/diagnostic，schema AST 只保留已验证 identity 与源 span；重名、import/namespace 解析完全复用普通类型引用规则。

schema 中每个顶层 check 和每条 statement 必须保留 `ModuleId + Span`。project runtime 同时保留用于 schema 编译的 module source catalog（module、path、source text），由应用服务在输出边界把 schema span 转换为 `coflow-api::SourceLocation::FileSpan`。不得使用 check name 猜测文件，也不得把某条业务记录伪装成 schema 位置。

顶层 type analyzer 使用显式 `CheckScope::TopLevel`，不复用一个虚构 `TypeInfo`。名称解析公共逻辑拆成 current-record、lexical binding、const/enum/type-special-form 四类；type-local analyzer 使用 `CheckScope::Record(TypeName)`。这样顶层裸字段拒绝、`records(Type)` 类型参数和后续 LSP scope 使用同一语义来源。

### 8.4 Checker 与增量执行

现有 type-local diagnostics 和 snapshot 以 record root 为主要 identity；本阶段将 checker execution、diagnostic ownership、dependency graph 和 snapshot root 统一重构为：

```rust
enum CheckExecutionId {
    Record(RecordCoordinate),
    TopLevel(TopLevelCheckName),
}

struct CheckRoot {
    execution: CheckExecutionId,
    round: CheckRound,
}

struct RecordSetDependency {
    type_name: TypeName,
    include_derived: bool,
}

struct RecordReadDependency {
    record: RecordCoordinate,
    path: CheckValuePath,
}

struct RootCheckState {
    diagnostics: Vec<LogicalCheckDiagnostic>,
    reads_from: BTreeSet<RecordReadDependency>,
    record_sets: BTreeSet<RecordSetDependency>,
}
```

`CheckValuePath` 是 checker/runtime 共用的规范化 typed path，segment 显式区分 field、array index 和 dict key；它不能是展示用字符串，也不能直接复用带 provider 物理路径语义的类型。显示路径由单独 formatter 从 typed segment 生成，依赖比较不经过文本格式化。

type-local 和 top-level 执行都使用这一套 identity，不并行保留第二套 snapshot。公开 `CheckRequest`、`CheckTargets`、`RootedCheckDiagnostic`、execution stats 和 snapshot merge/replace API 同步升级，避免 runtime 通过额外 side table 拼接顶层结果。

该重构先以 record-only 行为完成一次等价迁移：引入新 identity/change/dependency 类型，迁移 checker、snapshot、runtime session 和所有调用方，删除旧字段后通过既有增量测试；随后才加入 top-level variant 和 membership dependency。这样可以把“执行模型迁移错误”与“顶层语义错误”分开定位，同时最终实现仍只有一套 engine。

`CheckRequest::all()` 同时选择全部 record roots 和全部 top-level roots；incremental request 由 snapshot 的 `affected_roots` 精确生成两类 target。execution stats 增加明确的 `executed_top_level_checks` 和 `record_set_members_materialized`，不把顶层执行伪装成 record/check round 数量。

顶层 check 至少收集两类依赖：

1. 类型成员集合依赖：`records(Item)` 的成员新增、删除、改 key，或 Item 子类型成员变化时重跑。
2. 实际读取依赖：执行过程中读取的具体记录路径和引用目标路径变化时重跑。

record-set dependency 由统一 schema visitor 从 lowered check block 静态收集，即使 `records(Type)` 位于当前轮短路或未进入的分支也附加到 root state，保证成员变化后能够重新判断分支；具体 record/path read 仍只记录实际执行的读取。

只记录当前成员的 record ID 不足以覆盖新增记录。runtime 的增量输入从单一 changed-coordinate set 重构为：

```rust
struct CheckChangeSet {
    records: BTreeMap<RecordCoordinate, ChangedPaths>,
    memberships: BTreeSet<TypeName>,
}

enum ChangedPaths {
    All,
    Paths(BTreeSet<CfdPath>),
}
```

runtime 在旧/新 record index 或 mutation execution result 边界计算 membership delta，覆盖新增、删除、改 key、实际类型变化，并把受影响类型的全部 inheritance ancestors 加入 `memberships`。checker 不通过查看新 model 猜测已删除记录的旧类型。

现有 record-level `reads_from` 同步升级为 path-level dependency。evaluator 在字段、索引、引用解引用和 dimension overlay 读取点记录稳定 `RecordCoordinate + CfdPath`；runtime mutation plan 能提供精确路径时写入 `ChangedPaths::Paths`，全量 reload、来源重排或无法证明精确 diff 时使用 `ChangedPaths::All` 安全回退。path overlap 使用 ancestor/descendant 前缀规则，例如替换 `stats` 必须使读取 `stats.hp` 的 check 失效。`affected_roots` 同时匹配 path reads 和 record-set dependencies。

集合结构修改按容器路径失效：array 插入/删除/重排标记 array 字段路径，避免索引位移漏检；dict 单 value 修改可以标记具体 key path，key 新增/删除同时标记 dict 容器路径；引用目标改变标记引用字段路径。dimension variant 修改携带逻辑字段路径与 dimension round，不用物理 overlay 存储路径作为稳定 identity。

`records(Type)` 使用 data model 的 assignability 查询，但返回顺序固定为 `(actual_type, record_key)` 升序，不依赖 provider/input insertion order。为避免每次表达式临时排序，`coflow-data-model` 增加基于正式 type/key index 的稳定 assignable-record query API；永久索引在 data model 构建阶段按既有模型预算管理，check 查询遍历按成员数收取 work budget，返回的临时 EvalValue 集合按成员数收取 structure budget。

`records(Type)` 的元素在 evaluator 中表示为带 `RecordCoordinate`、实际类型和逻辑根路径的只读 record handle，而不是把 record object 深拷贝成普通 `EvalValue::Object`。字段访问、safe access、引用解引用、量词 context 和 diagnostic label 都从该 handle 延伸路径并收集依赖；handle 不跨 model/schema generation 持久化，snapshot 只保存稳定 identity、依赖和逻辑诊断。这样既避免大对象复制，也不会丢失读取来源。record handle 与普通内联 object 的静态可访问字段规则共用同一 typed field resolver，但运行期 provenance 明确区分。

执行和诊断要求：

- 每个顶层 check 每轮最多执行一次，不从每条成员记录重复启动。
- 量词失败仍指向具体失败元素的逻辑数据路径。
- 有具体失败元素时，数据位置为 primary，CFT statement source span 作为 related schema label。
- 没有具体元素位置的集合级失败，以 CFT statement 的 `ModuleId + Span` 为 primary，不创建虚假 record/path。
- 自定义消息继续遵循“false 时覆盖自动解释、求值错误不覆盖”的规则。
- 检查成员枚举和遍历均计入结构/工作预算。
- checker 内部引入独立 `CheckDiagnostic`/`CheckDiagnosticLabel`，label 明确区分 `Data(ValueLocation)` 与 `Schema(ModuleId, Span)`；`coflow-checker` 不依赖 `coflow-api`，project runtime/application service 负责映射到 canonical `coflow-api::Diagnostic`。
- snapshot 的 logical label 同时支持稳定 data coordinate 和稳定 schema source ref。记录删除后，仅引用已删除数据位置的旧诊断失效；schema-primary 的集合级诊断仍可恢复。schema/module generation 变化使整个 snapshot 不可复用，不尝试跨 schema 版本猜测 span。

### 8.5 Dimension 语义

不采用“所有顶层 check 在所有 dimension round 全跑”的临时方案。dimension schedule 在 check type analyzer 已经持有完整 receiver/binding 类型时编译，并 lower 到 type-local 与 top-level 共用的 `CftSchemaCheckBlock`：

- lexical scope 保存 binding 名及完整静态类型，不再只保存名称。
- field access 根据 receiver 的 object/reference type 解析目标字段及 dimension。
- quantifier 根据 array/dict/`records(Type)` 推导一个或两个 binding 类型。
- formatted-string 插值、message、safe access、builtin receiver/args 和嵌套字段链全部进入同一遍历。
- type-local 与 top-level check 共用分析器，仅 current-record scope 不同。
- typed traversal 产出 statement dimensions；静态 record-set dependencies 由 lowered schema visitor 收集。两者复用各层 canonical traversal，不新增第二套类型推断或近似 AST walker。

只有静态计划涉及的 dimension 才执行对应 statement。运行期实际 read dependency 继续负责增量失效，但不能替代静态 dimension schedule，因为未执行分支在其他 variant 中可能变为可达。

baseline 与 dimension round 的诊断去重、来源附加和消息覆盖规则必须与 type-local check 一致。`records(Type)` 的成员集合不因 dimension value overlay 改变，但成员字段值可以改变。

### 8.6 相关代码

- `crates/coflow-cft/src/syntax/ast.rs`
- `crates/coflow-cft/src/syntax/parser/definitions.rs`
- `crates/coflow-cft/src/syntax/parser/check.rs`
- `crates/coflow-cft/src/schema/declarations.rs`
- `crates/coflow-cft/src/schema/compiler/entry.rs`
- `crates/coflow-cft/src/schema/compiler/symbols.rs`
- `crates/coflow-cft/src/schema/compiler/budget.rs`
- `crates/coflow-cft/src/schema/compiler/checks.rs`
- `crates/coflow-cft/src/schema/compiler/lower.rs`
- `crates/coflow-cft/src/schema/plans/typed_checks.rs`
- `crates/coflow-cft/src/schema/compiler/checks.rs`
- `crates/coflow-cft/src/schema/queries.rs`
- `crates/coflow-data-model/src/model/mod.rs` 及稳定 assignable-record index/query
- `crates/coflow-checker/src/dependencies.rs`
- `crates/coflow-checker/src/request.rs`
- `crates/coflow-checker/src/output.rs`
- `crates/coflow-checker/src/engine/runner.rs`
- `crates/coflow-checker/src/engine/expressions.rs`
- `crates/coflow-checker/src/snapshot.rs`
- `crates/coflow-api/src/diagnostics.rs`
- `crates/coflow-runtime/src/checks.rs`
- `crates/coflow-runtime/src/load.rs`
- `crates/coflow-runtime/src/session.rs`
- `crates/coflow-runtime/src/session_build.rs`
- `crates/coflow-lsp/src/document_symbols.rs`
- `crates/coflow-lsp/src/completion.rs`
- `crates/coflow-lsp/src/definition.rs`
- `crates/coflow-lsp/src/semantic_tokens.rs`

### 8.7 测试

- 顶层命名 check 的 parser、重复名称和错误恢复。
- check name、`records` 采用上下文关键字解析，不把同名字段/type 全局变成非法标识符；顶层语境之外保持兼容。
- 顶层作用域拒绝裸字段/id，接受 const、enum、binding 和引用字段。
- `records` 拒绝未知类型、primitive、enum、字符串和动态参数。
- `records(Type)` 按普通类型引用规则解析 namespace/import，schema API 暴露 resolved type identity 而不是未解析文本。
- abstract/concrete base 均包含派生类型记录。
- 空集合、singleton、稳定遍历顺序和跨类型引用访问。
- record handle 字段链保留实际记录坐标和 typed path；内联 object 不被误登记为 record-set 成员，record object 不发生深拷贝。
- `records(Type)` 在 type-local check 中拒绝；同名普通字段/type 保持兼容。
- `coflow cft check` 只做 schema/type validation，project check/build/export/data 按既有边界执行顶层规则。
- 新增、删除、改 key、实际类型和字段路径修改触发正确的增量重跑；parent/child path overlap 与 `ChangedPaths::All` 回退均覆盖。
- 实际类型变化和派生类型成员变化会使所有相关 ancestor record-set dependency 失效。
- 无关类型、无关记录或同一记录未读取且不重叠的字段路径修改不触发重跑。
- baseline、localized dimension overlay 和增量 dimension round 一致。
- 顶层 snapshot、诊断位置稳定化、JSON CLI 输出和 LSP symbol。
- data-primary/schema-related、schema-primary、structured contexts 及 module source catalog 到 `FileSpan` 的映射。
- 成员数量与量词遍历预算耗尽时产生稳定诊断而不 panic。

## 9. 动态 `matches` 的后置设计

格式化字符串首版用于精确 ID 更合适：

```cft
id == f"{category}_{level}";
```

阶段二不直接开放 `matches(f"...")`。现有 `matches` 要求静态字符串字面量，从而可在 schema 编译期验证正则。动态正则涉及正则注入、运行期非法 pattern、重复编译和插值转义语义。

后续可单独实现正则模板：

```cft
id.matches(f"^{category}_[a-z]+_{level}$");
```

固定语义：

- 固定文本片段是正则语法。
- 插值结果经过 `regex::escape`，只作为字面文本。
- schema 保存正则模板片段，不能先拼成普通字符串。
- 固定结构在 schema 编译期验证。
- 最终 pattern 长度和 regex size 受预算限制。
- 如需缓存，只能使用有界缓存。
- 第一版不提供插入任意动态正则片段的 escape hatch。

动态正则模板明确不属于本轮实现和验收范围。本轮只保证普通字符串 pattern 的既有行为，以及 formatted string 在 `==`、`!=` 等普通字符串位置可用；`matches(f"...")` 必须稳定地产生静态类型/编译诊断。若后续立项，应形成独立计划、错误模型和性能/安全评审，不在本轮留下半实现 token、AST variant、feature flag 或不可达 checker 分支。

## 10. LSP 与编辑器同步要求

每阶段同步检查：

- semantic tokens：消息、`f`、插值表达式和新操作符。
- completion：插值作用域、新内置函数、顶层 `check Name` 和 `records(Type)`。
- hover：函数签名、返回类型、nullable 行为。
- definition：插值内字段和 const 跳转，以及 `records(Type)` 的类型定义跳转。
- document symbols：命名顶层 check 作为独立 symbol 展示。
- diagnostics：schema-source label、data-source label 和 structured contexts 均映射到协议范围，不把 context 重写进自定义 message。
- formatting：冒号消息换行、f-string 保持和量词缩进。
- parser recovery：损坏的消息或插值不能破坏后续字段、type 或 check block。

重点文件：

- `crates/coflow-lsp/src/semantic_tokens.rs`
- `crates/coflow-lsp/src/state.rs`
- `crates/coflow-lsp/src/completion.rs`
- `crates/coflow-lsp/src/definition.rs`
- `crates/coflow-lsp/src/hover.rs`
- `crates/coflow-lsp/src/documentation.rs`
- `crates/coflow-lsp/src/formatting.rs`
- `crates/coflow-lsp/src/tests/cft.rs`

## 11. 总体测试计划

每阶段至少覆盖以下层级：

1. Lexer/parser：`crates/coflow-cft/tests/syntax.rs`、`parser_precedence.rs`、`parser_budget.rs`。
2. 静态类型：`crates/coflow-cft/tests/type_checker.rs`。
3. Schema API/lowering：`crates/coflow-cft/tests/schema_api.rs`。
4. Checker 行为：`crates/coflow-checker/tests/check.rs`。
5. 结构和求值预算：`crates/coflow-checker/tests/budgets.rs`。
6. 错误码覆盖：两个 crate 的 `error_coverage.rs`。
7. 维度执行：`crates/coflow-checker/tests/multi_language.rs`。
8. LSP：`crates/coflow-lsp/src/tests/cft.rs` 和 `tests/cli_lsp.rs`。
9. CLI 端到端：`tests/cli_check.rs`。
10. 增量依赖、diagnostic snapshot 稳定化和恢复渲染测试。
11. `coflow-api::Diagnostic` context 的 serde backward compatibility、CLI human rendering、JSON shape 和 editor wire compatibility。
12. runtime module source catalog、schema span 到 `FileSpan` 的 UTF-8/Unicode 行列转换及 schema generation 失效。

正常开发提交在仓库根目录运行：

```powershell
cargo check --workspace
cargo test --workspace
```

按项目约定，正常开发不把 `cargo fmt` 或 `cargo clippy` 作为提交门禁；release/packaging 使用 `AGENTS.md` 中的完整 gate。

## 12. 文档更新

功能落地时同步更新：

- `website/docs/docs/reference/03-language/04-check.md`
- `website/docs/docs/reference/03-language/01-cft.md`
- `website/docs/docs/reference/02-project-pipeline.md`
- `website/docs/docs/reference/11-schema-api.md`
- `website/docs/docs/reference/12-architecture.md`
- `website/docs/docs/reference/09-diagnostics/02-codes.md`
- LSP 内建函数 documentation

公开文档必须明确：

- 自定义消息只覆盖 condition 返回 false 时的自动可读解释，不覆盖错误码、位置、逻辑路径、related locations、执行上下文或任何求值错误。
- 自定义 message 字段保持原文；check name、`when`、量词和 dimension 通过 canonical structured contexts 表达，并分别说明 CLI、JSON、LSP/editor 的渲染形式。
- f-string 支持的类型、求值时机和转义。
- Unicode `len` 的单位。
- nullable 操作只传播 null，不吞其他错误。
- 集合函数对 null、空集合、重复值和 float 的行为。
- array 双 binding 固定为 `item, index`，dict 双 binding 固定为 `key, value`，并说明单 binding 的兼容行为。
- type-local check 与命名顶层 check 的作用域差异。
- `records(Type)` 包含派生类型、只枚举顶层记录、使用稳定顺序以及不包含内联 object。
- 顶层 check 的执行时机、dimension 行为、集合成员依赖和增量重跑边界。
- Schema API 中顶层 check identity、查询接口和静态 record-set dependency 表示。

release/packaging 时，如果 `website/docs/docs/reference/` 或 `skills/` 发生变化，按仓库要求执行并提交 skill reference 同步结果。

## 13. 推荐提交顺序

1. 将 check diagnostic context 从 message 拼接重构为 checker 内部结构化 context，并扩展 canonical `coflow-api::Diagnostic`、CLI/JSON/editor renderer；迁移现有 `when`、量词和 dimension 行为，删除旧拼接路径并保持人类输出兼容。
2. 表达式语句静态自定义消息。
3. 提取 syntax/schema check AST visitor 和 scalar stable formatter，再实现通用 f-string 与格式化消息；删除临时专用 walker/renderer。
4. 把现有 builtin 迁入 `BuiltinId` + registry metadata 并完成行为等价测试。
5. 基于统一 registry 实现字符串、数值、字典和集合内置函数及预算。
6. `?.`、`?[...]` 和 `??`，同步统一 visitor、类型系统、依赖和 formatter。
7. 引入 typed binding layout，再实现量词双 binding，并删除单 binding 的独立执行路径。
8. 在 check type analyzer 的 typed traversal 中生成 canonical block dimension schedule，先覆盖现有 type-local check，用回归测试证明行为一致后删除 `dimension_checks` 和中间 dependency planner。
9. 将 execution/dependency/snapshot 以 record-only 行为迁移为 `CheckExecutionId`、schema/data diagnostic labels、path-level record read dependency 和 `CheckChangeSet`；迁移全部调用方并删除旧 record-only/coordinate-only state。
10. runtime module source catalog、schema span canonical 映射和 path-aware mutation/full-reload change production。
11. 命名顶层 check 的 parser、schema 表示、Schema API 和 LSP symbol/scope。
12. `records(Type)` 稳定 data-model query、checker 执行、membership delta/dependency、snapshot 和精确 dimension schedule。
13. 顶层 check 的 CLI/JSON/editor 端到端测试、公开参考文档和最终技术债审计。

格式化字符串、nullable 操作和量词增强都会同时影响 AST、schema AST、type checker、evaluator 和 LSP，不应合并为一个大提交。每个阶段应先固定语义和错误行为，再进入下一阶段。

重构迁移矩阵用于约束每个阶段的删除工作：

| 引入的 canonical 能力 | 必须迁移的 consumer | 同阶段必须删除或收敛的旧实现 |
| --- | --- | --- |
| `CheckDiagnosticContext` 与 schema/data label | checker、runtime、CLI/JSON、LSP、editor、snapshot | message context 拼接/解析、host 自建 context、仅 data-coordinate 的 label 假设 |
| syntax/schema visitor 与 scalar formatter | budget、lowering、dependency plan、LSP、f-string、diagnostic explanation、quantifier display | 各功能专用递归 walker、重复 scalar renderer；path key formatter仅在转义规则确实不同处保留并复用 scalar primitive |
| `BuiltinId` registry metadata | type checker、checker dispatch、LSP completion/hover/documentation | 独立 builtin 名称/arity/文档表、字符串 dispatch；特殊类型规则只保留按 ID 的 typed dispatch |
| typed quantifier binding layout | lowering、scope/type checker、evaluator、diagnostic context、dimension/dependency visitor | array/dict 双 binding 的分叉 evaluator、以 dict entry object 模拟双 binding 的新路径；旧单 binding 仅保留为同一 layout 的兼容形态 |
| canonical `CftSchemaCheckBlock` dimension schedule | typed check query、top-level/type-local checks | `dimension_checks`、中间 dependency planner 及其他 statement-to-dimension 近似遍历 |
| `CheckExecutionId` + typed path dependency + `CheckChangeSet` | runner、dependency collector、snapshot、runtime load/session/mutation、tests | record-only root、record-ID-only `reads_from`、coordinate-only changed set、内部双写/全量失效 fallback |
| runtime module source catalog | diagnostic mapping、LSP/editor source lookup、snapshot generation validation | 根据 module/check 名称猜文件路径、host 各自 span-to-line 转换 |
| named top-level check + `records(Type)` | schema API/query、data-model record index、runner、dependency/snapshot、LSP/CLI/editor | 独立 top-level evaluator、无 generation 约束的临时 record scan、只跟踪当前成员 ID 的不完整依赖 |

每一行完成时，在对应提交说明中记录：新增 canonical 类型/入口、已迁移 consumer、删除的 symbol/file、执行的定向测试。矩阵中的旧实现只要仍有命中，该阶段就不能标记完成；名称因合理原因保留时必须给出具体文件、用途和为何不是旧路径，供最终审计复核。

## 14. 最终完成标准

功能、重构和文档全部满足以下条件后，本计划才算完成：

1. 计划中的所有语法都有 lexer/parser、schema lowering、静态类型、运行期、预算、诊断、LSP 和公开文档覆盖；所有明确拒绝的语法都有稳定负向测试。
2. 旧语法及其 wire JSON 回归测试通过；新增 serde 字段的缺省反序列化、未知 diagnostic context kind 保留和无 context 时的序列化形态均有 golden/round-trip 测试。
3. check AST 每个 variant 都被统一 visitor 覆盖；不存在为 f-string、nullable、dual binding 或 top-level check 单独复制的递归遍历器。
4. builtin registry、运行期 dispatch 和 LSP 文档做双向完整性断言，不存在已注册未实现、已实现未注册或只有 completion 没有 checker 的函数。
5. diagnostic message、contexts、data/schema labels 在 checker 到 API 再到 CLI/JSON/LSP/editor 的链路中无字符串反解析和双重拼接；自定义消息精确保持用户文本。
6. type-local 与 top-level check 共用 `CheckExecutionId`、snapshot、dependency graph 和 evaluator；仓库内不再出现旧 record-only root state、coordinate-only changed set 或旧 `dimension_checks` walker。
7. 增量测试证明 path overlap、短路动态读取、record membership、派生类型、删除/改 key、dimension overlay 和 `All` 回退既不漏跑，也不因实现方便永久退化为全量重跑。
8. 所有新增集合构造、格式化输出、记录枚举、依赖收集和 schema walker 均受结构/工作预算约束，并有耗尽测试证明返回稳定诊断而非 panic。
9. website reference、schema API、architecture、diagnostic code index 和 LSP builtin documentation 与最终行为一致；动态正则和计数量词只作为明确的未来能力出现，没有半实现代码。
10. 使用 `rg` 审计迁移矩阵中的旧类型/字段、`dimension_checks`、record-only `reads_from`、coordinate-only change API、重复 builtin 名称/arity/文档表、重复 AST 递归、重复 scalar renderer、message context 拼接、临时 feature flag、`TODO`/`FIXME` 和兼容 shim。审计结果作为最终 PR checklist 保存：每个命中必须被删除，或记录具体文件、所有者及其不是本计划技术债的理由；不得以“后续清理”关闭。
11. 从仓库根目录运行 `cargo check --workspace` 与 `cargo test --workspace` 均通过；不启动、停止或重启 CFD editor。测试若因正在运行的 editor 锁定二进制而无法替代，按 `AGENTS.md` 报告阻塞，不干扰用户进程。
12. 对集合关系、records 枚举、path overlap 和 snapshot invalidation 做实现复杂度复核；确认预算收费覆盖实际扫描/索引构建，关键路径没有无界缓存、未计费复制或可避免的二次扫描，并以预算耗尽测试及大输入回归测试作为证据。
13. 逐阶段架构前置条件均有提交级证据：顶层 check 没有虚构 record/current type，`records(Type)` 没有字符串反射或普通 object 深拷贝，mutation 与 snapshot 共用 typed path overlap，内部兼容 adapter 已在所属阶段删除。最终 PR 不以未来 issue、TODO 或 follow-up 代替这些交付项。
