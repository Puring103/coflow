# coflow核心类型

coflow是动态语言。类型标注可选，主要用于配置校验，宿主互操作，编辑器补全和诊断。

## 基础类型

核心版本内置以下类型：

1. `int`
2. `float`
3. `bool`
4. `string`
5. `null`
6. `any`

```coflow
var hp: int = 100
var speed: float = 3.5
var name: string = "hero"
var alive: bool = true
var target: any = get_target()
```

## null

`null`表示空值。

缺失字段和缺失索引读取结果也是`null`。

```coflow
var name = player.name ?? "unknown"
```

没有`void`类型。没有返回值的函数返回`null`。

## any

`any`表示任意动态值。

```coflow
var value: any = host.get("player")
value.move(1, 0)
```

`any`上的字段访问和调用主要在运行时检查。

## 数组

数组表示有序列表。

```coflow
var damages: [int] = [10, 20, 30]
var names: [string] = ["a", "b", "c"]
var mixed: [any] = [1, "hello", true]
```

数组可以被`for in`迭代。

数组支持`in`成员判断。

## 对象

对象表示静态字段集合。字段名固定，字段值可以是不同类型。

```coflow
weapon = {
  id: "sword",
  damage: 10,
}
```

对象支持点访问。

```coflow
var damage = weapon.damage
```

对象主要用于结构化数据，配置，class实例和宿主对象绑定。

## 字典

字典表示动态键值映射。字典值是同构的。当值类型为`any`时，字典接受任意值。

```coflow
var scores: [string: int] = {
  "alice": 10,
  "bob": 20,
}
```

字典主要用索引访问。

```coflow
var score = scores["alice"]
```

字典不支持点访问。

字典可以被`for in`迭代，默认迭代entry对象。

字典支持`in` key判断。

## 对象与字典的区别

对象和字典可以共享底层实现，但语言语义不同。

1. 对象是静态字段集合，字段可以异构。
2. 字典是动态键值集合，值类型同构。
3. 对象使用点访问。
4. 字典使用索引访问。
5. class配置校验面向对象，不面向字典。

有class类型上下文时，`{ ... }`按对象校验。

```coflow
class Weapon {
  id: string
  damage: int
}

sword: Weapon = {
  id: "sword",
  damage: 10,
}
```

有字典类型上下文时，`{ ... }`按字典校验。

```coflow
scores: [string: int] = {
  "alice": 10,
  "bob": 20,
}
```

没有类型上下文时，按key形式判断：

1. 标识符key → 对象。
2. 字符串key → 字典。

```coflow
weapon = {
  id: "sword",
  damage: 10,
}
```

纯字符串key的字面量在无类型上下文时推断为字典。值类型同构时推断具体类型，异构时推断为`[string: any]`。

```coflow
var scores = {
  "alice": 10,
  "bob": 20,
}
// 推断为 [string: int]

var meta = {
  "name": "hero",
  "level": 5,
}
// 推断为 [string: any]
```

此规则适用于所有无类型上下文的位置：`var`右值，函数参数，数组元素等。

顶层配置定义仍需显式字典类型标注以保证配置校验的严格性。

```coflow
scores: [string: int] = {
  "alice": 10,
  "bob": 20,
}
```

核心版本不支持动态key字面量语法。动态key对象构造放入提案。

## class

`class`声明对象结构。

```coflow
class Weapon {
  id: string
  name: string
  damage: int
  cooldown: float = 1.0
}
```

字段可以有默认值。

```coflow
class Enemy {
  id: string
  hp: int = 100
}
```

字段默认值必须是常量表达式。核心版本中，字段默认值不能引用`self`或同一对象的其他字段。

核心版本的`class`只声明结构，不声明方法。`validate`是专用关键字，不是方法。

## validate

`validate`是class内的配置校验块，在配置加载期执行。

```coflow
class Range {
  min: int
  max: int

  validate {
    if self.min > self.max {
      throw "min must be <= max"
    }
  }
}
```

`validate`块中`self`隐式可用，指向当前校验的对象实例。

校验失败使用`throw`。校验错误是加载期配置诊断，不通过脚本`try catch`捕获。

`validate`块的约束：

1. 禁止修改`self`或任何外部状态。
2. 禁止调用宿主API。
3. 只允许读取`self`字段和使用纯计算逻辑。

一个class只允许一个`validate`块。

## enum

`enum`用于有限选项。

```coflow
enum Rarity {
  common
  rare
  epic
}
```

使用枚举值：

```coflow
rarity = Rarity.common
```

## 函数值

函数是一种值。

```coflow
var f = fn(x) {
  return x + 1
}
```

函数值可以放入对象，数组，字典和配置。

函数对象本身可以是常量。函数执行结果不是配置常量。

## Iterator

核心版本使用一个动态Iterator协议统一`for in`，标准库可迭代值和`co fn`。

Iterator对象提供`next()`。

```coflow
var step = iterator.next()
step.done
step.value
```

`next()`返回对象：

```coflow
{
  done: bool,
  value: any,
}
```

没有泛型版本的Iterator协议。

数组，字典，标准库`range`返回值和`co fn`调用结果都可以被迭代。

## 联合类型

核心版本不引入联合类型。

需要允许空值时，直接使用动态值和运行时检查。联合类型放入提案。
