# P02 提案：is / is not 类型检查

## 动机

`any` 类型在游戏脚本中大量出现：宿主对象、事件参数、配置元数据。没有运行时类型检查，代码只能依赖约定，运行时崩溃难以定位。

```coflow
fn process(value: any) {
  if value is int { apply_int(value); }
  if value is string { apply_string(value); }
  if value is null { return; }
}
```

本提案与 [P01 联合类型与可空类型](./types.md) 配套：`is` 是联合类型收窄的基础能力。

## 语法

`is` 和 `is not` 作为二元运算符，右侧是类型名。

```
is_expr  ::= expr "is" type_name
not_expr ::= expr "is" "not" type_name
           | expr "is not" type_name
```

```coflow
value is int
value is string
value is bool
value is null
value is MyClass
value is not null
value is not int
```

第一期右侧只允许简单类型名（`int` `float` `string` `bool` `null` `any`、class 名、enum 名、函数类型 `fn`）。不允许复合类型（`is [int]` 不合法）。

优先级与比较运算符相同。

## 语义

- 检查运行时类型，结果为 `bool`。
- `is int` 和 `is float` 分别检查，不存在 `is number`。
- `is any` 永远返回 `true`。
- `is null` 等价于 `== null`，但更明确。
- class 类型检查：检查对象是否由该 class 实例化，不检查结构兼容性。
- enum 类型检查：检查值是否属于指定 enum 类型。
- `is not T` 等价于 `not (expr is T)`，但作为一个独立二元运算处理，便于诊断和类型收窄。

## 类型收窄

当左侧是简单变量名时，`if` 条件中的 `is` 可以在分支内收窄类型：

```coflow
fn label(value: string?) -> string {
  if value is string {
    return value;        # value 在此块内视为 string
  }
  return "unknown";
}
```

`is not null` 也可用于可空类型收窄：

```coflow
if player is not null {
  print(player.name);    # player 在此块内视为 Player
}
```

## 与 match 的联动

`is` 的完整价值在 match 表达式引入后体现：

```coflow
var result = match value {
  is int => process_int(value),
  is string => process_string(value),
  _ => default(),
};
```

建议 `is` 和 match 表达式一同设计，确保语义一致。

## 语法冲突

`is` 目前是普通标识符，加入后会变为保留关键字，存在兼容性成本。

`is not` 是两个 token，parser 在 infix 位置检测 `is` 后跟 `not` 时合并处理，与 `not in` 的实现方式相同。

## 实现成本

中等，主要成本在语义层：

- Lexer：加 `Is` 关键字 token。
- AST：`BinaryOp` 加 `Is` / `IsNot`，或新增 `Expr::Is { expr, ty, negated }`（后者更清晰）。
- Parser：低成本，与 `not in` 处理方式相同。
- 语义层：需要运行时类型标签系统，宿主对象类型注册机制。

## 开放问题

1. `is` 应该是 `BinaryOp` 变体还是独立的 `Expr::Is`？前者 uniform，后者右侧可以有更强的静态约束。
2. 数组类型检查 `value is [int]` 是否支持？
3. `is fn(int) -> string` 是否需要支持完整函数签名检查？
4. class 继承（若引入）时，子类是否通过父类的 `is` 检查？
