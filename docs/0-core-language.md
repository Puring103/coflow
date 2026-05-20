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
local fn helper(x) {
  return x * 2
}
```

核心版本支持：

1. `import module`
2. `import module as alias`
3. 顶层默认公开
4. `local`私有声明

核心版本不支持`from import`，通配符导入和循环导入。

## 注释

单行注释使用`#`。

```coflow
# this is a comment
var hp = 100
```

块注释使用`/* */`。

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
5. `iter fn`
6. `var`
7. 配置定义

顶层`name = value`是配置定义，隐式只读。

```coflow
base_damage = 10

sword = {
  id: "sword",
  damage: base_damage,
}
```

顶层`name: Type = value`是带类型标注的配置定义。

```coflow
sword: Weapon = {
  id: "sword",
  damage: 10,
}
```

顶层`var`是运行时模块变量，不是配置。

```coflow
var runtime_cache = null
```

核心版本不支持私有配置定义。`local name = value`不是合法顶层声明。

重复定义同名顶层成员是错误。

## 语句

核心版本支持以下语句：

1. `var`局部变量声明（块级作用域）
2. 普通赋值
4. 字段赋值
5. 索引赋值
6. 复合赋值
7. `if else`
8. `while`
9. `until`
10. `loop`
11. `for in`
12. `break`
13. `continue`
14. `return`
15. `throw`
16. `try catch`
17. `yield`
18. `yield from`

局部变量使用`var`，为块级作用域。

```coflow
fn main() {
  var hp = 100

  if hp > 0 {
    var alive = true
  }
}
```

`return`可以带值或不带值。

```coflow
fn get_name(player) {
  if player == null {
    return          # 等价于 return null
  }
  return player.name
}
```

`yield`，`yield from`只能出现在`iter fn`中。`iter fn`中可以使用`return`（不带值）提前结束迭代。

## 函数

函数使用`fn`声明。

```coflow
fn add(a: int, b: int) {
  return a + b
}
```

参数类型标注可选。返回类型使用`->`标注，可选。

```coflow
fn add(a: int, b: int) -> int {
  return a + b
}
```

参数可以有默认值。

```coflow
fn spawn(name: string, hp: int = 100, team: int = 0) {
  # ...
}

spawn("goblin")           # hp=100, team=0
spawn("boss", hp: 500)    # named argument
spawn("elite", 200, 1)    # positional
```

调用时可以使用具名参数，具名参数必须和形参名一致，可以和位置参数混用。

函数体可以是语句块或单个表达式。

```coflow
fn double(x: int) -> int => x * 2

fn greet(name: string) -> string {
  return "hello " + name
}
```

匿名函数是值。

```coflow
var double = fn(x) => x * 2

var greet = fn(name) {
  return "hello " + name
}
```

Lambda使用`(params) => expr`语法。

```coflow
var doubled = items.map((x) => x * 2)
var positive = items.filter((x) => x > 0)
```

Lambda参数可以有类型和默认值。Lambda体可以是表达式或块。

```coflow
var add = (x: int, y: int) -> int => x + y

var process = (x) => {
  var result = x * 2
  return result
}
```

核心版本不支持多返回值。

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

`while`用于条件循环，条件为真时持续执行。

```coflow
while running {
  update()
}
```

`until`用于条件循环，条件为真时停止执行（等价于`while not`）。

```coflow
until dead {
  tick()
}
```

`loop`用于无限循环，配合`break`退出。

```coflow
loop {
  var input = read()
  if input == "quit" {
    break
  }
  process(input)
}
```

`for in`用于迭代可迭代值。

```coflow
for item in items {
  print(item)
}
```

Range字面量可以直接用于`for in`。

```coflow
for i in 0..10 {    # [0, 10) 不含尾
  print(i)
}

for i in 0..=10 {   # [0, 10] 含尾
  print(i)
}
```

## 表达式

核心版本支持：

1. 字面量
2. 数组字面量
3. 对象字面量（支持展开`...`）
4. 字典字面量
5. 字段访问
6. 索引访问
7. 调用表达式
8. 算术运算（`+` `-` `*` `/` `%`）
9. 整数除法（`//`）
10. 幂运算（`**`）
11. 位运算（`&` `|` `^` `~` `<<` `>>`）
12. 字符串拼接（`+`）
13. 比较运算（含链式比较）
14. 逻辑运算
15. 空值合并`??`
16. 空值合并赋值`??=`
17. 可选字段访问`?.`
18. 成员判断`in`
19. Range字面量（`..` `..=`）

逻辑运算使用`and`，`or`，`not`。

```coflow
if alive and not dead {
  update()
}
```

比较运算支持链式写法，等价于各段比较用`and`连接。

```coflow
if 0 < damage <= 100 {
  apply(damage)
}
# 等价于 damage > 0 and damage <= 100
```

位运算优先级低于比较运算，高于逻辑运算。

```coflow
var flags = STATUS_DEAD | STATUS_STUNNED
if flags & STATUS_DEAD != 0 {
  die()
}
var cleared = flags & ~STATUS_STUNNED
```

整数除法`//`和幂运算`**`。

```coflow
var q = damage // armor    # 截断整除
var area = radius ** 2     # 幂运算，右结合
```

对象展开使用`...`。

```coflow
base = { damage: 10, speed: 1.0 }

sword = {
  ...base,
  name: "Iron Sword",
  damage: 15,    # 覆盖 base.damage
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
```

可选字段访问使用`?.`。如果左侧为`null`，结果为`null`。

```coflow
var name = player?.profile?.name ?? "unknown"
```

空值合并赋值使用`??=`。左侧为`null`时才写入右侧值。

```coflow
name ??= "unknown"
```

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

运行时错误使用`throw`抛出，必须抛出error对象。

```coflow
fn check_hp(hp) {
  if hp < 0 {
    throw error("hp must be >= 0")
  }
}
```

使用`try catch`捕获运行时错误。

```coflow
try {
  risky()
} catch err {
  print(err.message)
  print(err.stack)
}
```

配置加载和配置校验错误是加载期诊断，不通过脚本`try catch`捕获。
