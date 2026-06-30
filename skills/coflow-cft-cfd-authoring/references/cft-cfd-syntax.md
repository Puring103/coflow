# CFT/CFD 编写参考

## 目录

- [CFT 示例](#cft-示例)
- [CFT 基础语法](#cft-基础语法)
- [常量与默认值](#常量与默认值)
- [枚举与 @flag](#枚举与-flag)
- [字段类型速查](#字段类型速查)
- [nullable 与安全访问](#nullable-与安全访问)
- [check 块](#check-块)
- [高级注解](#高级注解)
- [继承和多态](#继承和多态)
- [CFD 记录](#cfd-记录)
- [表格与 CFD 的关系](#表格与-cfd-的关系)
- [表格单元格语法](#表格单元格语法)

## CFT 示例

```cft
const MAX_LEVEL: int = 100;

enum Rarity {
  Common = 0,
  Rare = 10,
  Epic = 20,
}

@idAsEnum(ItemId)
type Item {
  name: string;
  price: int;
  rarity: Rarity = Rarity.Common;
  tags: [string] = [];

  check {
    price > 0;
  }
}

enum ItemId {}
```

## CFT 基础语法

- `.cft` 只写 `const`、`enum`、`type`，不写数据和导入语句。
- `#` 是注释；字段用 `;` 分隔，enum 变体用 `,` 分隔，允许末尾分隔符。
- 所有 schema 文件共享项目级全局命名空间；`const`、`enum`、`type` 名称不能重名，支持前向引用。
- 注解写在目标前一行或前方，例如 `@idAsEnum(ItemId) type Item {}`、字段内写 `@localized` 后再写字段声明。
- 不要声明 `id`、`Id`、`ID` 字段；顶层 record key 是虚拟 `id`，只能在 `check` 里读取。
- 保留名不能用作类型、字段、枚举变体、量词变量名，包括 `const`、`enum`、`type`、`check`、`when`、`all`、`any`、`none`、`in`、`is`、`true`、`false`、`null`、`int`、`float`、`bool`、`string`、`len`、`contains`、`isUnique`、`min`、`max`、`sum`、`keys`、`values`、`matches`、`_`。

## 常量与默认值

```cft
const MAX_LEVEL: int = 100;
const DEFAULT_NAME = "unknown";

type Skill {
  name: string = DEFAULT_NAME;
  level: int = 1;
  ratio: float = 1.0;
  enabled: bool = true;
  rarity: Rarity = Rarity.Common;
  tags: [string] = [];
  weights: {string: int} = {};
  next: Skill? = null;
}
```

规则：

- `const` 只能是 `int`、`float`、`bool`、`string` 字面量，可省略类型标注；不能写数组、对象、`null` 或 named type。
- 无默认值字段必须在数据里填写；有默认值字段可以省略。
- 字段默认值只能是字面量、`const`、枚举值、`[]`、`{}` 或 `null`；不能引用其他字段或运行期对象。
- `float` 字面量必须是有限值；不要写 `NaN` 或 `inf`。

## 枚举与 `@flag`

普通 enum 默认从 `0` 自动编号，可显式指定整数值：

```cft
enum Status {
  None,
  Active = 10,
  Dead,
}
```

`Dead` 的值是 `11`。枚举值在默认值和 check 里写 `Status.Active`，不要裸写 `Status`。

位标志 enum 用 `@flag`：

```cft
@flag
enum Permission {
  Read = 1,
  Write = 2,
  Execute = 4,
}
```

规则：

- 同一 enum 内变体名和值都不能重复。
- enum 不和 `int` 隐式互转；比较时两边必须是同一个 enum 类型。
- `@flag` 变体值必须是 2 的幂；用 `&`、`|`、`^`、`~` 做位运算。
- `Permission(0)` 表示该 enum 的零值；不要和裸 `0` 比较。
- enum variant 当前不支持注解。

## 字段类型速查

| 类型 | 用法 |
| --- | --- |
| `int` | 64 位整数 |
| `float` | 64 位浮点 |
| `bool` | 布尔 |
| `string` | 字符串 |
| `[T]` | 数组，元素可以是任意合法字段类型 |
| `{K: V}` | 字典，`K` 只能是 `string`、`int` 或 enum 类型 |
| `T?` | nullable，允许显式 `null` |
| `TypeName` | 对 CFT type 的对象引用或内联对象 |
| `EnumName` | enum 值 |

建模规则：

- CFT type 是名义类型；字段相同的两个 type 也不能互相替代。
- 顶层记录 key 在同一具体类型内唯一；如果按父类或 abstract type 引用，赋值兼容范围内的 key 也必须唯一。
- 循环引用允许；引用解析在 data model build 阶段完成。
- 字典 key 解析后必须唯一；重复 key 是错误，不是后写覆盖。
- 对象字段默认同时接受记录引用和内联对象；字段上无参 `@ref` 强制引用，`@inline` 强制内联对象。

## nullable 与安全访问

```cft
type Drop {
  item: Item?;          # 必须填写，可填 null
  backup: Item? = null; # 可省略

  check {
    item == null || item.id != "";
    when backup != null {
      backup.price > 0;
    }
  }
}
```

- `T?` 和字段缺失不是一回事；没有默认值的 nullable 字段仍要显式填写。
- 对 `null` 做字段访问、索引、大小比较或算术会在 check 执行时报错。
- 安全访问使用短路：`x != null && x.field > 0`，或用 `when x != null { ... }`。
- `is null` 只能用于 nullable；`is TypeName` 只能用于对象或可空对象。

## check 块

`check` 放在 type 内所有字段声明之后。可以访问当前对象字段、继承字段、虚拟 `id`、`const`、枚举值，以及已解析引用对象的字段。

```cft
type Monster {
  name: string;
  level: int;
  tags: [string] = [];
  rewards: [Reward] = [];
  resistances: {DamageType: float} = {};

  check {
    id.matches("^[a-z][a-z0-9_]*$");
    name != "";
    1 <= level <= MAX_LEVEL;
    tags.isUnique();

    when rewards.len() > 0 {
      any reward in rewards {
        reward is ItemReward;
      }
    }

    all entry in resistances {
      0.0 <= entry.value <= 1.0;
    }
  }
}
```

常用语法：

- 普通条件：`price > 0;`，表达式结果必须是 `bool`。
- `when cond { ... }`：条件成立时执行块内约束；可嵌套。
- `all x in col { ... }`：所有元素满足；空集合通过。
- `any x in col { ... }`：至少一个元素满足；空集合失败。
- `none x in col { ... }`：没有元素满足；空集合通过。
- dict 量词变量是 entry 对象，使用 `.key` 和 `.value`。
- 链式比较只允许同方向：`0 <= x <= 10`、`10 >= x > 0`；不要写 `a < b > c`。

常用运算和函数：

- 逻辑：`&&`、`||`、`!`，支持短路。
- 比较：`==`、`!=`、`<`、`<=`、`>`、`>=`。
- 算术：`+`、`-`、`*`、`/`、`//`、`%`、`**`。
- 位运算：`&`、`|`、`^`、`~`，用于 `int` 或 `@flag` enum；`<<`、`>>` 只用于 `int`。
- 类型判断：`reward is ItemReward`、`item is null`。
- 集合：`col.len()`、`col.contains(x)`、`array.isUnique()`、`array.min()`、`array.max()`、`array.sum()`、`dict.keys()`、`dict.values()`。
- 字符串：`str.matches("^prefix")`，pattern 必须是字符串字面量。

执行规则：

- 子类实例会按父类到子类顺序执行所有 check。
- 普通失败会继续收集后续错误；null access、越界、缺失 dict key、空 `min/max` 等硬错误会停止当前对象后续 check。
- 同一对象被多处引用时，check 只执行一次。

## 高级注解

### `@idAsEnum`

`@idAsEnum(Name)` 把某个 type 的 record key 收集进一个手动声明的空 enum，用于生成强类型 key：

```cft
@idAsEnum(ItemId)
type Item {
  name: string;
}

enum ItemId {}
```

规则：

- 注解只能放在 `type` 上，参数是 enum 名称。
- 目标 enum 必须已声明且为空；不要手写数据驱动变体。
- `coflow build` 会在 `coflow.enum.lock.json` 中稳定分配整数值；该 lockfile 应提交。
- 数据行顺序改变不应改变已有 enum 值；新增 record key 会追加。
- 如果空 enum 带 `@flag`，新变体按 `1, 2, 4, ...` 分配。

### `@localized` 与维度变体

`@localized` 表示字段按语言维度取不同值：

```cft
type Item {
  name: string;
  @localized
  description: string;
  @localized("ui")
  title: string;
  @localized(bucket = "ui")
  icon: string;
}
```

项目需要在 `coflow.yaml` 配置：

```yaml
dimensions:
  language:
    variants: [zh, en]
    out_dir: data/dimensions/language
```

规则：

- 当前用户可用维度是 `language`；变体如 `zh`、`en` 必须是合法 CFT 标识符。
- `default` 是保留变体名，不能写进 `variants`。
- `out_dir` 下的维度文件由引擎维护，不需要写进 `sources`。
- `bucket` 改变维度文件分组名；未设置时默认按源 type/字段生成。
- `check` 会在默认值和各语言变体下执行；变体值为 `null` 时跳过该字段在该轮的替换。
- `@localized` 只能用于非 sealed type 的字段；不要用于 type、enum、enum variant、const 或 sealed value object 的内部字段。

### `@singleton`

`@singleton` 表示该 type 在数据集中有且仅有一条记录：

```cft
@singleton
type GameConfig {
  start_scene: string;
  max_level: int;
}
```

规则：

- 不能用于 `abstract type`，不能和 `@idAsEnum` 并用。
- singleton type 可以作为字段类型被引用，包括 `[T]`、`T?`、`{K: V}` 内层。
- 引用 singleton 的字段值必须写成记录引用，不能写内联对象；`@ref` 可显式写也可省略。
- `@inline` 不能用于包含 singleton type 的字段。
- 该 type 必须有且仅有一条 record，record key 必须是合法 CFT 标识符。
- 所有 singleton record key 在项目内全局不能撞名。
- C# codegen 不生成 `Tb*` 表访问器，而是在入口类上生成以 record key 命名的属性。

### `@struct`、`@expand`、`@ref`、`@inline`

- `@struct` 只能用在 `sealed type` 上，让 C# codegen 生成 struct。
- `@expand` 只能用在具体 object 字段上，不能用于 primitive、enum、array、dict、nullable；让 Excel/CSV 相邻列展开为嵌套对象字段。
- `@ref` 只能用在包含 object 的字段上，例如 `Item`、`Item?`、`[Item]`、`{string: Item}`；数据必须写记录引用或路径引用，拒绝内联对象。
- `@inline` 只能用在包含 object 的字段上；数据必须写内联对象，拒绝 `&key`、`@Type.key` 和路径引用。
- `@ref` 与 `@inline` 互斥；`@ref` 与 `@expand` 冲突；`@inline` 可以和 `@expand` 并用。
- 集合字段上的 `@ref` / `@inline` 会约束数组元素或字典 value。

注解目标速查：

| 注解 | 目标 | 关键限制 |
| --- | --- | --- |
| `@struct` | type | 必须是 `sealed type` |
| `@flag` | enum | 变体值必须是 2 的幂 |
| `@expand` | field | 字段类型必须是具体 type |
| `@ref` | field | 字段类型必须包含 object；无参数 |
| `@inline` | field | 字段类型必须包含 object 且不能包含 singleton；无参数 |
| `@idAsEnum(EnumName)` | type | 参数 enum 必须已声明且为空 |
| `@localized` / `@localized("bucket")` / `@localized(bucket = "bucket")` | field | bucket 必须是合法标识符 |
| `@singleton` | type | 不能用于 abstract，不能和 `@idAsEnum` 并用 |

## 继承和多态

```cft
abstract type Reward {
  source: string = "drop";
}

sealed type ItemReward : Reward {
  item: Item;
  count: int = 1;
}

sealed type CurrencyReward : Reward {
  amount: int;
}
```

字段类型为 `Reward` 时，可写入 `ItemReward` 或 `CurrencyReward`。

建模建议：

- 父类用于共享字段和 check。
- `abstract type` 不能直接实例化，适合纯接口/基类。
- `sealed type` 不能再派生，适合作为多态叶子或值对象。
- 子类记录可以赋给父类字段；父类记录不能赋给子类字段。
- `schema inspect --type Reward --include-derived` 可查看父类及所有可赋值子类。

## 常见建模错误

- 把 record key 建成 `id` 字段：应使用数据源的 key 列或 CFD 顶层 key，CFT 里不要声明 `id`。
- 用 `string` 表示固定集合：应使用 enum；需要数据驱动 key 时使用 `@idAsEnum`。
- 在 enum 和 int 之间比较：改成同 enum 比较，或调整字段类型。
- nullable 字段没有默认值却以为可以省略：需要 `= null` 才能省略。
- 在 `check` 里直接访问可空字段：先用 `!= null` 或 `when` 保护。
- 将 `@localized` 写进 sealed 值对象内部字段：维度字段应放在顶层业务 type 字段上。
- 把 singleton type 当字段类型引用：singleton 只能作为数据集中唯一记录，通过显式引用访问。
- 在 `check` 后继续声明字段：`check` 必须是 type 内最后一段。

## CFD 记录

```cfd
sword_01: Item {
  name: "Iron Sword",
  price: 100,
  rarity: Rare,
  tags: ["weapon", "melee"],
}
```

规则：

- `sword_01` 是 record key。
- `Item` 是 CFT 类型名。
- 顶层记录块内不要写 `id` 字段。
- 字段、数组、字典项用逗号分隔，允许尾逗号。

## CFD 分组

```cfd
Item {
  sword_01 { name: "Sword", price: 100 }
  shield_01 { name: "Shield", price: 80 }
}
```

多态分组：

```cfd
Reward {
  coin_01: CurrencyReward { amount: 100 }
  item_01: ItemReward { item: &sword_01, count: 1 }
}
```

## 引用

```cfd
item: &sword_01
item: @Item.sword_01
label: @TextTable.main.labels["start"]
weight: @DropTable.default.weights[Fire]
```

- `&key` 是直接引用简写，只适合目标类型明确的对象字段。
- `@Type.key` 是显式 typed record reference。
- 路径引用必须从 `@Type.key` 开始。
- 不要使用旧的 `@key`。
- 字段带 `@ref` 时必须使用引用；字段带 `@inline` 时必须写内联对象。

## 数组、字典、对象

```cfd
monster_01: Monster {
  name: "Slime",
  level: 3,
  stats: { hp: 100, attack: 20 },
  tags: ["early", "forest"],
  weights: { Fire: 10, Ice: 5 },
}
```

`null` 只用于 nullable 字段，例如 `Item?` 或 `string?`。

## Spread 覆盖

```cfd
base: Monster {
  name: "Base",
  stats: { hp: 100, attack: 20 },
}

elite: Monster {
  ...@Monster.base,
  name: "Elite",
  stats: { ...@Monster.base.stats, hp: 180 },
}
```

spread 按出现顺序合并；本地字段覆盖 spread 来源。

## 表格与 CFD 的关系

- Excel/CSV 每一行等价于一条顶层记录。
- 表格 `id` 列等价于 CFD 的 record key。
- CFD 和表格最终进入同一个 data model，可以互相引用。
- 表格适合大量同构数据；CFD 适合复杂嵌套和模板覆盖。

## 表格单元格语法

表格单元格语法由字段的 CFT 类型决定。目标类型是 `string` 时，`true`、`123`、`@Item.sword` 都是字符串；目标类型是对象或 enum 时才按对象、引用或枚举解析。

### 标量和字符串

| 目标类型 | 写法 |
| --- | --- |
| `int` | `42`、`-3` |
| `float` | `3.14`、`-1.5`，不能写 `NaN` 或 `inf` |
| `bool` | `true`、`false`，也可写 `1`/`0`、`yes`/`no`、`y`/`n` |
| `string` | 可裸写；含 `,`、`|`、`:`、括号、空字符串、`_`、`null` 时用双引号 |
| `EnumName` | `Rare` 或 `Rarity.Rare` |

字符串转义使用 JSON 风格：`\"`、`\\`、`\n`、`\r`、`\t`。

### 引用和路径

```text
@Item.sword_01
@DropTable.default.rewards[0]
&sword_01
```

- `@Type.key` 是显式记录引用；路径引用必须从它开始。
- `&key` 是按目标字段类型推断的直接引用，不支持路径。
- 目标类型是 string 时，`@Item.x` 和 `&x` 都只是字符串。

### 对象

对象可以按字段顺序写，也可以按字段名写；同一个对象内不要混用。

```text
100, 50
hp: 100, attack: 50
hp: 100, speed: 2.0
```

根对象可以省略 `{}`；嵌套对象必须写 `{}`：

```text
level: 5, stats: {hp: 100, attack: 50}
```

对象字段可以用 `_` 跳过，等价于该字段未提供，后续按 schema 默认值处理：

```text
_, 3
_, _, notice
```

表格 writer 会把对象单元格渲染成可回读的 `TypeName{field: value}` 形式；手写时也可以显式写实际类型：

```text
Stats{hp: 100, attack: 50}
Reward{amount: 25, name: Coin}
```

### 数组

数组用 `|` 分隔。根数组可以省略 `[]`，嵌套数组必须写 `[]`。

```text
1 | 2 | 3
Rare | Epic | Common
warrior | tank | elite
tags: [weapon | melee]
```

数组元素是对象时，每个对象元素必须写 `{}`：

```text
{hp: 100, attack: 50} | {hp: 200, attack: 80}
```

### 字典

字典条目用 `,` 分隔，key 和 value 用 `:` 分隔。根字典可以省略 `{}`，嵌套字典必须写 `{}`。

```text
Fire: 10, Ice: 5
{Fire: 10, Ice: 5}
alice: 10, bob: 20
1: sword, 2: shield
```

字典 key 按 schema 类型解析：`string`、`int` 或 enum。重复 key 是错误。

### 多态对象

字段声明为父类或 abstract type 时，用 `ConcreteType{...}` 标记实际类型：

```text
ItemReward{item: @Item.sword_01, count: 1}
CurrencyReward{amount: 100}
CurrencyReward{amount: 100} | ItemReward{item: &sword_01, count: 1}
```

### 空值、跳过和边界

| 写法 | 语义 |
| --- | --- |
| 空单元格 | root 字段未提供，按 schema 默认值处理 |
| `_` | root 或对象字段未提供，按 schema 默认值处理 |
| `null` | 显式 null，目标类型必须是 `T?` |
| `"_"` / `"null"` | 字符串内容 `_` / `null` |

只在括号深度为 0 且不在字符串内的位置分隔 `,` 和 `|`；引号内的分隔符只是字符串内容。数组不用逗号分隔，对象和字典不用 `|` 分隔。
