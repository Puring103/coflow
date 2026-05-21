# coflow 运行时语义

## 模块加载阶段

模块加载按顺序经历三个阶段：

### 阶段一：声明收集

扫描顶层，收集所有声明的名称（`fn`、`iter fn`、`class`、`enum`、`var`、配置定义）。此阶段不执行任何代码。阶段完成后，所有顶层名称在模块内任意位置可见，支持前向引用。

### 阶段二：配置常量求值

按依赖拓扑序求值所有顶层配置定义：

1. 构建配置依赖图
2. 检测循环依赖，若存在则报告加载期错误，终止加载
3. 按拓扑序逐一求值每个配置（被依赖的配置先求值）
4. 带 class 类型标注的配置在求值后进行结构校验
5. 结构校验通过后执行 class 的 `check` 块
6. 所有配置求值和校验完成后，标记为深只读

此阶段不初始化 `var`，配置不能依赖 `var`。

### 阶段三：运行时变量初始化

按源码顺序初始化所有顶层 `var`。此时配置已经可读，`var` 初始化表达式可以访问配置值和调用宿主 API：

```coflow
var player_count = host.get_player_count();
var cache = host.create_cache();
```

阶段三完成后，模块进入可调用状态。运行时代码从宿主调用公开函数开始，顶层禁止普通运行时语句：

```coflow
fn start() {
  print("game started");
}
```

## 真值规则

条件判断时：

- **假值**：`false`、`null`
- **真值**：其他所有值，包括 `0`、`0.0`、`""`（空字符串）、`[]`（空数组）

此规则适用于 `if`、`while`、`until`、`and`/`or`/`not` 以及 `??` 的所有条件判断位置。

## 函数调用语义

调用创建新的执行帧，参数按值绑定到形参：

```coflow
fn add(a, b) { return a + b; }
add(1, 2);   # 新帧：a=1, b=2
```

**参数传递**：基础类型（`int`、`float`、`bool`、`string`、`null`）按值传递，对其赋值不影响调用方。对象、数组、字典传递引用，通过引用修改内容对调用方可见。

**具名参数匹配**：具名参数按形参名匹配，与位置参数混用时，已按位置匹配的参数不再参与具名匹配。

**返回值**：`return expr` 返回 `expr` 的值；`return`（不带值）或函数体自然结束均返回 `null`。

## Iterator 协议

Iterator 是提供 `next()` 方法的对象：

```coflow
{ next: fn() -> { done: bool, value: any } }
```

`next()` 的返回值：

```coflow
{ done: false, value: <当前产出值> }   # 继续迭代
{ done: true,  value: null }           # 迭代结束
```

规则：
- 一旦 `done` 为 `true`，后续所有 `next()` 调用仍返回 `{ done: true, value: null }`
- Iterator 执行期间抛出错误时，错误从 `next()` 调用处传播
- 异常终止的 Iterator 进入 dead 状态，再次 `next()` 抛运行时错误

## iter() 内建函数

`for in` 通过内建 `iter(value)` 获取 Iterator，规则：

1. 若 `value` 已经是 Iterator（有 `next` 方法），直接返回
2. 若 `value` 是数组，返回数组 Iterator（按索引顺序产出元素）
3. 若 `value` 是字典，返回字典 entry Iterator（每次产出 `{ key, value }` 对象）
4. 若 `value` 是 Range，返回 Range Iterator（产出整数序列）
5. 否则抛运行时错误（静态可确定时提前诊断）

## for in 展开语义

```coflow
for item in items {
  body
}
```

语义等价于：

```coflow
var _it = iter(items);
while true {
  var _step = _it.next();
  if _step.done { break; }
  var item = _step.value;
  body
}
```

- `break`：退出等价的 `while true` 循环
- `continue`：跳到下一次 `_it.next()` 调用

## Range

Range 字面量产生惰性整数序列：

```coflow
0..10    # [0, 10)，产出 0, 1, ..., 9
0..=10   # [0, 10]，产出 0, 1, ..., 10
```

Range 的起始和终止是整数表达式，惰性求值（不预先生成数组），可用于：

- `for in` 迭代
- `in` 成员判断（`x in a..b` 等价于 `x >= a and x < b`；`x in a..=b` 等价于 `x >= a and x <= b`）

## 标准库 range 函数

```coflow
range(start, end)
range(start, end, step)
```

`range` 是标准库函数，作为 Range 字面量的补充，支持步长：

- `range(0, 10)` → 产出 0, 1, ..., 9（左闭右开，等价于 `0..10`）
- `range(0, 10, 2)` → 产出 0, 2, 4, 6, 8
- `range(10, 0, -1)` → 产出 10, 9, ..., 1

规则：`step` 默认为 1，不能为 0。`range` 返回 Iterator，不生成数组。

## iter fn 执行模型

`iter fn` 调用创建挂起的执行帧，返回 Iterator。函数体此时不执行。

每次调用 `next()`：

1. 恢复执行帧，从上次 `yield` 暂停处继续执行
2. 遇到 `yield value`：暂停，返回 `{ done: false, value: value }`
3. 遇到 `return`（不带值）或函数体执行到末尾：返回 `{ done: true, value: null }`，执行帧销毁

```coflow
iter fn count_up(start) {
  var i = start;
  while true {
    yield i;
    i += 1;
  }
}

var c = count_up(1);
c.next();   # { done: false, value: 1 }
c.next();   # { done: false, value: 2 }
```

每次 `iter fn` 调用都创建独立的执行帧：

```coflow
var c1 = count_up(1);
var c2 = count_up(100);

c1.next();   # { done: false, value: 1 }
c2.next();   # { done: false, value: 100 }
c1.next();   # { done: false, value: 2 }
```

### iter fn 闭包

`iter fn` 可以捕获外层局部变量，捕获语义与普通函数相同（共享引用）：

```coflow
fn make_sequence(start) {
  var i = start;
  return iter fn() {
    while true {
      yield i;
      i += 1;
    }
  };
}
```

## yield from 语义

```coflow
yield from expr;
```

执行过程：

1. 通过 `iter(expr)` 获取子 Iterator
2. 循环调用子 Iterator 的 `next()`
3. 子 Iterator 每次产出的值透明地向外产出（等价于对每个值执行 `yield`）
4. 子 Iterator 结束（`done: true`）后，`yield from` 结束，继续执行后续代码

```coflow
iter fn child() { yield 1; yield 2; }

iter fn parent() {
  yield from child();   # 产出 1, 2
  yield 3;
}
# parent() 依次产出：1, 2, 3
```

子 Iterator 执行期间抛出的错误从 `yield from` 处向上传播。

`yield value` 总是产出 `value` 本身，若 `value` 是 Iterator 对象，作为普通值产出，不自动展开：

```coflow
iter fn wrap() {
  yield child();       # 产出 Iterator 对象本身（一个值）
  yield from child();  # 展开，逐一产出 1, 2
}
```

## 错误处理语义

### throw

```coflow
throw error("message");
```

创建 error 对象并向上传播。error 对象包含：

- `message: string`：错误描述
- `stack: string`：调用栈信息（由运行时填充）

### try catch

```coflow
try {
  risky_operation();
} catch err {
  handle(err.message);
}
```

执行语义：

1. 执行 `try` 块
2. 若无错误，跳过 `catch` 块，继续后续代码
3. 若 `try` 块内（或调用链中）抛出错误，停止 `try` 块，将 error 对象绑定到 `catch` 标识符，执行 `catch` 块
4. `catch` 块执行完毕后，继续 `try catch` 之后的代码

`catch` 块内可以重新 `throw`：

```coflow
try {
  risky();
} catch err {
  if is_fatal(err) { throw err; }
  recover();
}
```

`try catch` 只捕获运行时错误，不捕获加载期配置错误。

### Iterator 中的错误

若 `iter fn` 执行期间（在 `next()` 调用中）抛出错误：
- 错误从该 `next()` 调用处传播
- Iterator 进入 dead 状态
- 后续 `next()` 调用抛运行时错误
