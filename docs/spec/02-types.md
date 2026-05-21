# coflow 类型系统

coflow 是动态语言。类型标注可选，主要用于配置校验、宿主互操作和编辑器补全。

## 类型标注语法

类型标注跟在名称后面，以 `:` 分隔。类型名可以是基础类型、class 名、enum 名，或带命名空间的路径名。

复合类型按以下规则书写：

- 数组类型写作 `[T]`，表示元素类型为 `T` 的数组。
- 字典类型写作 `{K: V}`，表示键类型为 `K`、值类型为 `V` 的动态键值映射。
- 函数类型可以写作 `fn`，表示任意函数；也可以写作 `fn(T1, T2) -> R`，指定参数类型和返回类型。
- 函数类型省略 `-> R` 时，只约束参数类型，不约束返回值。
- 类型可以嵌套，例如 `[{string: int}]` 或 `fn([int]) -> {string: bool}`。

示例：

```coflow
var hp: int = 100;
var names: [string] = [];
var scores: {string: int} = dict{};
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

`null` 表示空值，是独立类型。`null` 不能赋给除 `any` 外的基础类型或 class 类型变量：

```coflow
var hp: int = null;       # 错误：null 不能赋给 int
var name: string = null;  # 错误
var v: any = null;        # 合法
var x;                    # 合法，无类型标注，等价于 any，初值 null
```

需要"可选值"语义时，先使用 `any`；未来引入联合类型后用 `T | null` 表达。核心版本不提供专门的可空类型语法。

普通字段访问、索引访问和字典 key 访问是严格访问。字段不存在、数组越界或字典 key 不存在都会报告运行时错误。需要允许缺失时，使用可选访问 `?.` 或 `?[]`：

```coflow
var obj = { name: "hero" };
var x = obj.missing;    # 错误：字段不存在
var y = obj?.missing;   # null

var arr = [1, 2, 3];
var a = arr[10];        # 错误：数组越界
var b = arr?[10];       # null
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

整数类型，为 64 位有符号整数（`i64`）。超出 `i64` 表示范围（`-2^63` ~ `2^63 - 1`）的整数字面量是词法或语义错误，包括十进制和带进制前缀的字面量（如 `0xFFFF_FFFF_FFFF_FFFF` 因超 `i64` 最大值而非法，需要全 1 位模式时使用 `~0`）。

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

字符串字面量包括普通字符串、原始字符串、多行字符串和插值字符串，详见 [03-expressions.md](./03-expressions.md)。

### 字符串拼接

`+` 在任一操作数为 `string` 时执行拼接，另一侧的非字符串操作数会先调用其 `to_string` 转换：

```coflow
var greeting = "hello " + name;        # string + string
var msg = "hp: " + hp;                  # string + int → 自动转字符串
var debug = "items: " + [1, 2, 3];      # string + array
```

转换规则：

| 类型 | `to_string` 行为 |
|------|------------------|
| `int` / `float` | 标准十进制表示，`float` 保留至少一位小数 |
| `bool` | `"true"` / `"false"` |
| `null` | `"null"` |
| `string` | 自身 |
| 数组 / 对象 / 字典 | 调试用人类可读形式，**不保证**形态稳定，禁止用于序列化 |
| 函数 / Iterator | 含名称或地址的占位形式 |

`+` 两侧均非字符串、且均非数值时，是类型错误。需要拼接调试信息时优先使用插值字符串 `f"..."`（见 [03-expressions.md](./03-expressions.md)）。

字符串支持 `.len` 字段访问，返回 Unicode 标量值数（不是 UTF-8 字节数）：

```coflow
var n = "你好".len;   # 2
```

## 数组

数组表示同类型值的有序列表，类型标注为 `[T]`。

```coflow
var damages: [int] = [10, 20, 30];
var names: [string] = ["a", "b", "c"];
var mixed: [any] = [1, "hello", true];
```

数组支持：
- 索引访问：`arr[i]`，越界时报运行时错误
- 可选索引访问：`arr?[i]`，左侧为 `null` 或索引越界时返回 `null`
- `for in` 迭代（按索引顺序产出元素）
- `in` 成员判断
- `.len` 字段访问，返回元素数量
- `+` 浅 concat 产生新数组，原数组不变；嵌套元素仍为引用共享
- `arr.push(x)` 原地追加（**规范的追加写法**）

`+=` 严格展开为 `x = x + y`，因此 `arr += [x]` 创建新数组并重新绑定变量；旧数组的别名不会观察到变化。需要原地追加时使用 `arr.push(x)`。

```coflow
var a = [1, 2];
var b = a;
a += [3];           # a 重新绑定到新数组 [1, 2, 3]，b 仍是 [1, 2]
a.push(4);          # 此后 a 与之前重新绑定到的数组都被原地修改
```

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

访问不存在的字段时报运行时错误。

对象字面量支持 `...` 展开合并：

```coflow
base = { damage: 10, speed: 1.0 };
sword = { ...base, name: "Iron Sword", damage: 15 };
```

展开时，后面出现的同名字段覆盖前面的值。

可选字段访问 `obj?.field` 在 `obj` 为 `null` 或字段不存在时返回 `null`。若 `obj` 的静态类型是封闭 class，访问 class 未声明字段仍是语义错误，`?.` 不能绕过静态结构校验。

## 字典

字典表示动态键值映射，键和值类型同构，类型标注为 `{K: V}`。

```coflow
var scores: {string: int} = dict{
  "alice": 10,
  "bob": 20,
};
```

字典使用索引访问：

```coflow
var score = scores["alice"];
```

字典**不支持**点访问。普通索引访问要求 key 必须存在，key 不存在时报运行时错误。需要允许 key 缺失时，使用可选索引访问：

```coflow
var score = scores?["alice"];   # key 不存在时返回 null
```

字典支持：
- 索引访问：`dict[key]`
- 可选索引访问：`dict?[key]`
- `for in` 迭代，产出 entry 对象（含 `key` 和 `value` 字段）
- `in` 成员判断（判断 key 是否存在）
- `.len` 字段访问，返回 entry 数量

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

对象和字典使用不同字面量语法：

- `{ }` 永远表示对象字面量
- `dict{ }` 永远表示字典字面量

```coflow
# 对象，按 class 结构校验
sword: Weapon = {
  id: "sword",
  damage: 10,
};

# 字典
scores: {string: int} = dict{
  "alice": 10,
  "bob": 20,
};
```

```coflow
# 对象
var weapon = {
  id: "sword",
  damage: 10,
};

# 字典，推断为 {string: int}
var scores = dict{
  "alice": 10,
  "bob": 20,
};

# 字典，值类型异构，推断为 {string: any}
var meta = dict{
  "name": "hero",
  "level": 5,
};
```

空字典必须写作 `dict{}`。核心版本不支持动态 key 字面量语法（如 `dict{ [expr]: value }`）。

## class

`class` 声明对象的结构模板，用于配置校验和类型标注。

```coflow
class Weapon {
  id: string;
  name: string;
  damage: int;
  cooldown: float = 1.0;
}
```

class 内的字段、方法、`check` 块之间用 `;` 分隔，允许末尾分号。换行不作为分隔符。

字段声明语法：`name: Type` 或 `name: Type = default`。有默认值的字段在配置中可以省略。字段默认值必须是常量表达式，不能引用 `self` 或同一对象的其他字段。

class 字段的类型标注是必须的（不可省略）。

### class 方法

class 中可以定义方法：

```coflow
class Vector {
  x: float;
  y: float;

  fn length() -> float => (self.x ** 2 + self.y ** 2) ** 0.5;

  fn scale(factor: float) {
    self.x *= factor;
    self.y *= factor;
  };
}
```

方法内 `self` 隐式可用，指向当前对象实例。方法语法与顶层 `fn` 相同。

### check 块

`check` 是 class 内的配置校验块，在配置加载期完成结构校验后执行。块内由若干 `assert` 语句组成。

```coflow
class Range {
  min: int;
  max: int;

  check {
    assert self.min <= self.max or "min must be <= max";
  }
}
```

每条 `assert` 语句的形式：

```
assert <bool-expr> or <string-expr>;
```

- `bool-expr` 求值为真时校验通过；为假时校验失败，求值 `string-expr` 作为错误信息报告加载期配置错误
- `string-expr` 仅在 `bool-expr` 为假时才求值（短路）
- `bool-expr` 可以是任意纯表达式，**包括块表达式**，便于引入局部变量做中间计算
- `string-expr` 可以是任意求值为字符串的纯表达式，包括字符串拼接和插值字符串

`assert ... or ...` 是 `check` 块内的专用语法：这里的 `or` 不是普通逻辑或运算符，而是 `assert` 句法的一部分。普通函数体中不能写 `assert`。

`check` 块内 `self` 隐式可用。约束：

1. 禁止修改 `self` 或任何外部状态
2. 禁止调用宿主 API、IO、随机数、当前时间
3. 仅允许读取 `self` 字段、`env` 命名空间、引用其他配置；可以调用纯方法（保留二期定义）

一个 class 至多一个 `check` 块，块内可有多条 `assert`。多条 `assert` 按出现顺序求值，**第一条失败即中止该 class 实例的校验**，避免一次抛出大量级联错误：

```coflow
class Skill {
  id: string;
  damage: int;
  cooldown: float;

  check {
    assert self.damage > 0 or "damage must be positive";
    assert self.cooldown >= 0.0 or "cooldown must be non-negative";
    assert (self.damage / self.cooldown) < 100.0
      or f"DPS too high for skill {self.id}: {self.damage / self.cooldown}";
  }
}
```

校验错误是加载期诊断，不通过脚本 `try catch` 捕获。

## enum

`enum` 定义有限的命名整数集合。变体之间用 `;` 分隔，允许末尾分号。

```coflow
enum Rarity {
  common;
  rare;
  epic;
}
```

枚举变体默认从 `0` 开始自动编号，依次递增。可以显式指定整数值，未指定的变体从前一个值 +1 继续：

```coflow
enum Status {
  none   = 0;
  active = 10;
  dead   = 20;
  ghost;        # 值为 21
}
```

使用枚举值通过 `EnumName.variant` 语法：

```coflow
rarity = Rarity.common;
status = Status.active;
```

枚举底层表示为 `int`，但 `enum` 类型与 `int` **不可隐式互转**：

```coflow
var s: Status = Status.active;
var i: int = s;             # 错误：enum 不能隐式转为 int
var t: Status = 10;          # 错误：int 不能隐式转为 enum

if s == Status.active { ... }   # 合法：同枚举类型比较
if s == 10 { ... }               # 错误：enum 与 int 比较
```

未来引入显式转换语法 `expr as Type` 后，`s as int` / `(10 as Status)` 用于显式转换。核心版本不提供该语法，需要数值的场合应直接使用 `int`。

枚举仅命名整数，不携带数据载荷。需要带数据的"代数数据类型"（`Effect = Heal(amount) | Damage(amount, kind)`）的场景，使用对象 + `kind` 字段模拟，或等待二期联合类型。

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

函数值包括命名函数引用和匿名函数表达式，详见 [05-declarations.md](./05-declarations.md)。

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

### `for in` 解析顺序

`for x in obj` 按以下顺序决定如何取得 Iterator：

1. **内置可迭代值** — 数组、字典、Range、`iter fn` 调用结果，由 runtime 直接迭代，不查找用户方法
2. **已经是 Iterator** — `obj` 自身有 `next` 方法字段且它是函数时，直接把 `obj` 作为 Iterator 使用
3. **`obj.iter()`** — 否则调用 `obj.iter()` 获取 Iterator（要求返回的对象满足 Iterator 协议）

`iter` 是一个固定方法名（无双下划线、无修饰符），用户在 class 中显式声明：

```coflow
class Counter {
  start: int;
  end: int;

  fn iter() {
    return iter fn() {
      var i = self.start;
      while i < self.end {
        yield i;
        i += 1;
      }
    }();
  };
}

for v in Counter{ start: 0, end: 5 } {
  print(v);   # 0..4
}
```

字典 `for in` 产出 entry 对象（含 `key` 和 `value` 字段，顺序未规定）。Range `for in` 产出整数序列。

### 单向迭代器

`iter fn` 的 yield 是**单向**的：调用方通过 `next()` 拉取下一个值，不能向 Iterator 发送数据（无 `send`）、不能向其注入异常（无 `throw`）、不能跨函数 yield。需要双向通信或完整协程的场景由宿主程序处理，coflow 核心版本不提供。

## 联合类型

核心版本不支持联合类型。需要允许空值时，直接使用 `any` 和运行时检查。计划在二期引入 `T | null`（可空类型）和有限的联合类型语法。
