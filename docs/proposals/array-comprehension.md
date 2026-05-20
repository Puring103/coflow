# 提案：数组推导式

## 动机

配置生成和数据变换是 coflow 的核心场景。当前只能用 lambda + `map`/`filter` 调用链，或显式循环：

```coflow
# 现在
var doubled = items.map((x) => x * 2)
var alive   = enemies.filter((e) => e.alive)

# 嵌套时快速变丑
var names = enemies.filter((e) => e.alive).map((e) => e.name)
```

数组推导式直接在字面量语法中表达变换，更接近配置描述而非命令式代码：

```coflow
var doubled = [x * 2 for x in items]
var alive   = [e for e in enemies if e.alive]
var names   = [e.name for e in enemies if e.alive]
```

## 语法

```
array_comp ::= "[" expr "for" ident "in" expr ("if" expr)? "]"
```

基本形式：
```coflow
[expr for name in iterable]
[expr for name in iterable if condition]
```

多层嵌套（内层优先，从左到右）：
```coflow
[x + y for x in xs for y in ys]
# 等价于：for x in xs { for y in ys { yield x + y } }
```

与现有语法的一致性：
- `for` / `in` / `if` 复用现有关键字，无新关键字。
- `expr` 是任意表达式，支持方法调用、字段访问等。

## 语义

- 惰性求值还是立即求值：立即求值，结果是新数组。
- `if` 条件过滤：条件为假的元素不进入结果数组。
- `break` / `continue` / `return` / `yield`：不允许出现在推导式的 `expr` 或 `condition` 中。
- 变量遮蔽：推导式引入的 `name` 在推导式范围内遮蔽外层同名变量。

## 语法歧义

`[expr for ...]` 中，`for` 出现在数组字面量的第一个元素之后。目前数组字面量是：

```
array_literal ::= "[" (expr ("," expr)*)? "]"
```

加入推导式后，解析到 `[expr` 时需要向前看一个 token：
- 遇到 `for` → 推导式路径
- 遇到 `,` 或 `]` → 普通数组字面量路径

一位前瞻，无歧义。

## 实现成本

中等。

- AST：新增 `Expr::ArrayComp { element: Box<Expr>, clauses: Vec<CompClause> }`，其中 `CompClause` 是 `For { name, iter }` 或 `If { cond }` 的枚举。
- Lexer：无变化。
- Parser：数组字面量解析开始处加前瞻分支。

## 与其他提案的关系

- 与**切片**无冲突。
- 与**解构**联动：若引入解构，可以支持 `[f(k, v) for k, v in dict]`。
- 与**字典推导**：字典推导式 `{k: v for k, v in items}` 可作为后续扩展，语义和实现方式相同。

## 开放问题

1. 是否支持字典推导式 `{k: v for ...}`，同步引入还是单独提案？
2. 多层 `for` 的执行顺序：内层优先（笛卡尔积）是否是期望语义？
3. 推导式内的变量是否可以引用外层的 `var`？
