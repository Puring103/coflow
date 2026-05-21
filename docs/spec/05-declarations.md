# coflow 声明

顶层（模块级别）只允许声明，不允许普通语句。

## import

导入其他模块：

```coflow
import common;
import weapons as w;
import game.utils as utils;
```

导入目标是点分隔的模块路径。需要缩短访问路径或避免命名冲突时，可以在模块路径后使用 `as` 指定别名。

导入后用 `module_name.member`（或 `alias.member`）访问被导入模块的公开成员：

```coflow
import weapons;

var dmg = weapons.sword.damage;
var r = weapons.Rarity.common;
```

核心版本限制：
- 不支持 `from module import name` 选择性导入
- 不支持通配符导入
- 允许声明级循环导入；配置值和顶层 `var` 初始化依赖必须无环，详见 [06-modules.md](./06-modules.md) 和 [07-config.md](./07-config.md)

`import` 只能出现在顶层。

## fn

声明命名函数：

```coflow
fn add(a, b) {
  return a + b;
}
```

函数声明以 `fn` 开头，后接函数名、参数列表、可选返回类型和函数体。函数体可以是 `{ }` 包围的块体，也可以是 `=>` 后接单个表达式。声明前加 `local` 表示文件内私有函数。

### 参数

参数写在函数名后的圆括号中。每个参数至少包含名称，可以追加 `: Type` 标注类型，也可以使用 `= default_expr` 指定默认值。

有默认值的参数在调用时可以省略：

```coflow
fn spawn(name: string, hp: int = 100, team: int = 0) {
  # ...
}

spawn("goblin");            # hp=100, team=0
spawn("boss", hp: 500);     # hp=500, team=0
spawn("elite", 200, 1);     # hp=200, team=1
```

默认值必须是常量表达式（字面量、枚举值、简单常量运算）。

### 返回类型

用 `->` 标注，可选：

```coflow
fn add(a: int, b: int) -> int {
  return a + b;
}
```

### 函数体形式

**块体**：

```coflow
fn greet(name: string) -> string {
  return "hello " + name;
}
```

**表达式体**（`=>` 后接单个表达式）：

```coflow
fn double(x: int) -> int => x * 2
```

### 具名参数调用

调用时可以用 `param_name: value` 传入具名参数：

```coflow
spawn("boss", hp: 500);
spawn("elite", 200, team: 1);
```

位置参数和具名参数可以混用，但具名参数之后不能再有位置参数。

### local

`local fn` 声明文件内私有函数：

```coflow
local fn helper(x) {
  return x * 2;
}
```

### 匿名函数表达式

函数可以作为值内联创建：

```coflow
var double = fn(x) => x * 2;

var greet = fn(name) {
  return "hello " + name;
};

var typed = fn(a: int, b: int) -> int => a + b;
```

### 闭包

函数可以捕获外层作用域的局部变量，形成闭包。捕获是**共享引用**：闭包内外对同一变量的修改互相可见：

```coflow
fn make_counter() {
  var count = 0;
  return fn() {
    count += 1;
    return count;
  };
}

var c = make_counter();
c();   # 1
c();   # 2
```

## iter fn

声明迭代器工厂函数：

```coflow
iter fn counter(start: int) {
  var i = start;
  while true {
    yield i;
    i += 1;
  }
}
```

`iter fn` 以 `iter fn` 开头，后接函数名、参数列表、可选返回类型和块体。声明前加 `local` 表示文件内私有迭代器函数。

`iter fn` 的行为：
- 调用不立即执行函数体，而是返回一个 Iterator 对象
- 函数体在 Iterator 的 `next()` 被调用时逐步执行
- `yield value` 暂停执行，将 `value` 作为本次迭代结果
- `yield from iterable` 委托子迭代器的所有值
- `return`（不带值）提前结束迭代；禁止 `return value`
- 函数体执行到末尾时迭代自然结束

约束：

- `iter fn` 的函数体必须**至少包含一处** `yield` 或 `yield from`（出现在不可达分支也满足要求）。完全不含 `yield` 的 `iter fn` 是编译期错误——这种函数应直接写为普通 `fn`
- `iter fn` 是单向迭代器：仅支持外部通过 `next()` 拉取，不支持 `send` / `throw` / 跨函数 yield。详见 [02-types.md](./02-types.md) 的"单向迭代器"

```coflow
var c = counter(1);
c.next();   # { done: false, value: 1 }
c.next();   # { done: false, value: 2 }
```

`iter fn` 也可以是匿名表达式：

```coflow
var gen = iter fn() {
  yield 1;
  yield 2;
};
```

详细执行模型见 [../design/05-runtime.md](../design/05-runtime.md)。

## class

声明对象的结构模板：

```coflow
class Weapon {
  id: string;
  name: string;
  damage: int;
  cooldown: float = 1.0;

  fn description() -> string {
    return f"{self.name} (damage: {self.damage})";
  };

  check {
    assert self.damage > 0 or "damage must be positive";
  };
}
```

`class` 声明以 `class` 开头，后接类型名和 `{ }` 包围的成员列表。声明前加 `local` 表示文件内私有类型。class 成员可以包含字段、方法和可选的 `check` 块。**字段、方法、`check` 块之间用 `;` 分隔，允许末尾分号。**

### 字段声明

字段声明由字段名、`: Type` 类型标注和可选默认值组成。字段的类型标注是必须的。有默认值的字段在配置中可以省略。默认值必须是常量表达式，不能引用 `self` 或同对象其他字段。

### 方法

class 内的 `fn` 声明为方法，方法内 `self` 隐式可用：

```coflow
class Player {
  hp: int;
  max_hp: int = 100;

  fn is_alive() -> bool => self.hp > 0;

  fn heal(amount: int) {
    self.hp += amount;
    if self.hp > self.max_hp {
      self.hp = self.max_hp;
    }
  };
}
```

方法体内的匿名函数 / `iter fn` 表达式可以捕获 `self`，按一般闭包"共享引用"规则处理：

```coflow
class Player {
  hp: int;
  max_hp: int = 100;

  fn make_healer() -> fn(int) {
    return fn(amount: int) {
      self.hp += amount;     # 捕获外层方法的 self
      if self.hp > self.max_hp {
        self.hp = self.max_hp;
      }
    };
  };
}
```

闭包持有的是当前方法 `self` 这个绑定，闭包寿命可以超过方法返回；通过该闭包修改 `self.field` 与对实例的直接修改等效。

### check 块

配置校验块，在配置加载期结构校验通过后执行，详见 [07-config.md](./07-config.md)。

一个 class 最多一个 `check` 块，位于字段和方法之后。

### local

`local class` 声明文件内私有类型：

```coflow
local class InternalState {
  phase: int;
  timer: float;
}
```

## enum

声明有限命名整数集合：

```coflow
enum Rarity {
  common;
  rare;
  epic;
}
```

`enum` 声明以 `enum` 开头，后接枚举名和 `{ }` 包围的变体列表。声明前加 `local` 表示文件内私有枚举。变体之间用 `;` 分隔，允许末尾分号。每个变体可以只写名称，也可以用 `=` 显式指定整数值。

变体从 `0` 开始自动编号，可以显式指定值，后续变体从前一个值 +1 继续：

```coflow
enum Status {
  none   = 0;
  active = 10;
  dead   = 20;
  ghost;        # 自动为 21
}
```

访问：`EnumName.variant`

```coflow
var r = Rarity.epic;
```

枚举与 `int` 不可隐式互转，详见 [02-types.md](./02-types.md)。

## 顶层 var

顶层运行时模块变量：

```coflow
var runtime_cache = null;
var instance_count: int = 0;
local var _internal = null;
```

- 在运行时模块初始化阶段（晚于配置求值）初始化
- 可以在运行时修改
- 配置定义不能依赖顶层 `var`

## 配置定义

顶层 `name = value` 或 `name: Type = value` 是配置定义，需要以 `;` 结尾：

```coflow
base_damage = 10;

sword: Weapon = {
  id: "sword",
  damage: base_damage,
};
```

与顶层 `var` 的区别：配置是只读的，值必须是常量表达式，在加载期求值。详见 [07-config.md](./07-config.md)。

## 重复声明

同一模块顶层作用域内，重复声明同名成员是错误：

```coflow
fn helper() { ... }
fn helper() { ... }  # 错误：重复声明
```

顶层声明之间可以互相引用，不受声明顺序限制（函数可以调用在其之后声明的函数）。
