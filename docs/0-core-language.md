# coflow核心语言

coflow是一种面向嵌入式场景的动态脚本语言，目标是替代lua，并把配置作为语言的一等能力。

核心版本只定义第一阶段必须实现且语义闭合的语言特性。未进入核心版本的语法放入`docs/proposals/`单独讨论。

## 设计目标

1. 轻量，可嵌入。
2. 适合作为宿主程序的脚本语言。
3. 配置和脚本使用同一种语法。
4. 顶层配置可由宿主统一加载，校验和消费。
5. 动态语言为底，类型标注服务于配置校验和宿主互操作。
6. 控制流和数据模型清晰，避免lua table式语义过载。

## 文件与模块

一个文件是一个模块。

```coflow
import common
import weapons as w
```

顶层声明默认公开。使用`local`声明文件内私有成员。

```coflow
local var scale = 2

local fn helper(x) {
  return x * scale
}
```

核心版本支持：

1. `import module`
2. `import module as alias`
3. 顶层默认公开
4. `local`私有声明

核心版本不支持`from import`，通配符导入和循环导入。

## 注释

支持单行注释。

```coflow
// this is a comment
var hp = 100
```

支持块注释。

```coflow
/*
this is a block comment
*/
var hp = 100
```

核心版本不支持嵌套块注释。

## 顶层语法

顶层只允许声明，不允许普通运行时语句。

允许的顶层声明：

1. `import`
2. `class`
3. `enum`
4. `fn`
5. `co fn`
6. `var`
7. 配置定义

顶层`name = value`是配置定义。

```coflow
base_damage = 10

sword = {
  id: "sword",
  damage: base_damage,
}
```

`local name = value`是文件内私有配置定义。

```coflow
local base_damage = 10
```

顶层`name: Type = value`是带类型标注的配置定义。

```coflow
sword: Weapon = {
  id: "sword",
  damage: 10,
}
```

顶层`var`是普通模块变量，不是配置。

```coflow
var runtime_cache = null
```

重复定义同名顶层成员是错误。

## 语句

核心版本支持以下语句：

1. `var`局部变量声明
2. 普通赋值
3. 字段赋值
4. 索引赋值
5. 复合赋值
6. `if else`
7. `while`
8. `for in`
9. `break`
10. `continue`
11. `return`
12. `throw`
13. `try catch`
14. `yield`
15. `yield from`
16. `yield break`

局部变量使用`var`，为块级作用域。

```coflow
fn main() {
  var hp = 100

  if hp > 0 {
    var alive = true
  }
}
```

`return`不能出现在`co fn`中。`yield`，`yield from`和`yield break`只能出现在`co fn`中。

## 函数

函数使用`fn`声明。

```coflow
fn add(a: int, b: int) {
  return a + b
}
```

参数类型标注可选。

```coflow
fn print_value(value) {
  print(value)
}
```

核心版本不引入`void`。所有函数调用都有结果。

1. `return value`返回`value`。
2. 函数自然结束等价于`return null`。
3. 空`return`不允许；需要提前返回空值时写`return null`。

匿名函数是值。

```coflow
var add_one = fn(x) {
  return x + 1
}
```

函数也是值，因此函数值可以作为配置常量的一部分。函数对象本身是常量；函数体运行时执行，不属于配置常量求值。

核心版本不支持命名参数，默认参数和多返回值。

函数可以访问模块顶层名字，也可以捕获外层作用域的局部变量。捕获是共享引用，闭包内外对同一变量的修改互相可见。

```coflow
fn make_counter() {
  var count = 0

  return fn() {
    count += 1
    return count
  }
}
```

函数可以在局部作用域中声明。局部函数遵守同样的捕获规则。

## 控制流

`if`是语句，不是表达式。

```coflow
if hp <= 0 {
  die()
} else {
  update()
}
```

支持`else if`。

```coflow
if score >= 90 {
  rank = "S"
} else if score >= 60 {
  rank = "A"
} else {
  rank = "B"
}
```

`while`用于条件循环。

```coflow
while running {
  update()
}
```

`for in`用于迭代可迭代值。

```coflow
for item in items {
  print(item)
}
```

标准库`range`函数返回可迭代值。

```coflow
for i in range(0, 10) {
  print(i)
}

for i in range(1, 11) {
  print(i)
}
```

## 表达式

核心版本支持：

1. 字面量
2. 数组字面量
3. 对象字面量
4. 字典字面量
5. 字段访问
6. 索引访问
7. 调用表达式
8. 算术运算
9. 字符串拼接
10. 比较运算
11. 逻辑运算
12. 空值合并`??`
13. 空值合并赋值`??=`
14. 可选字段访问`?.`
15. 成员判断`in`

逻辑运算使用`and`，`or`，`not`。

```coflow
if alive and not dead {
  update()
}
```

成员判断使用`in`。

```coflow
if item in items {
  print(item)
}

if key in scores {
  print(scores[key])
}

if "hero" in text {
  print(text)
}
```

`in`支持：

1. 数组成员判断。
2. 字典key判断。
3. 字符串子串判断。

核心版本不支持对象字段存在性判断。对象字段缺失读取结果是`null`。

缺失字段和缺失索引的读取结果是`null`。

```coflow
var name = player.name ?? "unknown"
```

可选字段访问使用`?.`。如果左侧为`null`，结果为`null`。

```coflow
var name = player?.profile?.name ?? "unknown"
```

空值合并赋值使用`??=`。左侧为`null`时才写入右侧值。

```coflow
name ??= "unknown"
```

核心版本不支持范围语法，`match`，`if`表达式，切片，解构，展开，`is`，`not in`和字符串插值。

## 数字

整数默认使用十进制。

```coflow
var hp = 100
var cost = 1_000
```

核心版本支持前缀进制整数：

```coflow
var mask = 0xff
var flags = 0b1010_0101
var mode = 0o755
```

浮点数支持小数形式和科学计数法。

```coflow
var speed = 3.5
var scale = 1_000.000_5
var large = 1e3
var small = 1.0e-3
var also_large = 1E+3
```

数字分隔符`_`只能出现在两个数字之间。负号不是数字字面量的一部分，`-1`按一元负号和数字字面量解析。

## 字符串

普通字符串使用双引号。

```coflow
var name = "hero"
```

字符串拼接使用`+`。

```coflow
var text = "hello " + name
```

原始字符串使用`r"`开始，不处理转义字符。

```coflow
var path = r"C:\game\assets\hero.png"
```

多行字符串使用三引号，保留换行。

```coflow
var text = """
line one
line two
"""
```

原始多行字符串使用`r"""`开始。

```coflow
var shader = r"""
float4 main() {
}
"""
```

## 错误处理

运行时错误使用`throw`抛出。

```coflow
fn check_hp(hp) {
  if hp < 0 {
    throw "hp must be >= 0"
  }
}
```

`throw string`由运行时包装为错误对象。错误对象至少包含：

1. `message`
2. `stack`

使用`try catch`捕获运行时错误。

```coflow
try {
  risky()
} catch err {
  print(err.message)
}
```

配置加载和配置校验错误是加载期诊断，不通过脚本`try catch`捕获。
