# coflow 类型系统

coflow 是动态语言。类型标注可选，主要用于配置校验、宿主互操作和编辑器补全。

## 类型标注语法

类型标注跟在名称后面，以 `:` 分隔。类型名可以是基础类型、class 名、enum 名，或带命名空间的路径名。

复合类型按以下规则书写：

- 数组类型写作 `[T]`，表示元素类型为 `T` 的数组。
- 字典类型写作 `dict[K, V]`，表示键类型为 `K`、值类型为 `V` 的动态键值映射。
- 函数类型可以写作 `fn`，表示任意函数；也可以写作 `fn(T1, T2) -> R`，指定参数类型和返回类型。
- 函数类型省略 `-> R` 时，只约束参数类型，不约束返回值。
- 类型可以嵌套，例如 `[dict[string, int]]` 或 `fn([int]) -> dict[string, bool]`。

示例：

```coflow
var hp: int = 100;
var names: [string] = [];
var scores: dict[string, int] = {};
var handler: fn(int) -> bool = check_alive;
var transform: fn = double;
```

## 基础类型

| 类型     | 说明 |
|----------|------|
| `int`    | 整数 |
| `float`  | 浮点数 |
| `bool`   | 布尔值，`true` 或 `false` |
| `string` | 字符串 |
| `null`   | 空值，只有一个值 `null` |
| `any`    | 任意动态值 |

```coflow
var hp: int = 1_000;
var speed: float = 3.5;
var name: string = "hero";
var alive: bool = true;
var target: any = get_target();
```

## null

`null` 表示空值，是独立类型。

缺失的对象字段和越界的数组索引读取结果是 `null`：

```coflow
var obj = { name: "hero" };
var x = obj.missing;    # null

var arr = [1, 2, 3];
var y = arr[10];        # null
```

没有 `void` 类型。不带返回值的函数隐式返回 `null`，`return` 不带值也等价于 `return null`。

## any

`any` 表示任意动态值，可以持有任何值。

在 `any` 类型的值上访问不存在的成员会在运行时显式报错，不会静默返回 `null`：

```coflow
var value: any = host.get("player");
value.move(1, 0);       # 运行时决议，合法
value.nonexistent;      # 若不存在，运行时报错
```

## int

整数类型，精度由宿主平台决定（通常为 64 位有符号整数）。

整数字面量写法详见 [03-expressions.md](./03-expressions.md)。

## float

浮点数类型，通常为 IEEE 754 双精度浮点。

浮点字面量写法详见 [03-expressions.md](./03-expressions.md)。

## bool

布尔类型，值为 `true` 或 `false`。

```coflow
var alive: bool = true;
var dead: bool = false;
```

## string

字符串类型，表示 Unicode 文本序列，是不可变值。

字符串字面量包括普通字符串、原始字符串和多行字符串，详见 [03-expressions.md](./03-expressions.md)。

字符串拼接使用 `+`：

```coflow
var greeting = "hello " + name;
```

## 数组

数组表示同类型值的有序列表，类型标注为 `[T]`。

```coflow
var damages: [int] = [10, 20, 30];
var names: [string] = ["a", "b", "c"];
var mixed: [any] = [1, "hello", true];
```

数组支持：
- 索引访问：`arr[i]`，越界返回 `null`
- 可选索引访问：`arr?[i]`，左侧为 `null` 时返回 `null`
- `for in` 迭代（按索引顺序产出元素）
- `in` 成员判断

数组字面量和访问语法详见 [03-expressions.md](./03-expressions.md)。

## 对象

对象表示静态字段集合，字段名固定，字段值可以是不同类型。

```coflow
var weapon = {
  id: "sword",
  damage: 10,
};
```

对象使用 `.` 访问字段：

```coflow
var damage = weapon.damage;
```

访问不存在的字段返回 `null`。

对象字面量支持 `...` 展开合并：

```coflow
base = { damage: 10, speed: 1.0 };
sword = { ...base, name: "Iron Sword", damage: 15 };
```

展开时，后面出现的同名字段覆盖前面的值。

## 字典

字典表示动态键值映射，键和值类型同构，类型标注为 `dict[K, V]`。

```coflow
var scores: dict[string, int] = {
  "alice": 10,
  "bob": 20,
};
```

字典使用索引访问：

```coflow
var score = scores["alice"];
```

字典**不支持**点访问。键不存在时返回 `null`。

字典支持：
- 索引访问：`dict[key]`
- `for in` 迭代，产出 entry 对象（含 `key` 和 `value` 字段）
- `in` 成员判断（判断 key 是否存在）

```coflow
for entry in scores {
  print(entry.key);
  print(entry.value);
}

if "alice" in scores {
  print(scores["alice"]);
}
```

## 对象与字典的区别

对象和字典在字面量上都使用 `{ }` 语法，消歧规则如下：

**有类型上下文时**，按类型上下文决定：

```coflow
# class 类型上下文 → 对象，按 class 结构校验
sword: Weapon = {
  id: "sword",
  damage: 10,
}

# 字典类型上下文 → 字典
scores: dict[string, int] = {
  "alice": 10,
  "bob": 20,
}
```

**无类型上下文时**，按 key 形式判断：

- 标识符 key（`name: value`）→ 对象
- 字符串 key（`"name": value`）→ 字典

```coflow
# 对象（标识符 key）
var weapon = {
  id: "sword",
  damage: 10,
};

# 字典（字符串 key），推断为 dict[string, int]
var scores = {
  "alice": 10,
  "bob": 20,
};

# 字典（字符串 key），值类型异构，推断为 dict[string, any]
var meta = {
  "name": "hero",
  "level": 5,
};
```

此规则适用于所有无类型上下文的位置：`var` 右值、函数参数、数组元素等。

核心版本不支持动态 key 字面量语法（如 `{ [expr]: value }`）。

## class

`class` 声明对象的结构模板，用于配置校验和类型标注。

```coflow
class Weapon {
  id: string
  name: string
  damage: int
  cooldown: float = 1.0
}
```

字段声明语法：`name: Type` 或 `name: Type = default`。有默认值的字段在配置中可以省略。字段默认值必须是常量表达式，不能引用 `self` 或同一对象的其他字段。

class 字段的类型标注是必须的（不可省略）。

### class 方法

class 中可以定义方法：

```coflow
class Vector {
  x: float
  y: float

  fn length() -> float => (self.x ** 2 + self.y ** 2) ** 0.5

  fn scale(factor: float) {
    self.x *= factor;
    self.y *= factor;
  }
}
```

方法内 `self` 隐式可用，指向当前对象实例。方法语法与顶层 `fn` 相同。

### check 块

`check` 是 class 内的配置校验块，在配置加载期完成结构校验后执行。

```coflow
class Range {
  min: int
  max: int

  check {
    self.min <= self.max => "min must be <= max"
  }
}
```

每条 check 语句格式为 `condition => message`：

- `condition` 为 `true` 时校验通过
- `condition` 为 `false` 时，以 `message` 作为错误信息报告加载期配置错误

`check` 块内 `self` 隐式可用。check 块的约束：

1. 禁止修改 `self` 或任何外部状态
2. 禁止调用宿主 API
3. 只允许读取 `self` 字段和使用纯计算逻辑

一个 class 只允许一个 `check` 块，块内可以有多条语句：

```coflow
class Skill {
  id: string
  damage: int
  cooldown: float

  check {
    self.damage > 0       => "damage must be positive"
    self.cooldown >= 0.0  => "cooldown must be non-negative"
  }
}
```

校验错误是加载期诊断，不通过脚本 `try catch` 捕获。

## enum

`enum` 定义有限的命名整数集合。

```coflow
enum Rarity {
  common
  rare
  epic
}
```

枚举变体默认从 `0` 开始自动编号，依次递增。可以显式指定整数值，未指定的变体从前一个值 +1 继续：

```coflow
enum Status {
  none   = 0
  active = 10
  dead   = 20
  ghost        # 值为 21
}
```

使用枚举值通过 `EnumName.variant` 语法：

```coflow
rarity = Rarity.common;
status = Status.active;
```

枚举底层类型为 `int`，枚举值可以与整数比较。

## 函数类型

函数是一等值，可以赋给变量、传入参数、放入数组和对象。

函数类型标注：

- `fn`：任意函数（不约束参数和返回值）
- `fn(T1, T2) -> R`：指定参数类型列表和返回类型
- `fn(T1, T2)`：指定参数类型，不约束返回值

```coflow
var handler: fn(int) -> bool = check_alive;
var transform: fn(int) -> int = fn(x) => x * 2;
var callback: fn = on_event;
```

函数值包括命名函数引用、匿名函数表达式和 lambda 表达式，详见 [05-declarations.md](./05-declarations.md)。

## Iterator

Iterator 是支持逐步迭代的对象，提供 `next()` 方法。

`next()` 返回一个对象：

```coflow
{
  done: bool,
  value: any,
}
```

- `done` 为 `false` 时，`value` 是本次产出的值
- `done` 为 `true` 时，迭代结束，`value` 固定为 `null`
- 已结束的 Iterator 再次调用 `next()` 仍返回 `{ done: true, value: null }`

以下值可以被 `for in` 迭代（内部通过 `iter()` 获取 Iterator）：

- 数组
- 字典（产出 entry 对象，含 `key` 和 `value` 字段）
- Range 字面量
- `iter fn` 调用结果
- 已经是 Iterator 的对象（直接使用）

## 联合类型

核心版本不支持联合类型。需要允许空值时，直接使用 `any` 和运行时检查。
