# CFT Schema 语言

CFT（Coflow Type）定义配置数据的静态结构与校验规则。CFT 文件只包含 schema，不包含 CFD 记录；
`coflow cft check` 负责解析所有配置的 schema 模块并完成名称、类型、继承、默认值和注解检查。

```cft
enum Rarity {
  Common,
  Rare = 10,
}

type Item {
  name: string;
  rarity: Rarity = Common;
  tags: [string] = [];

  check {
    name != "";
  }
}
```

空白不参与语义，注释从 `#` 延续到行尾。标识符区分大小写。项目统一使用
2 空格缩进，可通过 `coflow format` 规范化排版。

## 文件与名称

项目中的所有 CFT 文件共同组成一个 schema。文件和目录只用于组织源码，不创建名称作用域。顶层
type、enum、const、类型别名和命名 check 均使用短名，并且必须在整个项目中唯一。

```cft
type ItemId = string;

type Item {
  id: ItemId;
}
```

类型引用同样使用短名。`::` 用于 `Enum::Variant` 等静态成员路径，不用于限定类型名。

## 顶层声明

### enum

enum 变体以逗号分隔，可显式指定 `i64` 值；未指定时由编译器分配稳定的顺序值。

```cft
enum Element {
  Neutral = 0,
  Fire,
  Ice,
}

@flag
enum Permission {
  Read = 1,
  Write = 2,
  Execute = 4,
}
```

`@flag` enum 除零值外只接受 2 的幂，CFD 中可用 `|`、`^`、`&` 组合值。

### type 与继承

普通 type 既可作为顶层记录类型，也可作为字段中的内联对象。`abstract` type 不能直接创建顶层记录，
可通过基类字段保存具体子类型；`sealed` type 不允许继续派生。

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

`Effect` 字段保存内联对象，`&Effect` 保存按 record key 解析的记录引用。派生 type 继承父 type 的字段和
check；一个 type 最多有一个直接父 type。object 可以通过 `Option`、数组、字典或记录引用递归包含自身，
直接或间接的必填 object 包含环会在 schema 检查时报错。

### 类型别名、常量和顶层 check

```cft
type ItemId = string;
type Callback = fn(value: int) -> Result<int, string>;

const MAX_LEVEL: int = 100;
const DEFAULT_TAGS = ["common"];

check ItemIntegrity {
  records(Item).len() > 0;
}
```

类型别名使用 `type Name = ValueType;`，只为已有类型提供名称，不创建新的 object type。const 可显式
声明类型，也可由默认值推断。命名顶层 check 用于跨记录规则，详见 [Check 校验](./04-check.md)。

## 值类型

| 类型 | CFT 写法 | CFD 值示例 |
| --- | --- | --- |
| 整数 | `int` | `42` |
| 浮点数 | `float` | `3.5` |
| 布尔 | `bool` | `true` |
| 字符串 | `string` | `"text"` |
| enum | `Rarity` | `Rare` |
| 内联对象 | `Stats` | `{ hp: 100 }` |
| 记录引用 | `&Item` | `&sword` |
| 数组 | `[T]` | `[a, b]` |
| 字典 | `{K: V}` | `{ "hp": 100 }` |
| 可选值 | `Option<T>` | `None`、`value`（也接受 `Some(value)`） |
| 结果值 | `Result<T, E>` | 函数协议、常量和表达式中的 `Ok(value)`、`Err(error)` |
| 函数 | `fn(name: T) -> R` | `fn(name: T) -> R { ... }` |
| unit | `()` | 仅用于函数签名 |

nullable 使用 `Option<T>`，没有 `T?` 类型简写。`Result<T, E>` 不作为 object 数据字段类型，可用于函数
参数、函数返回、常量和表达式。primitive、集合和 `Option` / `Result` 类型参数不做隐式转换；enum 也
不会隐式转换为 int。

## 字段与默认值

字段语法为 `name: Type;` 或 `name: Type = default;`。没有默认值的字段必须由 CFD 提供；有默认值的
字段在 CFD 省略时由构建阶段补齐。

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
  next: Option<&Item> = None;
  owner: &Item = &Item::default_item;
  effect: Effect = DamageEffect { label: "default", amount: 10 };
  normalize: fn(value: int) -> int = fn(input: int) -> int {
    if input > 0 { input } else { 0 }
  };
}
```

默认值支持 scalar、格式化字符串、enum/const 路径、flag 位表达式、数组、字典、内联对象、
`None`、`Some(...)`、记录引用和函数字面量。多态 object 字段可用
`ConcreteType { ... }` 指定具体子 type。CFT 中的记录引用默认值必须写为 `&Type::key`，以便在没有字段
上下文时确定目标记录类型。普通字符串包含插值时会自动识别为格式化字符串，`{{` / `}}` 分别表示
字面量 `{` / `}`，不存在 `f"..."` 语法。

函数字段的默认实现必须与字段签名一致，参数名不参与签名相等性。CFD 显式提供同字段函数时覆盖 CFT
默认实现；`@Host` type 的函数字段不能声明默认实现。函数默认值仅支持直接用于函数字段，不能嵌套在
集合、Option、Result 或 object 默认值中。Rust 数据模型保留函数源码；C# Runtime 的 `Load` 保留函数，
`LoadAndCompile` 才类型检查并编译函数体。默认值展开必须有限；`Some(object)`、非空集合或省略的 object
字段形成默认物化环时，schema 检查会报告错误。

## 注解

注解写在声明前；同一目标不能重复使用同名注解。

| 注解 | 目标 | 作用 |
| --- | --- | --- |
| `@label("...")` | object type、enum、变体、字段 | 编辑器显示名称 |
| `@description("...")` | object type、enum、变体、字段 | 编辑器说明 |
| `@flag` | enum | 位标志 enum |
| `@struct` | sealed type | 生成值类型 |
| `@singleton` | 具体 type | 约束该类型只有一个固定 key 的记录 |
| `@Host` | `@singleton` 具体 type | 声明由宿主提供的服务类型 |
| `@idAsEnum(Name)` | type | 用空 enum `Name` 为 record key 生成稳定枚举值 |
| `@expand` | 具体内联对象字段 | 在表格来源中展开子字段 |
| `@localized` / `@localized("bucket")` | 顶层 type 字段 | 绑定 `language` 维度 |
| `@dimension("name")` | 顶层 type 字段 | 绑定指定维度 |

`@localized` 与 `@dimension` 不能同时用于同一字段；维度字段不能位于 sealed 内联 type 中。
`@idAsEnum(Name)` 要求 `Name` 是无变体 enum，且不能与 `@singleton` 同时使用。
不在上表中的自定义注解会作为 schema metadata 保留，但不会获得内建语义。

## Check 块

type 内的 `check` 必须位于所有字段之后。普通条件、`when`、集合量词、运算符、内建方法、格式化诊断
消息和命名顶层 check 的完整规则见 [Check 校验](./04-check.md)。

```cft
type Monster {
  level: int;
  drops: [int] = [];

  check {
    1 <= level <= MAX_LEVEL;
    all count in drops {
      count >= 0;
    }
  }
}
```

## 检查边界

`coflow cft check` 只检查 schema，不加载 CFD；`coflow check` 会继续发现并解析 CFD、构建记录和引用，
再执行 check。格式化不等同于检查，`coflow format` 只规范化配置中的 `.cft` 与 `.cfd` 文本。
