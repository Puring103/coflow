# Check 诊断错误码设计

## 目标

运行期 `check {}` 失败需要同时满足两个目标：

- 对机器稳定可分类：每类规则失败都有明确、稳定的错误码。
- 对人可操作：错误信息能说明哪条规则失败、实际值是什么、期望是什么。

当前大量 false 条件都落到 `CFD-CHECK-001`，并输出 `check condition evaluated to false`，无法看出具体是哪种规则失败，也无法直接定位违反规则的数据原因。

本设计保留现有 `CFD-CHECK-001` 到 `CFD-CHECK-006` 的含义和编号，从 `CFD-CHECK-007` 开始新增更细的 false 条件错误码。`when` 不分配独立错误码，只作为上下文追加到内部真实失败规则的诊断信息中。

## 范围

包含：

- 为运行期 false 条件新增更细的 `CFD-CHECK-*` 错误码。
- 为每类 check 规则输出更详细的 message，包括表达式文本、实际值、期望条件、数据路径和上下文。
- 保持现有 JSON 诊断结构不变，继续输出 `code`、`stage`、`severity`、`message`、来源定位和 related labels。
- 增加测试，断言错误码和 message 细节。

不包含：

- 修改 CFT parser/type-checker 的静态错误码。
- 新增公开 JSON 诊断 schema。
- 给 `when` 分配单独错误码。
- 把主定位从数据源位置改成 schema 位置。schema 表达式文本可以进入 message，但 primary location 仍然指向违反规则的数据 cell/path。

## 错误码分配

现有错误码保持不变：

| 错误码 | 名称 | 含义 |
| --- | --- | --- |
| `CFD-CHECK-001` | `CheckFailed` | 兜底 false 条件。只有无法归类到更具体规则时使用。 |
| `CFD-CHECK-002` | `CheckEvalTypeError` | check 求值运行期类型错误。 |
| `CFD-CHECK-003` | `CheckNullAccess` | 对 `null` 做字段访问、索引访问、大小比较或算术。 |
| `CFD-CHECK-004` | `CheckIndexOutOfBounds` | 数组索引越界。 |
| `CFD-CHECK-005` | `CheckMissingDictKey` | 字典 key 不存在。 |
| `CFD-CHECK-006` | `CheckEmptyMinMax` | `min` 或 `max` 作用于空数组或全 null 数组。 |

新增 false 条件错误码：

| 错误码 | 名称 | 适用规则 |
| --- | --- | --- |
| `CFD-CHECK-007` | `CheckComparisonFailed` | `==`、`!=`、`<`、`<=`、`>`、`>=`，包括链式比较。 |
| `CFD-CHECK-008` | `CheckBoolExpectedTrue` | 裸 bool 表达式或 bool 字段求值为 `false`。 |
| `CFD-CHECK-009` | `CheckNegationFailed` | `!expr` 失败，因为 `expr` 求值为 `true`。 |
| `CFD-CHECK-010` | `CheckAndFailed` | `lhs && rhs` 失败，因为至少一侧为 false。 |
| `CFD-CHECK-011` | `CheckOrFailed` | `lhs || rhs` 失败，因为两侧都为 false。 |
| `CFD-CHECK-012` | `CheckTypePredicateFailed` | `expr is TypeName` 求值为 false。 |
| `CFD-CHECK-013` | `CheckNullPredicateFailed` | null 谓词失败，包括 `expr is null`、`expr == null`、`expr != null`。 |
| `CFD-CHECK-014` | `CheckContainsFailed` | `contains` 返回 false。 |
| `CFD-CHECK-015` | `CheckUniqueFailed` | `unique` 返回 false。 |
| `CFD-CHECK-016` | `CheckMatchesFailed` | `matches` 返回 false。 |
| `CFD-CHECK-017` | `CheckAnyQuantifierFailed` | `any x in collection { ... }` 没有任何元素满足。 |
| `CFD-CHECK-018` | `CheckNoneQuantifierFailed` | `none x in collection { ... }` 存在满足条件的元素。 |
| `CFD-CHECK-019` | `CheckAllQuantifierFailed` | `all x in collection { ... }` 存在不满足条件的元素。 |

`when` 只作为上下文。例如：

```cft
when is_passive {
  range != null;
}
```

如果 `range != null` 失败，错误码仍然是 `CFD-CHECK-013`，message 中追加：

```text
上下文: 在 when is_passive 内
```

## Message 约定

message 使用紧凑的多行格式：

```text
校验失败: <表达式或语句>
实际值: <实际值摘要>
期望: <期望规则>
上下文: <可选 when / quantifier / language 上下文>
```

示例：

```text
校验失败: level > 0
实际值: level = 0
期望: > 0
```

```text
校验失败: tags.contains("boss")
实际值: tags = <array len=2>
期望: 包含 "boss"
```

```text
校验失败: all reward in rewards { reward.amount > 0; }
校验失败: reward.amount > 0
实际值: rewards[2].amount = 0
期望: > 0
上下文: 绑定 reward 位于 rewards[2]
```

```text
校验失败: range != null
实际值: range = null
期望: != null
上下文: 在 when is_passive 内
```

当某项信息确实不可得时可以省略对应行，但每个新增具体错误码至少必须包含失败表达式/语句，并包含实际值或失败元素/path。

## 规则行为

### 比较

普通比较失败使用 `CheckComparisonFailed`。

null 等值/非等值谓词使用 `CheckNullPredicateFailed`，例如 `item == null`、`item != null`。

链式比较需要指出第一个失败的相邻比较：

```text
校验失败: 0 < damage <= 100
实际值: damage = 120
期望: <= 100
```

### 布尔表达式

裸 bool 字段或表达式为 false 时使用 `CheckBoolExpectedTrue`。

`!expr` 失败时使用 `CheckNegationFailed`，说明内部表达式实际为 true。

`&&` 和 `||` 使用自己的组合错误码：

- `lhs && rhs` 失败：`CheckAndFailed`，message 说明哪侧失败。
- `lhs || rhs` 失败：`CheckOrFailed`，message 说明两侧都为 false。

如果能拿到子表达式解释，组合错误 message 应包含子表达式的失败细节。

### 类型和 null 谓词

`expr is TypeName` 失败时使用 `CheckTypePredicateFailed`，能拿到实际类型时展示实际类型。

`expr is null`、`expr == null`、`expr != null` 失败时使用 `CheckNullPredicateFailed`，展示实际值摘要。

### 内建函数

`contains` 返回 false 时使用 `CheckContainsFailed`，需要区分：

- array：检查元素值是否存在。
- dict：检查 key 是否存在。

`unique` 返回 false 时使用 `CheckUniqueFailed`，message 包含重复值和能取得的重复 index/path。

`matches` 返回 false 时使用 `CheckMatchesFailed`，message 包含被匹配文本和正则 pattern。

`len`、`min`、`max`、`sum`、`keys`、`values` 是值生产表达式；如果它们参与比较，false 条件由外层比较错误码负责，并在 message 中展示计算结果。

### 量词

`all` 中某个元素的 body 规则求值为 false 时，使用 `CheckAllQuantifierFailed`。message 保留内部失败表达式、实际值、期望条件，并追加绑定变量和元素 path。body 中出现运行期硬错误时，保留硬错误码 `CFD-CHECK-002` 到 `CFD-CHECK-006`，并追加量词上下文。

`any` 没有任何元素满足时，使用 `CheckAnyQuantifierFailed`。如果过程中收集了元素失败原因，message 应包含失败数量或简短样例。

`none` 存在满足条件的元素时，对每个意外匹配的元素使用 `CheckNoneQuantifierFailed`。

### When 上下文

`when` 条件为 true 且 body 规则失败时，诊断使用内部真实规则的错误码，并在 message 追加：

```text
上下文: 在 when <condition> 内
```

`when` 条件为 false 时不产生诊断。

## 实现形态

在 `coflow-checker` 内部增加一个解释模型，例如：

```text
CheckExplanation {
  code: CfdErrorCode,
  expression: String,
  actual: Option<String>,
  expected: Option<String>,
  context: Vec<String>,
  path: Option<CfdPath>,
}
```

增加渲染 helper：

- 将 `CftSchemaCheckExpr` 渲染为紧凑表达式文本。
- 将 `CftSchemaCheckStmt` 渲染为紧凑语句文本。
- 将 `CheckValue` 渲染为短诊断值文本。
- 将 `CfdPath` 渲染为 message 中可读的字段/index 路径。

用递归解释器替换当前覆盖面较窄的 `eval_expr_explained`，覆盖所有 false bool 表达式形态。运行期硬错误继续通过 `diag_at` 直接发出，但在可行时补充表达式文本。

`CheckRunner` 和 engine 映射原则上不需要公开 API 变更。primary source location 仍由 `CfdDiagnostic.primary` 映射到数据来源。

## 测试

新增 checker 单元测试：

- `level > 0` -> `CFD-CHECK-007`。
- `enabled;` 且值为 false -> `CFD-CHECK-008`。
- `!enabled` 且 `enabled = true` -> `CFD-CHECK-009`。
- `a && b` 失败 -> `CFD-CHECK-010`。
- `a || b` 失败 -> `CFD-CHECK-011`。
- `reward is CurrencyReward` 失败 -> `CFD-CHECK-012`。
- `optional != null` 且值为 null -> `CFD-CHECK-013`。
- `tags.contains("boss")` 失败 -> `CFD-CHECK-014`。
- `tags.unique()` 失败 -> `CFD-CHECK-015`。
- `name.matches("^npc_")` 失败 -> `CFD-CHECK-016`。
- `any item in items { item.enabled; }` 无匹配元素 -> `CFD-CHECK-017`。
- `none item in items { item.enabled; }` 存在匹配元素 -> `CFD-CHECK-018`。
- `all item in items { item.enabled; }` 元素失败 -> `CFD-CHECK-019`，message 包含内部表达式细节和量词上下文。
- `when cond { optional != null; }` -> 内部错误码 `CFD-CHECK-013`，message 包含 `在 when cond 内`。

更新 CLI 测试，确保 human 和 JSON 输出中代表性 project check 失败使用新错误码，并包含详细 message。

## 兼容性

这是诊断精度变更。只判断 `stage == CHECK` 的消费者不受影响。直接断言所有 false 条件都是 `CFD-CHECK-001` 的消费者需要更新为更具体的错误码。

`CheckFailed` / `CFD-CHECK-001` 保留，作为无法分类 false 条件的兜底。
