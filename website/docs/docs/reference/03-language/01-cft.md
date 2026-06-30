# CFT 语法参考

CFT（Coflow Type File，`.cft`）是专为 coflow 设计的 schema 语言，用来声明配置数据的类型、字段、默认值、引用、继承、多态和业务校验规则。

`.cft` 文件只包含 schema 定义，不包含数据，不执行 I/O。Excel、CSV、CFD、飞书/Lark 表格等数据源都会按照编译后的 CFT schema 解析、校验并导出。

下面是一个包含 `enum`、`type` 和 `check` 的简单示例：

```text
enum Rarity {
  Common,
  Rare,
}

type Item {
  name: string;
  rarity: Rarity = Rarity.Common;
  price: int;

  check {
    name != "";
    price >= 0;
  }
}
```

这个示例展示了 CFT 如何约束 `Item` records 的字段、默认值和业务规则。更小的 schema 可以只包含一个 `type`。

## 文件与命名空间

文件与命名空间规则决定一个项目里哪些 `.cft` 文件会被编译，以及这些文件中的名称如何互相引用。

`coflow.yaml` 中的 `schema` 可以指向单个 `.cft` 文件、目录或文件/目录列表。目录会递归发现精确小写 `.cft` 文件。

同一个项目的所有 CFT 文件共同编译到同一个全局命名空间：

- `const`、`enum`、`type` 名称在整个项目中唯一。
- 支持前向引用，不要求先声明后使用。
- 当前没有 `module`、`import` 或 `use` 语句。
- 诊断中看到的 module id 通常是项目相对路径，例如 `schema/item.cft`。

注释使用 `#`：

```text
# 整行注释
type Item { name: string; }  # 行尾注释
```

## 标识符与保留名

标识符用于命名常量、枚举、类型、字段和 check 变量；保留名避免 schema 名称和语言语法发生冲突。

标识符遵循 Unicode XID 规则，可以使用中文等合法标识符字符。

以下名称不能用作 `const`、`enum`、`type`、字段、枚举变体或量词变量名称：

- 关键字和字面量：`const`、`enum`、`type`、`abstract`、`sealed`、`check`、`when`、`all`、`any`、`none`、`in`、`is`、`true`、`false`、`null`
- primitive 类型名：`int`、`float`、`bool`、`string`
- 内建函数名：`len`、`contains`、`isUnique`、`min`、`max`、`sum`、`keys`、`values`、`matches`
- 虚拟 record key 字段：`id`、`Id`、`ID`
- 预留语法名：`if`、`else`、`match`、`case`、`for`、`while`、`let`、`module`、`import`、`export`、`from`、`as`、`use`
- `_`

## 类型

`type` 用来声明一类配置 record 的字段结构。数据源中的 sheet、CSV、CFD record 通常会映射到某个 CFT `type`。

```text
type Weapon {
  name: string;
  damage: int;
  cooldown: float = 1.0;
}
```

字段之间用 `;` 分隔。无默认值字段必须由数据源提供；有默认值字段可以省略。

### 字段类型

字段类型决定数据源中的值如何解析，也决定导出和代码生成时字段如何表达。

| 类型 | 说明 |
| --- | --- |
| `int` | 64 位整数 |
| `float` | 64 位浮点 |
| `bool` | 布尔值 |
| `string` | 字符串 |
| `EnumName` | 枚举类型 |
| `TypeName` | 对象类型 |
| `[T]` | 数组 |
| `{K: V}` | 字典，key 只允许 `string`、`int` 或 enum 类型 |
| `T?` | nullable，可为 `null` |

示例：

```text
type Item {
  name: string;
  rarity: Rarity = Rarity.Common;
  tags: [string] = [];
  attributes: {string: int} = {};
  next_tier: Item? = null;
}
```

### 默认值

默认值用于减少数据源重复填写。字段有默认值时，数据源可以省略该字段。

字段默认值必须是编译期常量：

```text
type Item {
  price: int = 10;
  rarity: Rarity = Rarity.Common;
  tags: [string] = [];
  attributes: {string: int} = {};
  next_tier: Item? = null;
}
```

默认值不能引用其他字段。

### 继承与多态

继承和多态用于表达“字段结构随具体类型变化”的配置。例如奖励可以是物品奖励、货币奖励或经验奖励；用 `abstract type Reward` 表达共同接口，用具体子类表达差异字段。

`abstract` 禁止直接实例化，只能通过子类使用。`sealed` 禁止被继承。

```text
abstract type Reward {
  source: string = "drop";

  check { source != ""; }
}

sealed type ItemReward : Reward {
  item: Item;
  count: int = 1;
}

sealed type CurrencyReward : Reward {
  amount: int;
}
```

规则：

- 每个 `type` 最多一个父类。
- 子类继承父类所有字段。
- 子类不能声明与父类同名的字段。
- 子类实例可以赋值给父类类型字段。
- `abstract type` 字段只能填入其具体子类实例。

## 记录 key 与引用

记录 key 和引用用于表达跨记录关系，例如怪物引用掉落表、掉落奖励引用物品。CFT 声明字段类型，数据源提供具体 record key，build 阶段检查引用目标是否存在且类型兼容。

顶层记录 key 由数据源提供，不在 CFT 字段中声明。`id` 是只读虚拟字段，可在 `check` 中读取当前顶层记录 key。

```text
type Item {
  name: string;

  check {
    id.matches("^[a-z][a-z0-9_]*$");
  }
}
```

对象字段的数据默认既可以是内联对象，也可以是数据源中的记录引用。需要强制形态时使用：

- `@ref`：必须写成记录引用。
- `@inline`：必须写成内联对象。
- 字段类型包含 `@singleton` type 时，无论是否显式写 `@ref`，数据值都必须写成记录引用，且不能写 `@inline`。

```text
type Drop {
  @ref
  item: Item;

  @inline
  reward: Reward;
}
```

表格单元格和 CFD 中的具体引用写法见数据源与 CFD 参考。导出时，记录引用会保存为目标 record key。

常见引用场景：

- 掉落奖励引用物品 record，例如 `Item.sword`。
- 怪物引用掉落表 record，例如 `DropTable.goblin_drop`。
- 路径引用定位到某条记录的字段或集合元素，例如 `DropTable.goblin_drop.rewards[0]`。

## nullable

nullable 用于表达字段可以没有值，但仍然要显式处理 `null`。

`T?` 表示字段可以为 `null`。

```text
type Drop {
  item: Item?;
  backup: Item? = null;
}
```

安全访问惯用写法：

```text
check {
  item != null && item.id != "";
}
```

对 `null` 做字段访问、索引访问、大小比较或算术，会在 check 执行时报错。

## check 块

`check` 用来把业务规则写进 schema，并在 `coflow check` / `coflow build` 阶段提前拦截错误配置。它是 CFT 的核心能力之一。

`check` 是 `type` 内部的校验块，必须位于所有字段声明之后。

`check` 只在 Coflow 检查/构建阶段执行，用来阻止错误配置进入导出产物。它不会被导出成游戏运行时代码。

```text
const MAX_LEVEL: int = 100;

type Monster {
  level: int;
  tags: [string] = [];
  drop_weights: [int] = [];

  check {
    1 <= level <= MAX_LEVEL;
    tags.isUnique();
    all weight in drop_weights {
      weight > 0;
    }
  }
}
```

`check` 在对象构建完成、记录引用解析完成后执行。它可以访问：

- 当前对象字段。
- 继承字段。
- 虚拟 `id`。
- `const` 常量。
- enum 值。
- 已解析引用对象的字段。

父类 `check` 会对子类实例执行，并且执行顺序是从根类到当前类。

### 条件语句

条件语句用于声明一条必须成立的布尔规则。

一个表达式加分号就是一条条件语句，表达式结果必须是 `bool`。

```text
check {
  damage > 0;
  cooldown >= 0.1;
  0 < damage <= 999;
}
```

多条语句相互独立。某条条件为假时，Coflow 会继续执行后续语句并收集诊断。

### when 块

`when` 用于有条件地启用一组校验，适合“某个开关打开后，另一些字段必须满足要求”的规则。

```text
type Skill {
  is_passive: bool;
  cooldown: float? = null;
  range: float? = null;

  check {
    when !is_passive {
      cooldown != null;
      cooldown > 0.0;
    }
    when is_passive {
      range != null;
      range > 0.0;
    }
  }
}
```

### 量词块

量词用于遍历数组或字典，适合校验集合中的每个元素、至少一个元素或没有元素满足某个条件。

```text
check {
  all weight in weights {
    weight > 0;
  }

  any reward in rewards {
    reward is CurrencyReward;
  }

  none tag in tags {
    tag == "";
  }
}
```

| 量词 | 语义 | 空集合行为 |
| --- | --- | --- |
| `all x in col { ... }` | 全部元素通过 | 通过 |
| `any x in col { ... }` | 至少一个元素通过 | 失败 |
| `none x in col { ... }` | 没有元素通过 | 通过 |

遍历字典时，变量是 entry 对象，可访问 `.key` 和 `.value`：

```text
all entry in resistances {
  0.0 <= entry.value <= 1.0;
}
```

### 类型判断

`is` 用于在 check 中判断多态对象的实际类型，也可以判断 nullable 值是否为 `null`。

```text
check {
  reward is Reward;
  reward is CurrencyReward;
  item is null;
}
```

`is null` 用于 nullable 类型。

### 内建函数

内建函数提供常见集合和字符串校验能力，避免在数据源外再写脚本检查。

| 函数 | 适用类型 | 返回值 | 说明 |
| --- | --- | --- | --- |
| `col.len()` | array / dict | int | 元素数量 |
| `col.contains(value)` | array / dict | bool | array 检查元素，dict 检查 key |
| `array.isUnique()` | array | bool | 元素是否唯一，支持 int、bool、string、enum 及其 nullable 形式 |
| `array.min()` | int / float / enum array | 同元素类型 | 最小值 |
| `array.max()` | int / float / enum array | 同元素类型 | 最大值 |
| `array.sum()` | int / float array | 同元素类型 | 求和 |
| `dict.keys()` | dict | array | key 数组 |
| `dict.values()` | dict | array | value 数组 |
| `str.matches("pattern")` | string | bool | 正则匹配 |

`matches` 使用 Rust `regex` 语义，默认是子串匹配；需要全量匹配时写 `^...$`。

## 枚举

`enum` 用来约束有限选项，适合稀有度、伤害类型、职业、阵营等固定分类。

当字段只能从一组固定选项中选择时，用 `enum`，不要用裸 `string` 表达业务分类。

```text
enum Rarity {
  Common,
  Rare,
  Epic,
}
```

变体默认从 `0` 开始递增，也可以显式指定整数值：

```text
enum Status {
  None = 0,
  Active = 10,
  Dead = 20,
  Ghost,  # 自动为 21
}
```

使用枚举值时写 `EnumName.Variant`：

```text
type Item {
  rarity: Rarity = Rarity.Common;
}
```

规则：

- 同一枚举内禁止重复整数值。
- 枚举与 `int` 不隐式互转。
- 枚举只能与同类型枚举比较。

### 位标志枚举

`@flag` 用于定义位标志枚举。

```text
@flag
enum Permission {
  Read = 1,
  Write = 2,
  Execute = 4,
}
```

约束：

- 除 `0` 外，所有变体值必须是 2 的幂。
- 支持 `&`、`|`、`^`、`~` 位运算。
- 运算结果仍是同一枚举类型。

```text
check {
  (flags & Permission.Read) != Permission(0);
}
```

## 注解

注解用于补充 schema 语义，影响加载器、导出和代码生成。注解写在 `type`、`enum` 或字段之前。

| 注解 | 适用目标 | 影响阶段 | 说明 |
| --- | --- | --- | --- |
| `@flag` | enum | schema / codegen | 位标志枚举 |
| `@struct` | type | codegen | C# codegen 生成 value-like struct；目标必须是 `sealed type` |
| `@expand` | field | table loader | 表格相邻列展开成嵌套对象 |
| `@ref` | field | data parsing / model build | 数据值必须写成记录引用 |
| `@inline` | field | data parsing / model build | 数据值必须写成内联对象 |
| `@idAsEnum(EnumName)` | type | build / codegen | 按 record key 填充空 enum，用于强类型 key |
| `@localized` / `@localized("bucket")` | field | dimensions / check / codegen | 字段值按语言维度变化 |
| `@singleton` | type | data model / codegen | 数据集中该 type 只有一条 record |

示例：

```text
@idAsEnum(ItemId)
type Item {
  @localized
  name: string;
}

enum ItemId {}
```

### `@idAsEnum`

`@idAsEnum(EnumName)` 用于把数据源中的 record key 填充进一个空 enum。

```text
@idAsEnum(ItemId)
type Item {
  name: string;
}

enum ItemId {}
```

构建时，Coflow 会维护 `coflow.enum.lock.json` 来稳定生成 enum 的整数值。这个文件位于 `coflow.yaml` 同级，不属于生成输出目录。

### `@singleton`

`@singleton` 声明该类型在数据集中只有一条 record。

```text
@singleton
type GameConfig {
  max_level: int;
}
```

约束：

- 不能用于 `abstract type`。
- 不能与 `@idAsEnum` 同时使用。
- 可以被字段类型引用，包括数组、字典和 nullable 内层。
- 引用 singleton 的字段值必须写成记录引用，不能写成内联对象。
- `@inline` 不能用于包含 singleton type 的字段；`@ref` 可以显式写，也可以省略。
- record key 仍由数据源提供。

### `@localized`

`@localized` 声明字段值随语言维度变化。

```text
type Item {
  @localized
  name: string;

  @localized("ui")
  description: string;
}
```

项目中使用 `@localized` 时，需要在 `coflow.yaml` 配置 `dimensions.language`。详见 [本地化与维度](../10-localization.md)。

`@localized` 只能用于顶层 type 字段，不能用于 `sealed type` 的内部对象字段。`@localized("bucket")` 的 bucket 必须是合法 CFT 标识符。

## 常量

`const` 用来定义编译期常量，适合复用等级上限、权重总和、默认阈值等固定值。它可用于字段默认值和 `check` 表达式。

当多个字段默认值或业务规则共享同一个阈值时，用 `const` 避免 magic number 分散在 schema 中。

```text
const MAX_LEVEL = 100;
const MIN_SPEED = 0.1;
const EMPTY_NAME = "unknown";
```

可以显式标注基础类型：

```text
const MAX_LEVEL: int = 100;
const MIN_SPEED: float = 0.1;
const ENABLED: bool = true;
const NAME: string = "hero";
```

规则：

- 值暂时只允许整数、浮点、布尔、字符串字面量。
- 类型标注只支持 `int`、`float`、`bool`、`string`。
- 不允许数组、对象或 `null` 作为 `const` 值。
- `const` 不接受注解。

## 和数据源的关系

CFT 只定义 schema，不保存 record 数据。数据来自 Excel、CSV、CFD、飞书/Lark 表格等 source。

数据源会根据 CFT schema 解析单元格或文本值：

- 表格 sheet 通常映射到 CFT type。
- 表头映射到 CFT 字段。
- `id` / `Id` / `ID` 列作为 record key，不是 CFT 字段。
- 空值、`_`、`null`、数组、字典、内联对象、记录引用等值语法见 [单元格值语法](./03-cell-value.md)。
- CFD 文本配置语法见 [CFD 语法参考](./02-cfd.md)。

## 和导出/codegen 的关系

CFT schema 会影响导出和代码生成：

- JSON 和 MessagePack 根据 schema/model 导出字段和值。
- C# codegen 根据 type、enum、字段和注解生成运行时 API。
- `@flag` 生成 C# `[Flags]` enum。
- `@struct` 生成 C# struct。
- `@idAsEnum` 生成强类型 record key。
- `@localized` 生成本地化运行时访问结构。

## 完整示例

```text
const MAX_LEVEL: int = 100;
const MAX_ATTACK: int = 999;

enum Rarity {
  Common = 0,
  Rare = 10,
  Epic = 20,
}

@struct
sealed type Stats {
  hp: int;
  attack: int;

  check {
    hp > 0;
    0 <= attack <= MAX_ATTACK;
  }
}

@idAsEnum(ItemId)
type Item {
  @localized
  name: string;

  rarity: Rarity = Rarity.Common;
  tags: [string] = [];

  check {
    id.matches("^[a-z][a-z0-9_]*$");
    name != "";
    tags.isUnique();
  }
}

enum ItemId {}

abstract type Reward {
  source: string = "drop";

  check { source != ""; }
}

sealed type ItemReward : Reward {
  @ref
  item: Item;
  count: int = 1;

  check { count > 0; }
}

sealed type CurrencyReward : Reward {
  amount: int;

  check { amount > 0; }
}

type Monster {
  name: string;
  level: int;
  stats: Stats;
  rewards: [Reward] = [];
  boss_drop: Item? = null;

  check {
    name != "";
    1 <= level <= MAX_LEVEL;
    stats.hp > 0;

    when boss_drop != null {
      boss_drop.rarity >= Rarity.Rare;
    }

    all reward in rewards {
      reward.source != "";
    }
  }
}
```

## 常见错误

| 错误写法 | 为什么错 | 推荐做法 |
| --- | --- | --- |
| 在同一项目里重复声明 `Item` | `const`、`enum`、`type` 共用全局命名空间 | 保持顶层名称唯一 |
| 字段名写成 `id`、`Id` 或 `ID` | record key 是虚拟字段名，属于保留名 | 使用业务字段名，例如 `name`、`item_key` |
| `rarity > 5` | enum 与 `int` 不隐式互转 | 写 `rarity >= Rarity.Rare` |
| `check { price; }` | `check` 条件必须是 `bool` | 写 `price > 0` |
| `type Item { check { name != ""; } name: string; }` | `check` 块必须位于所有字段声明之后 | 先声明字段，再写 `check` |
| `@struct type Stats { ... }` | `@struct` 要求目标是 `sealed type` | 写 `@struct sealed type Stats { ... }` |
| `@idAsEnum(ItemId)` 但没有声明 `enum ItemId {}` | `@idAsEnum` 参数必须是已声明的空 enum | 先声明空 enum |
| `@localized name: string;` 但项目未配置 `dimensions.language` | 语言维度未启用 | 在 `coflow.yaml` 中配置 `dimensions.language` |
| 字段默认值类型不匹配，例如 `price: int = "10"` | 默认值必须能赋给字段类型 | 写 `price: int = 10` |
| 引用未知类型或未知 enum variant | schema 编译时无法解析名称 | 检查类型名、enum 名和 variant 名拼写 |
