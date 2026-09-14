# CFT 声明

> 状态：语言设计
>
> 范围：CFT 的顶层声明、字段、默认值、继承、注解和函数字段。

CFT 定义配置数据的静态结构与校验规则。CFT 文件只包含 schema，不包含 CFD 记录。命名空间规则见
[08-命名空间](./08-命名空间.md)，check 规则见 [07-Check校验](./07-Check校验.md)。

## 1. 文件结构

```text
cft-file       := [ namespace-decl ] { use-decl } { declaration }
namespace-decl := "namespace" qualified-name ";"
use-decl       := "use" qualified-name ";"
qualified-name := identifier { "::" identifier }
declaration    := { annotation } ( enum-decl | type-decl | alias-decl | const-decl | check-decl )
annotation     := "@" identifier [ "(" [ annotation-arg { "," annotation-arg } [ "," ] ] ")" ]
```

```cft
namespace game::combat;

use std::Check::require;

enum Rarity {
  Common,
  Rare = 10,
}

type Item {
  name: string;
  rarity: Rarity = Common;
  tags: [string] = [];

  check {
    require(name != "", "名称不能为空");
  }
}
```

## 2. enum

```text
enum-decl := "enum" identifier "{" [ variant { "," variant } [ "," ] ] "}"
variant   := identifier [ "=" int-literal ]
```

enum 变体以逗号分隔，可显式指定非负 `i32` 值，enum 值不允许为负。

- 普通 enum 未指定值时，从 `0` 开始按前一个值 `+1` 分配。
- `@flag` enum 未指定值时总是取下一个未使用的 2 的幂（`1、2、4、…`）；显式 `= 0` 声明零值变体；非零
  显式值必须是 2 的幂且不重复。
- `@flag` 的底层值是 `i32`，只使用 bit 0..30，最多 31 个标志。

```cft
enum Element {
  Neutral = 0,
  Fire,
  Ice,
}
```

`@flag` 修饰的 enum 除零值外只接受 2 的幂：

```cft
@flag
enum Permission {
  Read = 1,
  Write = 2,
  Execute = 4,
}
```

## 3. type 与继承

```text
type-decl := [ "abstract" | "sealed" ] "type" identifier [ ":" qualified-name ]
             "{" { member } "}"
member    := field-decl | check-block
```

```cft
abstract type Effect {
  label: string;
}

sealed type DamageEffect : Effect {
  amount: int;
}

type EffectBundle {
  primary: Effect;
  source: &Effect;
}
```

- 普通 type 既可作为顶层记录类型，也可作为字段中的内联对象。
- `abstract` type 不能直接创建顶层记录，可通过基类字段保存具体子类型。
- `sealed` type 不允许继续派生。
- enum 与 `@struct` type 都不参与继承；`@struct` 只能修饰 `sealed` type。
- 一个 type 最多有一个直接父 type；派生 type 继承父 type 的字段和 check。
- object 可以通过可选类型、数组、字典或记录引用递归包含自身；直接或间接的必填 object 包含环在
  schema 检查时报错。

`@struct` 修饰 `sealed` type，生成值类型：

```cft
@struct
sealed type Point {
  x: int;
  y: int;
}
```

## 4. 字段

```text
field-decl := { annotation } identifier ":" type [ "=" value ] ";"
```

字段语法为 `name: Type;` 或 `name: Type = default;`。没有默认值的字段必须由 CFD 提供；有默认值
的字段在 CFD 省略时由构建阶段补齐。

字段的取值优先级：

| CFD 状态 | 结果 |
| --- | --- |
| 提供值 | 使用提供的值 |
| 省略且有默认值 | 使用 CFT 默认值 |
| 省略且无默认值且字段可选 | `None` |
| 省略且无默认值且字段必填 | 缺少必填字段错误 |
| 显式 `None` | 明确为空，即使默认值不是 `None` |

```cft
type Stats {
  hp: int = 100;
  title: string = "Unknown";
  label: string = "HP: {hp}";
  enabled: bool = true;
  rarity: Rarity = Common;
  permissions: Permission = Permission::Read | Permission::Write;
  tags: [string] = [];
  attrs: {string: int} = { "attack": 10 };
  next: Item? = None;
  offset: int = -5;
  owner: &Item = &default_item;
  effect: Effect = DamageEffect { label: "default", amount: 10 };
}
```

- 默认值支持 scalar、格式化字符串、enum/const 路径、flag 位表达式、数组、字典、内联对象、`None`、
  存在值和函数字面量。
- 数值默认值可带一元负号，例如 `-1`、`-0.5`。
- 多态 object 字段可用 `ConcreteType { ... }` 指定具体子 type。
- 记录引用默认值与字段声明类型相同时写 `&key`，需要显式目标类型时写 `&Type::key`。
- 默认值展开必须有限；非空可选值、非空集合或省略的 object 字段形成默认物化环时报错。

## 5. 类型别名与常量

```text
alias-decl := "type" identifier "=" type ";"
const-decl := "const" identifier ":" type "=" value ";"
```

```cft
type ItemId = string;
type Callback = fn(value: int) -> int;

const MAX_LEVEL: int = 100;
const DEFAULT_TAGS: [string] = ["common"];
```

类型别名只为已有类型提供名称，不创建新的 object type。`const` 必须声明类型；局部变量可由初始值推断类型。

## 6. 注解

```text
annotation := "@" identifier [ "(" [ annotation-arg { "," annotation-arg } ] ")" ]
```

注解写在声明前；同一目标不能重复使用同名注解。

| 注解 | 目标 | 作用 |
| --- | --- | --- |
| `@label("...")` | object type、enum、变体、字段 | 编辑器显示名称，并生成目标代码注释 |
| `@description("...")` | object type、enum、变体、字段 | 编辑器说明，并生成目标代码注释 |
| `@flag` | enum | 位标志 enum |
| `@struct` | sealed type | 生成值类型 |
| `@singleton` | 具体 type | 约束该类型只有一个固定 key 的记录 |
| `@Host` | `@singleton` 具体 type | 声明由宿主提供的服务类型 |
| `@idAsEnum(Name)` | type | 用空 enum `Name` 为 record key 生成稳定枚举值 |
| `@localized` | 顶层 type 字段 | 绑定 `language` 维度 |
| `@dimension("name")` | 顶层 type 字段 | 绑定指定维度 |

```cft
@label("物品")
type Item {
  @label("名称")
  @description("显示给玩家的名称")
  name: string;

  @dimension("language")
  description: string?;
}

@flag
enum Permission {
  Read = 1,
  Write = 2,
  Execute = 4,
}
```

`@singleton`、`@Host` 和 `@idAsEnum` 的用法：

```cft
@singleton
type Settings {
  version: string;
}

@Host
@singleton
type Logger {
  info: fn(message: string) -> ();
}

enum ItemId {}

@idAsEnum(ItemId)
type Item {
  name: string;
}
```

约束：

- `@localized` 与 `@dimension` 不能同时用于同一字段。
- 维度字段不能位于 sealed 内联 type 中。
- `@idAsEnum(Name)` 要求 `Name` 是无变体 enum，且不能与 `@singleton` 同时使用。
- 自定义注解作为 schema metadata 保留。

## 7. 函数字段

```text
field-decl := { annotation } identifier ":" func-type [ "=" function-literal | "=>" block ] ";"
```

```cft
type Calculator {
  classify: fn(value: int) -> string => {
    if value >= 10 { "large" } else { "small" }
  };
}
```

- CFT 声明函数签名，普通函数字段可以同时声明默认 body。
- 默认 body 用 `=>` 直接跟随字段的 `func-type`，省略重复签名，参数名取自字段签名；也可用 `= fn(...) -> ... { ... }` 的完整写法。`=>` 要求字段 `func-type` 为所有参数命名，否则必须使用完整写法。
- CFD 提供同字段函数值时覆盖该默认实现。
- 函数字段的默认实现必须与字段签名一致，参数名不参与签名相等性。
- 函数默认值只允许直接用于函数字段，不能嵌套在集合、可选类型或 object 默认值中。
- `@Host` type 的函数字段可以声明默认实现；宿主绑定优先覆盖默认实现。

## 8. check 块

```text
check-block := "check" [ identifier ] "{" function-body "}"
check-decl  := "check" identifier "{" function-body "}"
```

```cft
type Monster {
  level: int;
  drops: [int] = [];

  check {
    require(1 <= level <= 100, "等级必须在 1 到 100 之间");
    for drop in drops {
      require(drop >= 0, "掉落数量不能为负");
    }
  }
}
```

```cft
check ItemIntegrity {
  require(records(Item).len() > 0, "项目中至少需要一个物品");
}
```

type 内的 `check` 必须位于所有字段之后，一个类型只能有一个 `check` 块，可带可选名称。完整规则见
[07-Check校验](./07-Check校验.md)。
