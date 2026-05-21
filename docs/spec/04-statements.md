# coflow 语句

语句是构成函数体和块的基本执行单元。顶层只允许声明，不允许普通语句（见 [05-declarations.md](./05-declarations.md)）。

## 分号

每条语句必须以 `;` 结尾。以 `}` 自然结束的构造（`if`、`while`、`loop`、`for in`、`try catch`、局部 `fn`/`iter fn` 声明）**不需要**分号。

当语句的值是一个以 `}` 结尾的表达式（如赋值右侧为匿名函数）时，分号加在 `}` 之后：

```coflow
var f = fn(x) {
  return x * 2;
};                # var 声明的分号在匿名函数 } 之后

return fn() {
  yield 1;
};                # return 语句的分号同理
```

## 块

块是由 `{` 和 `}` 包围的语句序列，引入新的词法作用域：

```coflow
{
  var x = 10;
  print(x);
}
```

块内禁止裸 `:` 形式的标签语法（如 `label: stmt`），出现是语法错误。

## var 声明

在当前块作用域中声明局部变量：

```coflow
var name = "hero";
var hp: int = 100;
var target: any = null;
var cache;             # 无初始值，默认为 null
```

语法：

```
var name
var name = expr
var name: Type
var name: Type = expr
```

规则：
- 变量作用域从声明处到所在块的末尾
- 同一块内不能重复声明同名变量
- 内层块可以遮蔽（shadow）外层的同名变量
- 有类型标注且类型不允许 `null`（即非 `any`）时，必须提供初始值；`var x: int`（无初值）是编译期错误

```coflow
fn main() {
  var hp = 100;

  if hp > 0 {
    var alive = true;   # alive 只在此 if 块内可见
  }
  # alive 在这里不可见
}
```

## 赋值语句

### 普通赋值

```coflow
x = 10;
player.hp = 100;
items[0] = "sword";
```

赋值目标可以是：
- 变量名：`x = value`
- 字段访问：`obj.field = value`
- 索引访问：`arr[i] = value` 或 `dict[key] = value`

不能赋值给函数调用结果、字面量等非左值表达式。

可选字段访问和可选索引访问不能作为赋值目标：

```coflow
obj?.field = value;   # 错误
dict?[key] = value;   # 错误
```

字典普通索引赋值可以创建或覆盖 key。数组索引赋值要求索引有效，越界时报运行时错误。对象字段赋值要求字段存在；class 对象只能写入 class 声明的字段。

### 复合赋值

将运算和赋值合并，`x op= v` 严格等价于 `x = x op v`（按 `x = x op v` 展开求值，**不**保证原地修改语义）：

| 运算符  | 等价于 |
|---------|--------|
| `x += v`   | `x = x + v` |
| `x -= v`   | `x = x - v` |
| `x *= v`   | `x = x * v` |
| `x /= v`   | `x = x / v` |

只保留以上四种算术复合赋值，加上下面的 `??=`。位运算、整除、取余、幂运算的复合赋值在配置/嵌入式场景使用频次极低，统一展开为完整赋值。

数组的 `+= [x]` 同样按上述规则展开为 `arr = arr + [x]`，**总是创建新数组并重新绑定变量**，旧数组的别名不会观察到变化。需要原地追加时使用 `arr.push(x)`（参见 [02-types.md](./02-types.md)）。

复合赋值同样支持字段和索引目标：

```coflow
player.hp -= damage;
counters["hit"] += 1;
```

### 空值合并赋值

```coflow
x ??= default_value;
obj.field ??= "default";
dict[key] ??= 0;
```

仅当左侧当前值为 `null` 时才执行赋值，左侧非 `null` 时整个语句无效。

`??=` 只支持普通可写位置，不支持可选访问目标：

```coflow
obj?.field ??= value;   # 错误
dict?[key] ??= value;   # 错误
```

对字典索引目标，key 不存在时视为缺失并写入右侧值。对数组索引目标，索引越界时报运行时错误，不自动扩容。对对象字段目标，字段不存在时报运行时错误。

## 表达式语句

任何表达式都可以作为独立语句，通常用于调用有副作用的函数：

```coflow
print("hello");
obj.update();
list.push(item);
```

## if 语句

```coflow
if condition {
  # then 块
}

if condition {
  # then 块
} else {
  # else 块
}

if condition1 {
  # ...
} else if condition2 {
  # ...
} else {
  # ...
}
```

`else if` 可以连续链接任意多个分支。`condition` 可以是任意表达式。

coflow 的真值规则：只有 `false` 和 `null` 是假值，其他所有值（包括 `0`、`0.0`、`""`、`[]`）均为真值。

`if` 也可以作为**表达式**使用（`if cond => expr else => expr` 语法），见 [03-expressions.md](./03-expressions.md)。

## while 语句

条件为真时持续执行循环体：

```coflow
while running {
  update();
}
```

`condition` 在每次循环开始前求值，为假时退出循环。

## loop 语句

无限循环，必须通过 `break` 退出：

```coflow
loop {
  var input = read();
  if input == "quit" {
    break;
  }
  process(input);
}
```

## for in 语句

迭代可迭代值：

```coflow
for item in items {
  print(item);
}
```

`in` 右侧可以是数组、字典、Range、`iter fn` 调用结果或任何 Iterator 对象。

```coflow
for i in 0..10 {
  print(i);           # 0, 1, ..., 9
}

for i in 0..=10 {
  print(i);           # 0, 1, ..., 10
}
```

迭代字典时，循环变量绑定到 entry 对象（含 `key` 和 `value` 字段）：

```coflow
for entry in scores {
  print(entry.key);
  print(entry.value);
}
```

`for in` 的运行时展开语义见 [../design/05-runtime.md](../design/05-runtime.md)。

## break 语句

退出最近的 `while`、`loop` 或 `for in` 循环：

```coflow
loop {
  if done { break; }
  process();
}
```

`break` 只能出现在循环体内，不能跨越函数边界（包括跨越闭包边界）。

## continue 语句

跳过本次迭代，进入下一次循环：

```coflow
for i in 0..10 {
  if i % 2 == 0 { continue; }
  print(i);   # 只打印奇数
}
```

`continue` 只能出现在循环体内，不能跨越函数边界。

## return 语句

从当前函数返回：

```coflow
fn get_name(player) {
  if player == null {
    return;           # 等价于 return null
  }
  return player.name;
}
```

- `return`（不带值）：返回 `null`
- `return expr`：返回 `expr` 的值

`iter fn` 中的 `return` 规则见下方 yield 部分。

## throw 语句

抛出运行时错误：

```coflow
fn check_hp(hp) {
  if hp < 0 {
    throw error("hp must be >= 0");
  }
}
```

`throw` 的操作数必须是 Error 对象（由内建 `error()` 函数创建）。抛出的错误沿调用栈向上传播，直到被 `try catch` 捕获。

### error() 与 Error 对象

内建函数 `error` 用于创建 Error 对象：

```
error(message: string, data: any = null) -> Error
```

- `message`：人类可读的错误消息，必填
- `data`：附加结构化数据（错误码、上下文信息等），默认 `null`

返回的 Error 对象具有以下字段：

| 字段 | 类型 | 说明 |
|------|------|------|
| `message` | `string` | 由 `error()` 第一参数提供 |
| `data` | `any` | 由 `error()` 第二参数提供 |
| `stack` | `string` | 调用栈快照；**由 `throw` 触发时填充**，未经 `throw` 的 Error 对象 `stack` 为 `null` |

将创建与抛出分开是合法的，`stack` 在 `throw` 时记录抛出位置：

```coflow
var e = error("invalid input", "E_INPUT");
# e.stack == null
throw e;
# 抛出后被 catch 捕获时，e.stack 是 throw 处的栈
```

## try catch 语句

捕获运行时错误：

```coflow
try {
  risky();
} catch err {
  print(err.message);
  print(err.stack);
}
```

- `try` 块内（及其调用链中）抛出的错误被捕获
- `catch` 后的标识符（`err`）绑定到捕获的 Error 对象，作用域为 `catch` 块内
- Error 对象的字段见上方 `error()` 节
- `catch` 块内可以重新 `throw`；重新抛出时保留原始 stack trace

`try catch` 只捕获运行时错误，不捕获加载期配置错误。

## yield 语句

只能出现在 `iter fn` 内，用于产出迭代值：

```coflow
iter fn counter() {
  yield 1;
  yield 2;
  yield 3;
}
```

`yield value` 暂停 `iter fn` 的执行，将 `value` 作为本次 `next()` 调用的结果（`{ done: false, value: value }`）返回给调用方。下一次 `next()` 调用时从 `yield` 处恢复执行。

### yield from

```coflow
yield from expr;
```

`expr` 必须是可迭代值。等价于逐一 `yield` 子迭代器的每个元素：

```coflow
iter fn parent() {
  yield from child();   # 产出 child() 的所有值
  yield 99;
}
```

子迭代器结束后，控制权返回 `parent`，继续执行 `yield from` 之后的语句。

`yield value` 总是产出 `value` 本身；若 `value` 是 Iterator 对象，作为普通值产出，**不**自动展开：

```coflow
iter fn wrap() {
  yield child();       # 产出 Iterator 对象本身（一个值）
  yield from child();  # 展开子 Iterator，逐一产出其元素
}
```

### iter fn 中的 return

`iter fn` 中可以用不带值的 `return` 提前结束迭代：

```coflow
iter fn numbers(limit) {
  var i = 0;
  while true {
    if i >= limit { return; }  # 结束迭代
    yield i;
    i += 1;
  }
}
```

`iter fn` 中**禁止**使用 `return value`（带值的 return）。

## 局部函数声明

函数可以在块内声明，形成局部函数：

```coflow
fn process(items) {
  fn helper(x) {
    return x * 2;
  }

  for item in items {
    print(helper(item));
  }
}
```

局部函数遵守与局部变量相同的作用域和闭包捕获规则，见 [06-modules.md](./06-modules.md)。
