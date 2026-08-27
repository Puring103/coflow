# CFT / CFD 语言与运行时设计

> 状态：当前语言设计
> 定位：语法、语义以及编译器和运行时的核心约束
> 范围：CFT、CFD、数据语法、类型语法、表达式、函数、宿主注入、C# 运行时编译与 VM 核心语义

本文档只定义目标版本。旧语法、旧 API 和旧命名直接删除，不保留兼容别名、自动转换、fallback
或其他兜底路径。

## 1. 设计目标

- CFT 定义 schema、函数签名、默认值和校验规则。
- CFD 提供符合项目 schema 的只读配置记录和函数实现。
- CFT 与 CFD 不通过 import 建立文件依赖，由项目编译器统一收集并配对验证。
- CFT 与 CFD 共享标识符、类型和值语法；各自只保留符合其职责的顶层结构。
- CFT 不随 C# 运行时分发；CFT codegen 生成项目类型以及加载 CFD 所需的内部 schema 元数据。
- 普通配置保持结构化；任意计算表达式只进入函数体和 check。
- 函数是一等值，可以传递、保存、返回和放入运行时集合。
- 语言只保留一种 `fn`；CFD 函数与 `@Host` 的 C# delegate 共享函数身份、签名和调用 ABI，调用方
  不区分具体实现来源。
- 当前版本只提供同步执行，不提供协程、暂停、恢复、`await`、后台调用或任务句柄。
- C# Runtime 直接读取一个或多个 CFD 字符串。`LoadData` 只加载数据，`LoadAndCompile` 还会检查并
  编译函数；二者都发布 generation，其中普通数据和字节码不可变，`@Host` 绑定位置只能填充一次。
- 当前版本不生成或加载数据 artifact，不计算 CFT contract hash，也不在 C# Runtime 中加载 CFT。
- C# Runtime 的顶层 API 固定为 `Coflow.LoadData`、`Coflow.LoadAndCompile`、`Coflow.Combine` 和
  返回的 `CoflowModule`；codegen 不生成项目级 Runtime 入口或数据根类。
- `CoflowModule` 只表示 CFD source part 及其 generation，可以自由组合、编译和定向替换；固定的
  生成 CFT metadata 称为 generated contract，不属于可替换 module。
- 同一进程可以由 module initializer 注册多个 generated contract 片段；首次 Runtime 加载时将它们
  合并并冻结，名称冲突报错。此后不能动态增加或替换 CFT 合同，也不使用字符串选择合同。
- 记录按 key 查找统一使用 `CoflowTable<T>.Get(...) -> Option<T>`；命中返回 `Some(record)`，
  未命中返回 `None`，记录缺失不是异常。

## 2. CFT 与 CFD 的配合

CFT 和 CFD 是紧密配合的两种项目文件，而不是一门通用脚本语言的两个子集：

| 扩展名 | 职责 | 允许的顶层内容 |
| --- | --- | --- |
| `.cft` | schema 与约束 | `const`、`enum`、object type、类型别名、check |
| `.cfd` | 配置与实现 | 记录和记录分组 |

CFD 中的记录类型、字段类型和持久函数位置均由项目 schema 确定。应用只通过 CFT 生成的类型、
属性、函数方法和绑定方法访问这些内容；Runtime 不公开按字符串定位 type、field 或 function 的
动态 API。普通 record key 仍是数据，可以是 `string` 或 `@idAsEnum` 生成的 enum。

### 2.1 统一编译顺序

项目编译不按文件扩展名决定语义先后，而按声明类别建立依赖图：

1. 读取全部 `.cft`，收集 `const`、`enum`、type、类型别名和函数签名。
2. 读取全部 `.cfd`，收集记录 ID 和字段值。
3. 建立常量、默认值、插值和记录引用的依赖图，拒绝依赖环。
4. 验证全部记录、函数实现、记录引用以及 `@Host @singleton` 声明约束，并拒绝 CFD 为
   `@Host` type 声明记录。
5. Rust 项目工具按现有语义执行 CFT check，并由 codegen 生成项目 C# 类型与内部 schema metadata。

C# Runtime 随后使用生成代码直接加载 CFD。`LoadAndCompile` 在加载数据后对函数体完成名称解析、
类型检查和字节码生成；`LoadData` 不执行这些函数阶段。

两种文件都支持项目级前向引用，不要求 schema、常量和记录按文件顺序排列。

## 3. 文件、名称与词法

### 3.1 文件职责

- CFT 和 CFD 都可以声明一个文件级 namespace，并使用 `use` 缩短静态名称。
- 同一 namespace 内的 `const`、`enum`、object type 和类型别名顶层名称必须唯一。
- 支持前向引用，不要求先声明后使用。

namespace 使用 `::` 连接路径：

```cft
namespace game::items;

use game::common::Position;
use platform::services::Services as Api;

type Item {
  position: Position;
  api: &Api;
}
```

- 一个文件最多有一个 `namespace`，位于全部 `use` 和声明之前；省略时属于根 namespace。
- 同一 namespace 可以分布在多个文件中。
- `use` 位于 namespace 之后、其他声明之前，只建立名称别名，不加载文件或决定编译顺序。
- `use path::Name;` 引入单个 namespace-level CFT 声明；`use path::Name as Alias;` 指定本地别名。
  它不直接导入 namespace、record key、enum variant 或字段。
- 不支持通配符 `use`。同名 import、当前 namespace 声明和 alias 发生冲突时报错。
- 多段名称按绝对路径解析；当前 namespace 内的名称可以直接使用短名称。
- `::` 只用于 namespace、类型、enum、常量和 record key 等静态路径；`.` 只用于运行时字段和方法。

完整名称参与稳定符号身份。类型身份是 `namespace + type name`，常量身份是
`namespace + const name`，记录身份仍是 `TypeId + record key`。namespace 重命名属于 schema
身份变更，并影响生成代码和 RPC 协议身份。

记录引用沿用同一分隔符：

```cfd
&sword
&game::items::Item::sword
&game::items::Item::sword.price
```

带完整路径时最后一个 `::` 后是 record key，之前是类型路径。短写 `&sword` 仍从目标字段类型
推断 TypeId。record key 由目标类型限定，不受 CFD 文件自身 namespace 改变。

### 3.2 注释与分隔符

注释使用 `#`：

```cft
# 整行注释
type Item { name: string; } # 行尾注释
```

- CFT 字段和声明使用 `;` 分隔。
- CFD 字段、数组元素和字典条目使用 `,` 分隔，允许尾逗号。
- 函数体内表达式使用 `;` 分隔；块末尾无分号表达式是块结果。
- `//` 是整数除法，不是注释。

### 3.3 标识符

标识符遵循 Unicode XID 规则。以下类别属于保留名称：

- 声明与控制流：`namespace`、`use`、`as`、`const`、`enum`、`type`、`abstract`、`sealed`、
  `check`、`in`、`is`、`fn`、`var`、`return`、`if`、`else`、
  `match`、`for`、`while`、`break`、`continue`。
- 字面量与构造器：`true`、`false`、`None`、`Some`、`Ok`、`Err`。
- 内建类型：`int`、`float`、`bool`、`string`、`Option`、`Result`。
- check 内建名称：`alert`、`records`。
- 编译期元数据名称：`$id`、`$path`、`$type`、`$field`、`$function`。
- `_`。

`let`、`null`、`async`、`spawn` 和 `task` 不属于目标语言语法。

### 3.4 编译期元数据

以 `$` 开头的名称读取当前静态上下文的元数据：

| 写法 | 值 |
| --- | --- |
| `$id` | 当前顶层记录的 key，例如 `"sword"` |
| `$path` | 当前记录的完整路径，例如 `"game::items::Item::sword"` |
| `$type` | 当前记录具体类型的完整路径，例如 `"game::items::Item"` |
| `$field` | 当前配置位置所属的 CFT 字段名 |
| `$function` | 当前持久函数所属的 CFT 函数字段名 |

`$id`、`$path` 和 `$type` 也可用于记录引用，例如 `item.$id` 和 `item.$path`。`$field` 与
`$function` 只读取词法上下文，不是对象成员。它们都是编译期字符串常量，不经过运行时反射，
不能声明、赋值或遮蔽；缺少对应上下文时使用会产生编译错误。局部匿名函数没有独立的
`$function` 名称，函数体中的 `$function` 仍指向包含它的最近持久函数。普通字段可以命名为
`id`，并通过通常的字段访问语法读取，不与 `$id` 冲突。

## 4. CFT / CFD 类型系统

### 4.1 类型一览

| 写法 | 含义 |
| --- | --- |
| `int` | 64 位有符号整数 |
| `float` | 64 位浮点数 |
| `bool` | 布尔值 |
| `string` | Unicode 字符串 |
| `EnumName` | 枚举类型 |
| `TypeName` | 内联 object 类型 |
| `&TypeName` | 顶层记录引用 |
| `[T]` | 只读数组 |
| `{K: V}` | 保持构造顺序的只读字典 |
| `Option<T>` | 明确的可选值 |
| `Result<T, E>` | 明确的成功或业务错误 |
| `fn(A, B) -> R` | 固定参数函数 |
| `()` | 没有有用结果的 unit 值 |

字典 key 只允许 `string`、`int` 或 enum。函数不能比较、排序、哈希或作为字典 key。
当前版本没有协程、任务或其他可暂停值类型。

目标版本移除 `T?` 和 `null`，统一使用 `Option<T>`、`Some(value)` 和 `None`。

### 4.2 Object type

```cft
sealed type Stats {
  hp: int;
  attack: int;
}

type Weapon {
  name: string;
  damage: int;
  cooldown: float = 1.0;
  tags: [string] = ["weapon", "tradable"];
  stats: Stats = { hp: 0, attack: 10 };
  backup: Option<&Weapon> = None;
}
```

- 无默认值字段必须由数据源提供。
- 默认值使用与 CFD 字段相同的 schema-guided 静态值语法，包括标量、插值字符串、enum、
  flag 表达式、Option、内联 object、数组、字典、记录引用和常量引用。
- 默认值在每个 record 的上下文中求值，可以通过字符串插值读取该 record 的其他字段；
  默认值、插值、常量和记录引用共同建立依赖图，依赖环报错。
- 默认值仍是 generation 构建时确定的只读配置值，不执行普通算术、控制流或宿主调用。
- 普通函数实现只能由 CFD body 提供；`@Host @singleton` 函数由 `LoadAndCompile` 后绑定的 C#
  delegate 提供。函数不作为字段默认值。
- `Option<T>` 的非空默认值显式写 `Some(value)`。
- 函数类型字段没有 CFT 函数体；普通类型必须由 CFD body 提供实现，只有 `@Host @singleton` 的
  函数字段由宿主绑定。
- 上一条“必须”在 `LoadAndCompile` 的函数阶段检查，缺失产生 `COFLOW-FUNCTION-MISSING`；`LoadData`
  无条件跳过函数检查，因此允许数据中暂时没有普通函数 body，但该 module 不能调用或绑定函数。

### 4.3 继承与多态

```cft
abstract type Reward {
  source: string = "drop";
}

sealed type ItemReward : Reward {
  item: &Item;
  count: int = 1;
}

sealed type CurrencyReward : Reward {
  amount: int;
}
```

- 每个 object type 最多有一个父类型。
- `abstract` type 不能直接实例化。
- `sealed` type 不能继续派生。
- 子类型继承父类型字段，不能重复声明同名字段。
- 子类型实例可赋给父类型位置。
- `sealed` 层级可用于穷尽 `match`。

### 4.4 记录引用

`&Type` 表示对顶层记录的只读引用，普通 `Type` 表示内联 object：

```cft
type Drop {
  item: &Item;
  backup: Option<&Item> = None;
  reward: Reward;
  rewardPool: [&Reward] = [];
}
```

顶层 record key 不在 CFT 中声明。当前记录通过 `$id` 读取 key，通过 `$path` 读取完整路径；
它们是编译期元数据，不是隐式记录字段。

### 4.5 枚举

```cft
enum Rarity {
  Common,
  Rare = 10,
  Epic,
}
```

- 未指定值的首个变体从 `0` 开始，后续在前值上递增。
- 同一枚举禁止重复整数值。
- enum 与 `int` 不隐式转换。
- 表达式中写 `Rarity::Rare`；`EnumName(integer)` 可构造无具名变体的值。

Flag enum：

```cft
@flag
enum Permission {
  Read = 1,
  Write = 2,
  Execute = 4,
}
```

除 `0` 外，每个具名 flag 值必须是 2 的幂。支持 `&`、`|`、`^`、`~`，结果仍为同一 enum。

### 4.6 函数类型与类型别名

函数类型参数名可选：

```cft
type Predicate = fn(value: int) -> bool;
type RuleFactory = fn(seed: int) -> fn(value: int) -> bool;

type Rules {
  predicate: Predicate;
  handlers: [fn(event: Event) -> Result<Action, RuleError>];
  factories: {string: RuleFactory};
}
```

- `->` 右结合：`fn(int) -> fn(int) -> bool` 等价于
  `fn(int) -> (fn(int) -> bool)`。
- 函数类型是结构类型。
- 参数名只用于文档、生成代码和工具展示，不参与函数类型相等性、逆变/协变或调用 ABI；省略名称
  时 codegen 才使用目标语言的占位参数名。
- 参数类型逆变，返回类型协变。
- 参数数量必须完全匹配。
- 不支持重载、默认参数、具名参数、可变参数、自动柯里化或自动部分应用。
- 用户函数不定义泛型；内建集合方法可由编译器提供泛型能力。
- 函数返回值可以直接或递归包含函数。

### 4.7 注解

注解写在 `type`、`enum`、字段或枚举变体之前：

| 注解 | 目标 | 语义 |
| --- | --- | --- |
| `@flag` | enum | 位标志枚举 |
| `@struct` | sealed type | C# codegen 生成值类型 |
| `@expand` | object 字段 | 表格或结构化编辑视图展开内联 object |
| `@idAsEnum(EnumName)` | type | 使用指定 enum 表示 record key |
| `@localized` / `@localized("bucket")` | 字段 | 字段值随语言维度变化 |
| `@dimension("name")` | 字段 | 字段值随指定维度变化 |
| `@singleton` | type | 数据集中只允许一条记录 |
| `@Host` | singleton type | 记录由 C# 宿主以生成的强类型 API 注入，CFD 不得定义 |
| `@label("text")` | type / enum / 字段 / enum variant | 面向人的展示名称 |
| `@description("text")` | type / enum / 字段 / enum variant | 面向人的详细说明 |

注解约束：

- 表中列出的是编译器理解并执行语义校验的内建注解，不是注解名称的封闭集合。用户可以在
  `type`、`enum`、字段和 enum variant 上声明自定义注解；自定义注解允许名称、string、int、
  float 和 bool 参数，并由 codegen 原样保留到生成 metadata。
- 同一目标上的同名注解不能重复。编译器只对内建注解执行目标和参数约束；自定义注解不产生
  `unknown annotation` 错误，也不被静默丢弃。
- `@singleton` 不能用于 abstract type，也不能作为普通 object 字段或记录引用目标。
- `@struct` 只能用于 `sealed type`。
- `@idAsEnum` 的参数必须是项目中已声明的空 enum，且不能与 `@singleton` 同时使用。
- `@localized` 和 `@dimension` 互斥；一个字段最多绑定一个维度。
- `@Host` 大小写固定，只能用于同时显式声明了 `@singleton` 的具体 type；一个项目可以声明多个
  `@Host` type，且它们可以位于 namespace 中。
- `@Host` 不隐含 `@singleton`，缺少任一注解都产生 schema 错误。

### 4.8 常量

```cft
const MAX_LEVEL = 100;
const MIN_SPEED: float = 0.1;
const ENABLED: bool = true;
const NAME: string = "hero";
const DEFAULT_TAGS: [string] = ["starter", "tradable"];
const DEFAULT_STATS: Stats = { hp: 100, attack: 20 };
const FALLBACK_ITEM: Option<&Item> = Some(&Item::wooden_sword);
```

常量使用与 CFD 字段相同的 schema-guided 静态值语法，可以保存标量、string、enum、Option、
object、数组、字典和记录引用。无法从值唯一推断类型时必须显式标注类型；空集合、短 enum 名和
schema-guided object 通常需要该标注。

常量可以被 CFT 默认值、check、CFD 字段和函数体引用。常量及其全部子值只读；常量依赖环
报错。函数值不作为顶层常量，以免形成绕过 schema 函数位置的顶层函数。常量不接受注解。

## 5. CFT check

当前扩展不迁移 CFT check，也不改变仓库现有 check 语法、诊断和 Rust 执行语义。check 与本章新增的
CFD 函数编译是两条独立路径：

- CFT check 继续由 Rust 项目工具在构建、检查和编辑流程中执行。
- check 不能调用 CFD 函数或 C# delegate，也不进入 C# 函数表。
- CFT codegen 不生成 check body、check IR 或 check 调用包装。
- C# Runtime 不加载 CFT，也不提供 `Check()`。
- `LoadData` 和 `LoadAndCompile` 都不执行 CFT check。

将 check 迁移到函数 IR 或 C# VM 属于后续独立设计，不属于当前语言与运行时扩展。

## 6. CFD 数据语法

### 6.1 顶层记录

单条记录：

```cfd
sword_01: Item {
  name: "Iron Sword",
  price: 100,
}
```

同类型分组：

```cfd
Item {
  sword_fire {
    name: "Fire Sword",
    price: 100,
  }

  staff_ice {
    name: "Ice Staff",
    price: 150,
  }
}
```

分组记录之间的逗号可选。abstract 分组中的记录必须写具体子类型：

```cfd
Reward {
  sword_reward: ItemReward { item: &sword_fire, count: 1 }
  coin_reward: CurrencyReward { amount: 50 }
}
```

记录字段必须来自目标 CFT type 或其父类型。record key 使用 `$id` 访问，因此不占用普通的
`id`、`Id` 或 `ID` 字段名。

### 6.2 标量与字符串

```cfd
count: 10,
ratio: 1.25,
enabled: true,
name: "Fire Sword",
```

字符串使用双引号，支持 `\"`、`\\`、`\n`、`\r`、`\t`。所有字符串都支持
`{expression}` 插值，无 `f` 前缀：

```cfd
description: "价格 {price}，来源 {&Item::sword_fire.name}",
debugName: "{$path}::{$field}",
template: "使用 {{name}} 表示字面量花括号",
```

- `{field}` 读取当前记录字段。
- `{$id}`、`{$path}` 等读取当前静态上下文的编译期元数据。
- `{&key.field}` 读取可推断类型的其他记录字段。
- `{&Type::key.field}` 显式限定目标记录类型。
- 函数体字符串可插入任意普通表达式。
- `{{`、`}}` 产生字面量花括号。
- `None`、bool、数值、string、enum、记录引用、object、数组和字典使用确定的 CFD 文本表示；
  函数不能直接插值。
- 插值结果仍是普通 `string`，不增加第二种字符串类型。

### 6.3 Option 值

CFD 在有明确 CFT 目标类型时自动把内部值包装成 `Some`：

```cft
type Drop {
  backup: Option<&Item> = None;
  level: Option<int> = None;
}
```

```cfd
# 有值
drop_a: Drop {
  backup: &sword_fire, # Some(&sword_fire)
  level: 10,           # Some(10)
}

# 无值
drop_b: Drop {
  backup: None,
  level: None,
}
```

函数体和其他普通表达式中创建 Option 时必须显式写 `Some(value)` 或 `None`。字段只有在 CFT
声明默认值 `= None` 时才能省略，不为所有 `Option<T>` 隐式添加默认值。

### 6.4 enum 与 flag

普通 CFD 字段在目标 enum 明确时可写短变体名或完整名称：

```cfd
rarity: Rare,
rarity: Rarity::Rare,
```

函数体缺少 schema-guided 字段上下文，必须写完整名称。Flag enum 还支持整数 mask、括号和
`&`、`^`、`|` 表达式：

```cfd
permissions: Read | Write & (Execute | Admin),
permissions: 5,
```

### 6.5 数组、对象与字典

```cfd
tags: ["weapon", "melee"],
stats: { hp: 100, attack: 20 },
weights: { Fire: 10, Ice: 5 },
rewards: [
  ItemReward { item: &sword_fire, count: 1 },
  CurrencyReward { amount: 10 },
],
```

- 配置字段中的 `{ ... }` 由 CFT 目标类型区分内联 object 和字典。
- 多态 object 必须写具体类型名。
- 字典 key 只允许 `string`、`int` 或 enum，重复 key 报错。
- 字典严格保持构造顺序。
- 配置对象、数组和字典在运行时只读。

函数体内缺少配置字段上下文时：

```cfd
var stats = Stats { hp: 100, attack: 20 };
var weights: {string: int} = { "fire": 10 };
var values: [int] = [];
```

- object 构造必须写 `Type { ... }`。
- 无类型名的 `{ key: value }` 一律是字典。
- 非空数组可从元素推断；空集合必须显式标注类型。
- 不提供 tuple；多个结果使用已有 CFT object type。

### 6.6 记录引用

```cfd
featuredItem: &sword_fire,
explicitItem: &Item::sword_fire,
```

- `&key` 的目标类型来自 CFT 位置。
- `&Type::key` 显式限定目标类型。
- 目标必须存在，实际类型必须可赋给引用声明类型。
- 裸 key 不会自动成为记录引用。
- 数组和字典递归应用其元素或 value 的引用类型。

### 6.7 普通配置的表达式边界

普通 CFD 字段和集合元素保持结构化值，不接受任意算术、控制流或局部变量表达式。它们可以
引用 CFT 常量；保留的计算型位置只有字符串插值、flag enum 表达式、函数实现和允许的函数引用。

这一边界保证配置值保持结构化。编译器仍可在每个 generation 内把只读配置字段内联为函数常量。

## 7. 函数值与实现

### 7.1 CFT 声明与 CFD 实现

```cft
type PricingRules {
  rate: float;
  addTax: fn(price: int) -> int;
  apply: fn(value: int, operation: fn(input: int) -> int) -> int;
}
```

```cfd
pricing: PricingRules {
  rate: 0.1,

  addTax: fn(price: int) -> int {
    int(float(price) * (1.0 + rate))
  },

  apply: fn(value: int, operation: fn(input: int) -> int) -> int {
    operation(value)
  },
}
```

- 函数值写作 `fn(name: Type, ...) -> ReturnType { ... }`，`fn` 与 body 之间没有 `=`。
- 实现中的参数和返回类型必须完整书写，不能依赖 CFT 位置推断。
- CFD 没有顶层独立函数声明；持久函数值只能出现在 CFT 声明的函数类型位置。
- 函数字段、函数数组元素、函数字典 value 都是普通配置值。
- C# Runtime 的 `LoadData` 识别函数 body 边界并无条件跳过函数检查，不执行函数名称解析、类型检查
  或字节码生成，也不提供切换参数；
  该 generation 的普通数据可读取，但函数不可调用或绑定。
- C# Runtime 的 `LoadAndCompile` 对全部函数 body 完成项目级链接、类型检查和字节码生成；只有它
  返回的 generation 可以调用函数；C# delegate 绑定只属于 `@Host @singleton`。

### 7.2 所属配置实例与字段名称

函数的配置字段由“CFT 中声明该函数位置所属的配置实例”确定，而不是最近的 `{}`。函数在
该实例的数组或字典字段中时，所属实例不变。

函数体直接使用所属实例的字段，不使用 `self`：

```cfd
greaterThanMinimum: fn(value: int) -> bool {
  value > minimum
}
```

- 参数和局部变量不能与所属 CFT type 的字段同名。
- 持久函数字段中的 `$function` 是该 CFT 字段名，`$path` 是所属顶层记录的完整路径。
- 取出或传递函数后，其所属记录实例保持不变。
- 所属实例由静态记录 ID 与 generation 表示，不是可变对象接收者。

### 7.3 函数引用和调用

同一记录中的函数字段直接使用裸字段名：

```cfd
fallback: primary,

run: fn(value: int) -> int {
  var operation = fallback;
  operation(value)
}
```

跨记录函数调用使用记录引用语法：

```cfd
&pricing_rules.calculate(price)

var calculate = &PricingRules::pricing_rules.calculate;
calculate(price)
```

函数字段允许前向引用。普通字段依赖环报错；函数调用环按递归处理，不属于配置初始化环。

### 7.4 递归

持久函数字段通过自己的字段名直接递归，不增加专用关键字：

```cfd
factorial: fn(value: int) -> int {
  if value <= 1 {
    1
  } else {
    value * factorial(value - 1)
  }
}
```

相互递归通过同记录字段或跨记录函数引用完成。局部匿名函数以及数组、字典中的匿名函数没有
可引用的字段名，因此不能直接自递归；需要递归时应把实现放在具名函数字段中再传递该函数值。
编译器对尾递归执行尾调用消除；运行期执行预算与最大工作量由宿主策略控制，不写入语言语义。

### 7.5 匿名函数与闭包

函数体内允许匿名函数：

```cfd
filterAbove: fn(values: [int], minimum: int) -> [int] {
  values.filter(fn(value: int) -> bool {
    value > minimum
  })
}
```

- 捕获在匿名函数创建时按值进行。
- 匿名函数不能给外层绑定赋值。
- 外层 `var` 后续重新赋值不影响已经创建的闭包。
- 闭包可传递、返回、放入局部集合或另一个返回值。
- 闭包自身仍是只读函数值。

编译器优先执行内联、lambda lifting 和逃逸分析。无法消除的运行时捕获转换为类似 Rust/C++
匿名结构体的只读数据；不同动态 callable 可去函数化为有限标签与捕获字段。跨宿主或动态 Mod
边界使用带签名的函数表，不要求 VM 实现可变 upvalue 或通用闭包 GC。

## 8. 函数体表达式

### 8.1 Block、分号与返回值

```cfd
fn(value: int) -> int {
  var result = value * 2;
  result += 1;
  result
}
```

- block 最后一个无分号表达式是结果。
- 带分号表达式的结果被丢弃并视为 `()`。
- `return expression;` 可提前返回。
- `var` 声明和赋值表达式的结果都是 `()`。
- 参数不可重新赋值。

### 8.2 var 与作用域

```cfd
var total = 0;
var scale: float = 1.0;
total += 10;
```

- `var` 必须初始化，可推断或显式标注类型。
- 支持 `=`, `+=`, `-=`, `*=`, `/=`。
- 不提供 `++`、`--`。
- 只有局部 `var` 绑定可重新赋值；字段、对象、集合元素和捕获变量只读。
- 同一作用域禁止重复定义，内层块允许遮蔽外层局部变量。

### 8.3 if

```cfd
var result = if value >= 0 {
  value
} else {
  0
};
```

- 条件必须是 `bool`，没有 Lua 式 truthiness。
- `if` 作为值时必须有 `else`，各分支结果类型兼容。
- 只用于控制流且结果为 `()` 时可以省略 `else`。

### 8.4 match

```cfd
match result {
  Ok(value) => value,
  Err(error) => handle(error),
}
```

支持的简单模式：

- 标量字面量和完整 enum 变体。
- `Some(value)`、`None`、`Ok(value)`、`Err(error)`。
- 类型 pattern：`ItemReward item`。
- 绑定和 `_`。

`=>` 后可接单个表达式或 block。match 必须穷尽；sealed 层级可由全部具体子类型穷尽，
开放层级必须包含 `_`。不增加 object/list 的复杂结构解构。

`if value is Type` 在对应分支中自动收窄，不提供可能失败的 `as` 强制转换。

### 8.5 循环

```cfd
var total = 0;

for value in values {
  total += value;
}

for value, index in values {
  use(index, value);
}

for key, value in weights {
  use(key, value);
}

while total < limit {
  total += 1;
}
```

- 支持 `for` 和 `while`，不增加无限 `loop`。
- 数组双 binding 为 `item, index`，字典双 binding 为 `key, value`。
- 循环 binding 不可重新赋值。
- 支持普通 `break;`、`continue;`，不支持标签或携带值的 break。
- 循环结果是 `()`。
- 数值范围写 `start..end` 或 `start..=end`。

### 8.6 Option、Result 与传播

```cfd
findPrice: fn(items: {string: int}, key: string) -> Option<int> {
  items[key]
}

load: fn(key: string) -> Result<Item, LoadError> {
  var item = findItem(key)?;
  Ok(item)
}
```

- 数组和字典下标返回 `Option<T>`，越界或缺 key 不产生隐式 null。
- `?` 可传播 `Option` 或 `Result`。
- `Option` 只能传播给返回 Option 的路径。
- `Result<T, E>` 只能自动传播相同的 `E`，不隐式转换错误类型。
- 不提供 `throw`、`try/catch`、`?.`、`?[...]` 或 `??`。
- 普通整数溢出、除零和 VM 不变量破坏属于带源码位置的运行时故障，不伪装成业务 Result。

### 8.7 运算符

函数体使用以下运算符：

| 类别 | 运算符 | 主要类型规则 |
| --- | --- | --- |
| 后缀 | `.field`、`[index]`、`(args)`、`?` | 字段、Option 下标、调用、传播 |
| 一元 | `!`、`~`、`-` | bool、整数/flag、数值 |
| 幂 | `**`，右结合 | 两侧同为 int 或同为 float |
| 乘除 | `*`、`/`、`//`、`%` | `//`、`%` 仅 int，其余两侧数值类型相同 |
| 加减移位 | `+`、`-`、`<<`、`>>` | 数值；`+` 还支持 string；移位仅 int |
| 位运算 | `&`、`^`、`|` | int 或同一 flag enum |
| 比较 | `==`、`!=`、`<`、`<=`、`>`、`>=` | 相同/兼容类型；顺序比较限标量 |
| 类型判断 | `is` | object 的运行时具体类型 |
| 逻辑 | `&&`、`||`，短路 | bool |
| 赋值 | `=`、`+=`、`-=`、`*=`、`/=` | 仅局部 var，结果为 unit |

连续比较有效：`0 <= value <= 100`。函数参数严格从左到右求值。`int(value)`、
`float(value)` 显式转换数值，不进行 `int` 到 `float` 的隐式提升。字符串 `+` 只接受两个
string，不隐式格式化数字。

### 8.8 内建方法

内建方法：

| 方法 | receiver | 返回值 |
| --- | --- | --- |
| `.len()` | string / array / dict | Unicode 字符数或元素数 int |
| `.contains(x)` | string / array / dict | bool |
| `.isUnique()` | 可比较标量数组 | bool |
| `.min()` / `.max()` | int / float / enum 数组 | 元素类型；空数组运行时故障 |
| `.sum()` | int / float 数组 | 元素类型 |
| `.keys()` / `.values()` | dict | key/value 数组，保持字典顺序 |
| `.matches("pattern")` | string | bool |
| `.startsWith(x)` / `.endsWith(x)` | string | bool |
| `.isBlank()` | string | bool |
| `.abs()` | int / float | 原数值类型 |
| `.sqrt()` | float | float |
| `.isFinite()` | float | bool |
| `.approxEqual(other, epsilon)` | float | bool |
| `.containsKey(k)` / `.containsValue(v)` | dict | bool |
| `.isSorted()` / `.isStrictlySorted()` | 可排序标量数组 | bool |
| `.intersects(x)` / `.isDisjoint(x)` | 可比较标量数组 | bool |
| `.isSubsetOf(x)` / `.isSupersetOf(x)` | 可比较标量数组 | bool |

`matches` 的 pattern 必须是字符串字面量并使用 Rust regex 语法；默认执行子串匹配。
`approxEqual` 的 epsilon 必须有限且非负。`.sqrt()` 使用 IEEE-754 语义；对 `int` 最小值执行
`.abs()` 是运行时故障。

函数体增加高阶只读集合方法：

```cfd
values.map(fn(value: int) -> int { value * 2 })
values.filter(predicate)
values.fold(0, fn(total: int, value: int) -> int { total + value })
values.find(predicate)
values.any(predicate)
values.all(predicate)
```

这些方法不修改 receiver。匿名回调通过闭包按值捕获局部值；编译器可把已知调用直接融合为循环。
不增加集合推导式或 `collect/yield`。

概念签名如下：

```cft
[T].map(fn(T) -> U) -> [U]
[T].filter(fn(T) -> bool) -> [T]
[T].fold(A, fn(A, T) -> A) -> A
[T].find(fn(T) -> bool) -> Option<T>
[T].any(fn(T) -> bool) -> bool
[T].all(fn(T) -> bool) -> bool
```

这些泛型签名属于编译器内建描述，不是用户可声明的 CFT 泛型语法。

## 9. C# 宿主绑定

C# Runtime 是固定库，不是 codegen 产物。顶层加载入口固定如下，参数都是 CFD 文本内容而不是
路径：

```csharp
public static class Coflow
{
    public static CoflowModule LoadData(string cfd);
    public static CoflowModule LoadData(string cfd, params CoflowModule[] children);
    public static CoflowModule LoadData(string[] cfdSources);
    public static CoflowModule LoadData(string[] cfdSources, params CoflowModule[] children);
    public static CoflowModule LoadAndCompile(string cfd);
    public static CoflowModule LoadAndCompile(string cfd, params CoflowModule[] children);
    public static CoflowModule LoadAndCompile(string[] cfdSources);
    public static CoflowModule LoadAndCompile(string[] cfdSources, params CoflowModule[] children);
    public static CoflowModule Combine(params CoflowModule[] modules);
}
```

`LoadData` 只构建可读取普通数据的 generation，无条件跳过全部函数检查和编译，也不能绑定或调用
函数；该行为没有开关参数。`LoadAndCompile` 在加载数据后检查并编译全部 CFD 函数；只有其返回的
generation 可以调用函数和绑定 `@Host`。两个入口都不加载 CFT、artifact 或 CFT hash。

数据访问也由固定 Runtime 泛型提供，不生成任何项目级 Runtime 入口或数据根类型：

```csharp
public sealed class CoflowModule : IDisposable
{
    public bool FunctionsCompiled { get; }
    public CoflowTable<T> Table<T>();
    public T Singleton<T>();
    public CoflowModule Compile();
    public Result<ReloadInfo, CoflowReloadError> Reload(string cfd);
    public Result<ReloadInfo, CoflowReloadError> Reload(string[] cfdSources);
    public Result<ReloadInfo, CoflowReloadError> Reload(CoflowModule child, string cfd);
    public Result<ReloadInfo, CoflowReloadError> Reload(CoflowModule child, string[] cfdSources);
}

public sealed class CoflowTable<T> : IReadOnlyList<T>
{
    // 仅当 T 未声明 @idAsEnum 时有效
    public Option<T> Get(string key);

    // 仅当 T 声明 @idAsEnum(TKey) 且参数正是该 TKey 时有效
    public Option<T> Get<TKey>(TKey key) where TKey : struct, Enum;
}
```

这里的 `Item`、`Rule`、`Services` 等业务类型才由 CFT codegen 生成，名称直接来自 CFT 声明；
不增加 `Record`、`Action`、`Model`、`Data` 等后缀。数组和字典使用 Runtime/标准库泛型，
`Option<T>` 与 `Result<T, E>` 使用 Runtime 泛型，不为它们生成逐类型包装类。

### 9.1 `@Host` singleton 注入

完整记录注入只允许 CFT 明确定义的 `@Host @singleton` type：

```cft
@Host
@singleton
type Services {
  environment: string;
  limits: Limits;
  log: fn(message: string) -> ();
  moveTo: fn(entity: EntityId, position: Position) -> Result<(), MoveError>;
}
```

- `@Host` type 是普通 CFT type，字段类型、namespace 和函数签名都由 CFT 决定。
- CFD 不得为 `@Host` type 声明记录或函数 body；其唯一 singleton 由 C# 应用通过生成类型完整注入。
- 一个项目可以声明多个 `@Host @singleton` type，不存在名称固定为 `Host` 的特殊类型。
- 注入不使用 type name、field name、function name 或 record key 字符串。
- 生成类型直接提供与全部字段对应的强类型 `Bind(...)`；它表示完整记录注入，普通字段和函数都必须
  一次性提供，CFT 默认值不形成隐式宿主读取或省略参数的重载。
- `LoadData` 的 generation 不允许绑定 `@Host`；只有 `LoadAndCompile` 的 generation 可以绑定。
- 每个 `@Host` singleton 在一个 generation 中最多成功绑定一次；重复绑定产生绑定错误。
- `@Host` singleton 绑定后通过普通 singleton 字段和方法语义访问。

可恢复业务失败必须通过 CFT 声明的 `Result<T, E>` 返回。C# delegate 抛出的未处理异常、返回值
不符合签名或其他 ABI 违约都转换为带 Coflow 调用栈的 VM fault。

### 9.2 普通函数字段调用

普通记录的函数字段只能由 CFD 提供实现。CFT codegen 只生成强类型调用方法，不生成 `BindXxx`；
应用通过 `CoflowModule.Table<T>()` 和强类型 key 取得记录后直接调用：

```csharp
Option<Rule> found = data.Table<Rule>().Get("combat");
if (found.TryGetValue(out Rule rule)) {
  Result<long, RuleError> result = rule.Evaluate(42);
}
```

- `CoflowTable<T>.Get(...)` 返回 `Option<T>`；key 存在时返回 `Some(record)`，不存在时返回
  `None`。缺失记录不是加载错误或 VM fault，也不用 `default(T)` 表示。
- `Get` 是唯一按 key 查找入口，不提供 `TryGet`、`GetRequired`、按 key 索引器、nullable 返回值、
  record-not-found 异常或带默认值的替代版本。
- `IReadOnlyList<T>` 的索引器仅按 generation 中的记录顺序和整数位置访问，不接受 record key，
  也不是另一种 key 查找入口。
- 普通 type 的 `Get` 只接受 `string` key；声明 `@idAsEnum(KeyEnum)` 的 type 只接受生成的
  `KeyEnum`。两者不互相转换，也不接受 enum 名称字符串作为替代输入。
- C# 无法仅通过 `CoflowTable<T>` 的 `T` 静态表达其关联 enum key，因此 enum 重载先由泛型约束
  保证参数为 enum，再由 Runtime 根据 `T` 的生成 metadata 验证它正是 `@idAsEnum` 指定的类型；
  类型不匹配立即报 API 使用错误，不按名称或底层整数转换。
- `Option<T>.TryGetValue(...)` 只负责解包 `Get` 已经返回的 Option，不是 table 的第二种查找入口。
- 不公开动态函数 descriptor 或字符串函数路径。
- CFT 函数签名中的可选参数名保留到生成方法；未命名参数才生成 `arg0`、`arg1`。参数名不参与
  函数类型兼容性或 VM ABI。
- 函数数组元素和函数字典 value 不提供按 index 或 key 的绑定 API，必须由 CFD body 提供。

VM 的函数调用语义不区分 CFD body 与 C# delegate。函数表内部可以保存不同实现入口，但直接调用、
间接调用、参数布局、返回布局和 fault 语义完全一致。

### 9.3 Reload

每个 `CoflowModule` 保存一个或多个带稳定内部 identity 的 CFD source part。模块操作遵循以下规则：

- `Combine(a, b, ...)` 合并 source part 并重新链接数据；同一个 part 不能重复加入，不同 generated
  contract 的模块不能组合。
- 全部输入模块均已编译时，Combine 重新编译合并后的函数图；否则产生 data-only module，可以再用
  `Compile()` 生成新的已编译模块。
- `LoadData(sources, children)` 与 `LoadAndCompile(sources, children)` 可以在加载父 CFD 的同时组合
  已有子模块，使父记录可以在第一次加载时引用子记录。
- `Compile()` 和 `Combine()` 返回新模块，不修改输入模块。

reload 使用新的 CFD 字符串构建候选 generation，不加载 CFT，也不比较 CFT hash。运行中的生成类型
就是固定 schema 契约，因此 reload 只允许数据和 CFD 函数实现发生变化。`root.Reload(child, sources)`
按稳定 identity 替换组合根中的 child source part，并自动重建记录引用及重新链接、编译根函数图；
父模块的 CFD source 本身不变，调用方也不需要额外调用“重新编译父模块”的 API。

`module.Reload(sources)` 替换该 module 的完整 CFD 内容；定向的 `root.Reload(child, sources)` 只替换
组合根中归属于 child 的内容。child 本身可以是由多个 source part 组成的组合模块，并可连续替换。

Runtime 不建立 child 指向所有组合根的反向订阅。一个 child 可以参与多个互不相关的根；直接调用
child 自身的 Reload 只更新 child，不会隐式修改其他根。需要替换某个根中的 child 时，必须在该根
上调用定向 Reload，这避免反向强引用、级联更新歧义和模块生命周期泄漏。

成功 reload 时，Runtime 复用已有 `@Host` singleton 强类型绑定。绑定目标无法满足生成签名时，
reload 失败并继续保留旧 generation。活动调用和外部 view 在结束前固定持有启动时的 generation。

Host Bind 与 Reload 使用 generation gate 串行化。先完成的 Bind 被迁移到候选 generation；先完成
发布的 Reload 将旧 generation 标记为 retired，旧对象上尚未完成的 Bind 返回
`HostBindError.GenerationRetired`。候选 generation 只有在加载、编译、Host 状态迁移全部成功后才
原子发布。

## 10. C# Runtime generation 与记录映像

### 10.1 Generation

C# Runtime 直接从 CFD 字符串构建 generation，不生成、保存或加载数据 artifact：

```text
Generation
  GeneratedTypeMetadata
  RecordTables
  DataHeap
  FunctionTable?     # 仅 LoadAndCompile
  Bytecode?          # 仅 LoadAndCompile
```

`LoadData` 和 `LoadAndCompile` 都解析 CFD、按生成 metadata 加载普通字段、解析默认值和记录引用并
构建 `RecordTables` 与 `DataHeap`。`LoadData` 到此结束；`LoadAndCompile` 继续生成函数表和字节码。
普通 CFD 数据和字节码在发布后不可变；`LoadAndCompile` 只为 `@Host` singleton 预留 bind-once
slot。后续绑定只填充这些 slot，不改写普通记录或已编译字节码。

`TypeId`、`FieldId`、`RecordId`、`LayoutId` 和 `FunctionId` 等紧凑索引只在一个 generation 内稳定。
生成 C# 类型不保存这些索引；每个新 generation 都从生成 metadata 重新解析布局和函数位置。

### 10.2 紧凑记录表

每个具体 record type 可以使用固定 row width 的连续 slot 表。`RecordId` 定位记录表和
row index；`FieldDescriptor` 保存声明类型、值 layout、slot offset 和 width。编译器在发布
generation 前完成默认值、引用、继承字段和静态集合的解析与编码，VM 不在字段首次访问时递归
物化记录值。

```text
RecordTable<Item>
  row 0: [field slots...]
  row 1: [field slots...]
  row 2: [field slots...]
```

标量、enum、`RecordId`、`FunctionId` 和数据 handle 可以占一个 slot。`Option<T>` 使用 tag 加 payload
window；`Result<T, E>` 使用 tag 加共享 payload window。字符串、数组、字典和较大的内联 object
存放在 generation 只读 `DataHeap`，记录 row 只保存 generation-relative handle，不保存进程裸指针。

具体派生类型在编译期展开为 `base fields + derived fields`。同一 generation 内，基类字段在派生
记录中保持兼容布局；Runtime 内部 reader 通过 `FieldId` 取得当前 generation 的 descriptor，不跨
generation 假定 offset 不变。

### 10.3 Record、Object 与字段访问

Record 是 generation 中带稳定 `RecordId` 的只读配置行；Object 是执行期构造、存放在 execution
arena 中的值。两者可以共享 layout width 和字段 window 规则，但不能共享生命周期或身份。

记录字段指令直接使用编译期解析的字段 window。单 slot 热路径执行 row base 加 field offset 后的
一次读取；宽值复制连续 slot window。VM 不保留 execution-local 的记录字段物化 HashMap。

生成类型的内部 reader 使用已知字段 metadata 构造强类型 C# 对象。应用侧只通过
`CoflowModule.Table<T>()`、`CoflowModule.Singleton<T>()` 和生成属性访问，不公开按字符串读取 type、
field 或 function 的 API。普通 record key 仍是语言数据，可以是 `string`；`@idAsEnum` type 的 key
使用生成 enum。

`CoflowTable<T>.Get(...)` 是唯一的按 key 记录查找入口，结果固定为 Runtime `Option<T>`：命中时
返回 `Some(record)`，未命中时返回 `None`。它不返回 nullable，不因记录缺失抛异常，也没有
`TryGet`、`GetRequired`、按 key 索引器、带默认值或 fallback 的替代入口，也不用 `default(T)`
表示缺失。普通 key 为 `string`；声明 `@idAsEnum` 后只接受指定的生成 enum，两种 key 不互相转换，
也不接受 enum 名称字符串作为替代输入。`IReadOnlyList<T>` 提供的整数索引器只表达记录顺序，不是
按 key 查找，也不具有缺失 key 的语义。

## 11. 编译器与 VM 核心设计

### 11.1 编译管线

```text
CFT source --Rust codegen--> generated C# types and internal metadata

CFD strings + generated metadata
  -> C# 词法、语法与结构化数据加载
  -> 项目链接、函数名称解析和类型检查
  -> Typed HIR
  -> generation-local 字节码生成
  -> CompiledModule
  -> C# VM 执行
```

C# Runtime 不读取 CFT。生成代码携带加载 CFD 所需的类型、字段、继承、默认值、函数签名和注解
metadata。C# 前端只处理 CFD 源语言结构；类型化 HIR 不包含寄存器和字节码布局；VM 只执行
`LoadAndCompile` 的编译输出，不重新承担源语言类型检查。

`LoadData` 仍必须正确识别函数 body 的边界并报告破坏整个 CFD 结构的词法或分隔符错误，但无条件
跳过函数 body 内部表达式解析、名称解析、类型检查、控制流检查和字节码生成；不提供切换参数。

### 11.2 内部字节码

字节码只是同一个 C# Runtime 内 compiler 到 VM 的 generation-local 内部表示，不序列化、不加载、
不提供 artifact 版本兼容协议，也不属于生成 C# 代码。当前阶段优先保证语言语义和诊断正确，不把
指令位宽、寄存器数量、常量池表示、跳转编码、SSA、寄存器分配或优化管线固定为兼容契约。

正式 C# VM 可以使用结构化指令、常量池、值栈、局部变量槽和显式 frame。每条已发射指令都关联
CFD source span；算术、内建方法、索引、直接/间接调用和 C# delegate 边界产生的 fault 使用当前
指令位置。后续可以在保持函数语义、生成 metadata 和公共 API 不变的前提下替换为更紧凑的编码，
但不能要求加载旧字节码，因为当前版本不产生或加载 artifact。

### 11.3 统一函数表与 C# delegate

所有可调用代码使用同一套 `FunctionId`、`SignatureId` 和参数/返回窗口 ABI。函数表包含 CFD
持久函数、匿名函数、编译器生成函数和 C# delegate 入口，不包含 CFT check。函数值不携带“CFD”或
“宿主”语言类型；无捕获函数直接保存 `FunctionId`，仍需运行时捕获的闭包保存 `FunctionId` 加
environment handle。

已知目标使用 `call.direct`，函数值使用 `call.indirect`；二者都解析统一函数表条目。条目内部决定
执行 CFD bytecode 还是调用已绑定 C# delegate，但调用指令、参数窗口、返回窗口和 fault 处理不
分叉。函数签名来自生成 metadata，不在每条指令中重复编码完整类型。

编译器在发射函数表前完成捕获分析；匿名函数按值捕获所需局部值。常量捕获折叠、lambda lifting、
逃逸分析、内联和高阶集合融合属于后续性能优化，不是当前版本的语义前置条件。

### 11.4 栈帧与执行值

VM 使用自己管理的显式 frame，不使用系统线程栈表达 Coflow 调用栈。每个最外层同步调用拥有一份：

- frame、参数、局部变量和值栈；
- string、object、array、dict 和 closure 执行值；
- 指令、调用深度和值栈预算；
- generation lease 和简洁诊断信息。

VM → Host → VM 的同步重入继续使用同一份执行上下文、调用链和预算，不创建新的“最外层调用”；
因此 Host 反复调用收到的 VM 闭包不能重置预算。默认指令预算为 10,000,000，frame 上限为 4,096，
值栈上限为 1,000,000。

当前 VM 只执行到正常返回或 fault，不保存可恢复机器状态，也不拥有 completion queue 或后台
调度器。值栈使用清零后归还的池化数组；高阶数组操作直接遍历原始 `IReadOnlyList<T>`，不先复制
为 `object[]`，线性集合内建操作的工作量计入同一指令预算。

### 11.5 Generation 与热重载

每次成功加载产生一个 generation。`LoadData` generation 包含生成 metadata 对应的记录表和只读
数据堆；`LoadAndCompile` generation 还包含函数表、编译模块以及 `@Host` 的一次性绑定位置。
普通数据和字节码发布后不可变。当前版本不计算 schema hash，也不执行 CFT check。reload 的数据
加载、函数编译或已有绑定解析失败时继续保留旧 generation。

活动同步调用在结束前固定持有启动时的 generation，避免执行过程中记录、函数或宿主槽位被
替换。generation 只有在所有调用和外部 lease 释放后才能回收。

模块本身不进入按 CFD 内容或 reload 次数增长的全局缓存。成功发布后，旧 generation 只会被已经
取得的旧记录、闭包或活动调用持有；外部引用释放后即可由 GC 回收。`Dispose()` 清除模块持有的
table、singleton、function、storage 和 source part，并阻止 retired generation 上新的 Host Bind。
失败 reload 的候选 generation 不发布，并立即释放 Runtime 持有的根引用。

### 11.6 诊断与调试边界

- 词法、解析、链接、类型检查和编译错误包含源码位置。
- VM fault 包含故障位置和简短 Coflow 调用栈，不暴露寄存器快照。
- trace 默认关闭，只记录函数进入、返回、C# delegate 调用和 fault。
- profile 默认关闭，只汇总指令工作量、delegate 调用次数和耗时。
- 指令、调用深度和值栈预算属于执行控制，不依赖 trace/profile。

## 12. 明确不采用的设计

- 不采用 `null`、`T?`、`let`、异常语法和隐式数值转换。
- 当前版本不采用协程、`await`、`yield`、暂停中的宿主调用、completion、detached 调用、`Task<T>`、
  `Operation<T>`、`async fn`、`spawn`、`start` 或公开调度器 API；这些能力只能在数据与函数 ABI
  稳定后重新设计。
- 不采用数据或字节码 artifact、字节码加载器、产物兼容协议或不可信字节码验证器。
- 不将变长数据直接内联进固定宽度 record row，不在编译映像中保存进程裸指针，也不允许宿主
  跨 generation 缓存 field offset、record row 地址或函数索引。
- 不让 trace、profile 或调试缓存进入 VM 正常执行所依赖的主路径。
