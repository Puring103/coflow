# cft / cfd 配置系统

Coflow 配置系统由两种文件类型组成：

- `.cft`（Coflow Type File）：类型定义文件，只包含 `type` 和 `enum` 定义；
- `.cfd`（Coflow Data File）：数据文件，只包含数据定义和 `check` 块。

两种格式均可脱离 `.cfs` 独立运行，可被任意宿主语言加载，定位类似 JSON，但额外提供类型化枚举、对象 identity 和 schema 校验。两种格式均为纯数据语言，不执行运行时逻辑，不能定义函数、方法或控制流语句。

注释使用 `//`：

```cft
// 这是注释
type Item { id: string; }  // 行尾注释
```

## 模块系统

`.cft` 和 `.cfd` 均无 `use` 导入语句。宿主将所有相关模块批量注册到容器，语言层在 build 阶段全局解析名称，找不到则报错。

**ModuleId** 由宿主在注册时指定，是不透明字符串，语言层不做路径解析。推荐宿主使用相对项目根的路径去掉扩展名作为 ModuleId：

```
schema/item.cft   →  ModuleId = "schema/item"
data/weapons.cfd  →  ModuleId = "data/weapons"
```

**类型全局可见**：所有已注册的 `.cft` 模块共享同一个全局类型命名空间。`.cfd` 文件中类型标注和枚举值直接使用裸名，无需限定模块前缀。两个 `.cft` 模块定义同名 `type` 或 `enum` 时，注册时立即报错。

**跨模块数据引用**：`.cfd` 文件中引用其他模块的数据节点使用 `moduleid.name` 限定名，ModuleId 直接作为前缀：

```cfd
// 引用 ModuleId="data/common" 模块中的 shared_stats 数据节点
boss: Monster = { stats: data/common.shared_stats };
```

循环引用完全允许，loader 在构建阶段统一处理。

## CFT 文件顶层结构

`.cft` 文件只包含 `type` 和 `enum` 定义，可混合，任意顺序。

`type` 和 `enum` 支持所有前向引用，包括 `type` 引用 `enum`、`type` 引用 `type`。

示例：

```cft
// schema/entities.cft
enum Rarity { common, rare, epic; }

type Stats {
  hp: int;
  speed: float;
}

type Item {
  id: string;
  rarity: Rarity;
}
```

## CFD 文件顶层结构

`.cfd` 文件的顶层由两段组成，段之间不能交错：

1. `type` 和 `enum` 定义（可混合，任意顺序）——**禁止**，`.cfd` 不允许类型定义
2. 顶层数据定义和 `check` 块（可穿插）

实际上 `.cfd` 只有数据段，不含定义段：

```cfd
// data/monsters.cfd
slime_stats: Stats = { hp: 30; speed: 1.25; };

slime: Monster = {
  id: "slime";
  stats: slime_stats;
  rarity: Rarity.common;
  drop: data/items.goblin_drop;  // 跨模块引用
};
```

数据定义之间支持前向引用，跨模块引用使用 `moduleid.name` 限定名。

## `type` 类型定义

`type` 用于定义数据结构。类型定义只能包含字段，不能包含方法。字段之间用 `;` 分隔，允许末尾 `;`。

```cfc
type Weapon {
  id: string;
  damage: int;
  cooldown: float = 1.0;
}
```

字段规则：

- 字段必须显式标注类型
- 无默认值的字段必须填写
- 有默认值的字段可以省略；默认值必须是常量（字面量或枚举值，包括空数组 `[]`、空对象 `{}` 和空字典 `dict{}`），不能引用其他字段
- 默认值填充时会复制默认值本身；数组、对象和字典默认值不会在多个实例之间共享 identity

支持的字段类型：

- 基础类型：`int`、`float`、`bool`、`string`
- `null`：显式空值，只能写入允许 `null` 的类型
- 字面量类型：例如 `"currency"`、`1`、`true`，只接受完全相同的值
- `any`：接受任意值，loader 只做引用合法性检查，不做类型匹配
- 数组类型：`[T]`，T 可以是任意合法字段类型，包括 `any`
- 字典类型：`{K: V}`，K 只允许 `string`、`int` 或任意已定义的 `enum` 类型名，V 可以是任意合法字段类型，包括 `any`
- 当前文件或导入文件中的 `type`
- 当前文件或导入文件中的 `enum`
- union alias：由若干已命名 `type` 组成，例如 `type Reward = ItemReward | CurrencyReward;`
- nullable：`T | null`，也可写成 `T?`

`type` 使用名义类型（nominal typing），不使用结构类型。两个 `type` 即使字段完全相同，也不是同一个类型。对象字面量只有在上下文提供明确类型时才按该 `type` 校验；无类型标注的数据节点和 `any` 字段不会因为字段形状自动推断为某个 `type`。

```cfc
type A {
  id: string;
}

type B {
  id: string;
}

a: A = { id: "x" };
b: B = a;          // 错误：A 不是 B
raw = { id: "x" }; // 合法，但 raw 的结构不按 A 或 B 校验
```

### union alias

`type` 也可以声明为 union alias。union alias 会在运行时保存为 union wrapper，wrapper 记录 union alias，同时包含实际分支对象；字段访问、索引、`check` 遍历和 `is` 判断会透明访问内部实际值。

```cfc
type ItemReward {
  item: Item;
}

type CurrencyReward {
  amount: int;
}

type Reward = ItemReward | CurrencyReward;

reward: Reward = CurrencyReward {
  amount: 100,
};
```

第一版 union 分支必须是已命名 `type`，不支持匿名 object union。对象字面量赋给 union 时必须显式写出分支类型：

```cfc
reward: Reward = CurrencyReward { amount: 100 };
remote_reward: lib.Reward = lib.CurrencyReward { amount: 100 };
```

若普通对象字面量直接赋给 union，build 报类型错误。若显式分支类型不是该 union 的分支，build 也报类型错误。若已有命名节点的实际 nominal type 是 union 分支之一，则可以赋给 union：

```cfc
coin: CurrencyReward = { amount: 10 };
reward: Reward = coin;
```

第一版不支持裸结构匹配、按唯一可匹配字段集合推断分支、隐式 discriminator、匿名 object union。

### `null` 和 nullable

`null` 是显式值，不等价于字段缺失。字段缺失仍然按必填字段报结构错误。

```cfc
type Drop {
  item: Item | null = null;
  backup: Item?;
}
```

规则：

- 只有类型允许 `null` 时才能填入 `null`
- `any` 可以接受 `null`
- `null == null` 为 true，`null != value` 为 true
- 对 `null` 做字段访问、索引访问、大小比较、算术或聚合会报 check eval error
- 可用逻辑短路表达安全访问：`item != null && item.id != ""`

`type` 支持前向引用和自引用：

```cfc
type Node {
  value: int;
  children: [Node];
}

type A {
  b: B;
}

type B {
  value: int;
}
```

### `check` 块（v2）

`check` 是 `type` 内部的可选数据校验块。Rust reference parser 会解析 `check` 内容；`build()` 只构建对象图，不执行 `check`，语义校验由 `CfcContainer.check()` 在 `build()` 成功后显式执行。`check` 块必须位于所有字段声明之后。

```cfc
type Range {
  min: int;
  max: int;

  check {
    min <= max;
    min >= 0;
  }
}
```

`check` 块由若干条件语句和 `all` / `any` / `none` 量词块组成，没有 `assert` 关键字，没有用户自定义错误消息。

**条件语句**：一个表达式加分号，求值结果必须为 bool。

```cfc
type Weapon {
  damage: int;
  cooldown: float;
  rarity: Rarity;

  check {
    damage > 0;
    cooldown >= 0.1;
    rarity >= Rarity.common;
    0 < damage <= 1000;
  }
}
```

**量词块**：对集合中元素执行内部条件。`all` 表示全部元素通过，`any` 表示至少一个元素通过，`none` 表示没有元素通过。

```cfc
type Loot {
  drops: [Drop];

  check {
    all drop in drops {
      drop.value > 0;
      drop.rarity != Rarity.common;
    }
  }
}
```

`all`、`any` 和 `none` 支持 Array 和 Dict。迭代 Array 时绑定变量直接是元素；迭代 Dict 时绑定变量是 entry 对象，具有 `.key` 和 `.value` 字段：

```cfc
type ScoreTable {
  scores: {string: int};

  check {
    all entry in scores {
      entry.value >= 0;
    }
  }
}
```

量词块支持任意深度嵌套：

```cfc
type Zone {
  monsters: [Monster];

  check {
    all monster in monsters {
      monster.level > 0;
      all drop in monster.drops {
        drop.value > 0;
      }
    }
  }
}
```

**访问范围**：`type` 内 `check` 只能访问当前对象的字段和枚举值，不能引用外部命名节点。跨节点约束用顶层 `check` 表达。

**执行规则**：
- 多条语句顺序求值，条件为假时继续收集后续错误。
- 求值过程中发生类型错误或数组越界时立即停止当前对象的校验。
- 空集合上的 `all` 和 `none` 视为通过，空集合上的 `any` 视为失败。
- 同一对象图中多处引用同一命名节点时，该对象的 check 只执行一次。

**支持的运算符**（优先级从低到高）：

- 逻辑：`||` `&&`，短路求值
- 类型判断：`is`
- 比较：`==` `!=` `<` `<=` `>` `>=`，支持链式比较（方向一致，如 `0 < x <= 100`）
- 按位：`|` `^` `&`
- 算术：`+` `-` `<<` `>>` `*` `/` `//` `%` `**`，`**` 右结合
- 一元：`!` `~` `-`
- 后缀：`.`（字段访问）`[]`（索引访问）、内建函数调用

枚举类型支持全部六种比较运算符，按底层整数值比较。

`is` 判断对象的实际 nominal type，也可以判断对象是否属于某个 union alias，或是否为 `null`：

```cfc
check {
  reward is CurrencyReward;
  reward is Reward;
  !(reward is ItemReward);
  item is null;
}
```

`is` 不做结构匹配。`&&` 短路可以用于最常见的安全访问形式，例如 `item != null && item.id != ""`、`reward is CurrencyReward && reward.amount > 0`。

**内建函数**：`check` 表达式支持受限的内建函数调用，不支持用户自定义函数调用。

| 函数 | 语义 |
|------|------|
| `len(value)` | 返回 array 或 dict 的元素数量；不支持 string。 |
| `contains(collection, value)` | array 中判断元素是否存在；dict 中判断 key 是否存在。 |
| `unique(array)` | 判断数组元素是否唯一；第一版支持 int、bool、string 和同一 enum 类型。 |
| `min(array)` | 返回非空 int、float 或同一 enum 数组中的最小值。 |
| `max(array)` | 返回非空 int、float 或同一 enum 数组中的最大值。 |
| `sum(array)` | 对 int 或 float 数组求和；空数组返回 `0`。 |
| `keys(dict)` | 返回 dict key 数组。 |
| `values(dict)` | 返回 dict value 数组。 |

内建函数只在 `check` 表达式中可用，不是数据定义表达式。`unique` 不支持 float、object、array、dict、null 或 union 元素；`min` / `max` 对空数组报 check eval error；`contains(dict, value)` 只检查 key，不检查 value。

不支持：用户自定义函数调用、字符串插值、变量声明、赋值、`?.`、`?[]`。

## `enum` 枚举定义

`enum` 定义有限的命名整数集合。变体之间用 `,` 分隔，允许末尾 `,`。

```cfc
enum Rarity {
  common,
  rare,
  epic,
}
```

枚举变体默认从 `0` 开始自动编号，依次递增。可以显式指定整数值，未指定的变体从前一个值 +1 继续。同一枚举内禁止重复整数值：

```cfc
enum Status {
  none = 0,
  active = 10,
  dead = 20,
  ghost,        // 自动为 21
}

enum Bad {
  a = 1,
  b = 1,        // 错误：重复值
}
```

使用枚举值通过 `EnumName.variant` 语法：

```cfc
rarity = Rarity.rare;
```

枚举底层表示为整数，但枚举类型与 `int` 不隐式互转。

同一文件内重复声明同名 `type`、`enum` 或数据节点是错误。跨 `.cft` 模块重复声明同名 `type` 或 `enum` 也是错误，在注册时立即报告。

## 字典字面量

字典使用 `dict{ }` 语法，与对象字面量 `{ }` 严格区分：

```cfc
dict{ "alice": 10, "bob": 20 }      // {string: int}
dict{ 1: "sword", 2: "shield" }     // {int: string}
dict{ DamageType.fire: 0.5 }        // {DamageType: float}
```

key 类型和 value 类型从字面量推断。空字典 `dict{}` 无法推断类型，必须带类型标注才合法：

```cfc
scores: {string: int} = dict{};    // 合法
scores = dict{};                    // 错误：无法推断类型
```

同一字典字面量中所有 key 必须类型一致，所有 value 必须类型一致。

## 数据定义

顶层数据定义用于声明命名数据节点：

```cfc
name = value;
name: Type = value;
```

所有顶层数据节点均公开，可以被其他 `.cfd` 或 `.cfs` 通过 `moduleid.name` 限定名引用。`.cfd` 没有私有数据节点。

顶层命名数据节点具有对象 identity。引用命名数据节点时，加载后的对象图保留共享引用关系：

```cfc
shared_stats = {
  hp: 100,
};

slime = {
  stats: shared_stats,
};

goblin = {
  stats: shared_stats,
};
```

加载后，`slime.stats` 和 `goblin.stats` 指向同一个对象。

identity 规则：

- 顶层命名节点如果保存对象、数组或字典，则该值具有稳定 identity。
- 顶层命名节点如果保存基础值或枚举值，不提供可观察的对象 identity。
- 内联对象、数组和字典字面量不具有可被其他节点直接引用的独立 identity；它们是所属值的一部分。
- 只有通过命名节点引用同一个对象、数组或字典时，加载后才保留共享引用关系。

```cfc
shared_tags: [string] = ["enemy"];

slime = {
  tags: shared_tags,       // 与 goblin.tags 共享同一个数组
};

goblin = {
  tags: shared_tags,
};

orc = {
  tags: ["enemy"],         // 独立内联数组，不与 shared_tags 共享
};
```

数据值必须在加载期可完全解析为常量。合法的值形式包括：字面量（整数、浮点、布尔、字符串）、枚举值、对象字面量、数组字面量、字典字面量、以及对其他命名节点的引用（支持前向引用）。

数组字面量的元素必须类型一致，从元素推断数组类型。空数组 `[]` 无法推断类型，必须带类型标注才合法：

```cfc
[1, 2, 3]               // 合法，推断为 [int]
[1, "a"]                // 错误：元素类型不一致
items: [string] = [];   // 合法
items = [];             // 错误：无法推断类型
```

对象字面量的字段用 `,` 分隔，允许末尾 `,`。字典字面量 `dict{}` 内部条目同样用 `,` 分隔，允许末尾 `,`。

顶层数据节点声明以 `;` 结尾，必须显式写出：

```cfc
slime = { id: "slime" };    // 合法
slime = { id: "slime" }     // 错误：缺少末尾 ;
```

无类型标注的数据节点与 `any` 类型语义一致：不进行结构校验，但所有引用必须合法。

跨模块数据引用使用 `moduleid.name` 限定名：

```cfd
// ModuleId="data/common" 模块中的数据节点
boss: Enemy = {
  id: "boss";
  hp: 500;
  drop: data/common.rare_loot;   // 引用 data/common 模块的 rare_loot 节点
};
```

顶层命名节点支持循环引用，对象图允许有环。`build()` 遍历对象图时做 visited 标记以避免无限循环。循环引用只能通过命名节点表达：

```cfc
node_a = {
  value: 1,
  next: node_b,
};

node_b = {
  value: 2,
  next: node_a,
};
```

## 顶层 `check`（v2）

顶层 `check` 块用于约束跨节点的全局关系，是 `type` 内 `check` 做不到的能力。Rust reference parser 会解析顶层 `check` 内容；语义校验由 `CfcContainer.check()` 在 `build()` 成功后显式执行。

顶层 `check` 属于数据定义段，必须位于所有 `type` 和 `enum` 定义之后，可以穿插在数据节点之间。一个文件可以有多个顶层 `check` 块。

```cfc
slime: Monster = {
  id: "slime",
  stats: slime_stats,
  rarity: Rarity.common,
};

goblin: Monster = {
  id: "goblin",
  stats: goblin_stats,
  rarity: Rarity.rare,
};

check {
  slime.stats.hp < goblin.stats.hp;
  goblin.rarity > slime.rarity;
}
```

语法与 `type` 内的 `check` 块完全一致，但访问范围是所在模块的顶层命名节点，而非 `self` 字段。`all` / `any` / `none` 量词块同样可用：

```cfc
monsters: [Monster] = [...];

check {
  all monster in monsters {
    monster.stats.hp > 0;
  }
}
```

**报错标识**：顶层 `check` 失败时以模块标识和行号定位，而非类型名。

## 综合示例

下面的两文件示例覆盖主要语言特性：枚举、类型定义、字段默认值、字面量类型、nullable、union alias、显式 union 分支对象、命名节点共享引用、循环引用、数组、字典、跨模块引用、字段/索引路径、`type` 内 `check`、顶层 `check`、量词和 check-only 内建函数。

`common.cft`：

```cft
enum Rarity {
  common = 0,
  rare = 10,
  epic = 20,
  legendary,
}

enum DamageType {
  physical,
  fire,
  ice,
}

type Stats {
  hp: int;
  attack: int;
  speed: float = 1.0;
  flags: int = 0;

  check {
    hp > 0;
    0 <= attack <= 999;
    speed >= 0.1;
    flags == 0 || flags & 1 == 1;
  }
}

type ResistanceTable {
  values: {DamageType: float};

  check {
    len(values) > 0;
    contains(values, DamageType.fire);
    unique(keys(values));
    min(values(values)) >= 0.0;
    max(values(values)) <= 1.0;

    all entry in values {
      entry.value >= 0.0;
      entry.value <= 1.0;
    }
  }
}

type Item {
  id: string;
  rarity: Rarity = Rarity.common;
  tags: [string] = [];
  resistances: ResistanceTable? = null;

  check {
    id != "";

    none tag in tags {
      tag == "";
    }

    resistances is null || contains(resistances.values, DamageType.fire);
  }
}

type SchemaMarker {
  category: "combat" = "combat";
  version: 1 = 1;
  enabled: true = true;
}

type ItemReward {
  item: Item;
  count: int = 1;

  check {
    count > 0;
    item.id != "";
  }
}

type CurrencyReward {
  amount: int;

  check {
    amount > 0;
  }
}

type Reward = ItemReward | CurrencyReward;

type DropTable {
  rewards: [Reward];
  weights: [int];
  tags: [string] = [];

  check {
    len(rewards) == len(weights);
    len(rewards) > 0;
    sum(weights) == 100;
    min(weights) >= 0;
    max(weights) <= 100;
    unique(tags);

    any reward in rewards {
      reward is CurrencyReward && reward.amount > 0;
    }

    all reward in rewards {
      reward is Reward;
    }

    none tag in tags {
      tag == "";
    }
  }
}

type Monster {
  id: string;
  display: string;
  rarity: Rarity;
  stats: Stats;
  drops: DropTable;
  primary_drop: Reward;
  highlighted_drop: Reward;
  optional_boss_drop: Item?;
  resistances: {DamageType: float};

  check {
    id != "";
    display != "";
    stats.hp > 0;
    rarity >= Rarity.common;
    primary_drop is Reward;
    highlighted_drop is Reward;
    optional_boss_drop is null || optional_boss_drop.rarity >= Rarity.rare;
    contains(resistances, DamageType.fire);

    all entry in resistances {
      entry.value >= 0.0;
      entry.value <= 1.0;
    }
  }
}
```

`monsters.cfd`（ModuleId = `"data/monsters"`）：

```cfd
shared_stats: Stats = {
  hp: 30,
  attack: 5,
  speed: 1.25,
  flags: 1,
};

fire_resists: {DamageType: float} = dict{
  DamageType.fire: 0.25,
  DamageType.ice: 0.0,
};

schema_marker: SchemaMarker = {};

potion: Item = {
  id: "potion",
  rarity: Rarity.rare,
  tags: ["consumable", "healing"],
  resistances: null,
};

coin_reward: CurrencyReward = {
  amount: 25,
};

loot: DropTable = {
  rewards: [
    ItemReward {
      item: potion,
      count: 1,
    },
    coin_reward,
  ],
  weights: [40, 60],
  tags: ["starter", "forest"],
};

slime: Monster = {
  id: "slime",
  display: "Green Slime",
  rarity: Rarity.common,
  stats: shared_stats,
  drops: loot,
  primary_drop: loot.rewards[0],
  highlighted_drop: primary_drop,
  optional_boss_drop: null,
  resistances: fire_resists,
};

goblin: Monster = {
  id: "goblin",
  display: "Cave Goblin",
  rarity: Rarity.rare,
  stats: shared_stats,
  drops: loot,
  primary_drop: highlighted_drop,
  highlighted_drop: drops.rewards[1],
  optional_boss_drop: potion,
  resistances: dict{
    DamageType.physical: 0.10,
    DamageType.fire: 0.20,
  },
};

monsters: [Monster] = [slime, goblin];
first_reward: Reward = loot.rewards[0];

cycle_a = { name: "a"; next: cycle_b; };
cycle_b = { name: "b"; next: cycle_a; };

check {
  slime.stats.hp == goblin.stats.hp;
  first_reward is ItemReward;
  Rarity.rare > Rarity.common;

  all monster in monsters {
    monster.stats.hp > 0;
    contains(monster.resistances, DamageType.fire);
  }
}
```

## Loader 接口

loader 本身无 I/O，路径解析和文件读取由宿主程序负责。

**双容器模型**：宿主使用 `CftContainer` 注册所有 `.cft` 模块，建立全局类型表；再使用 `CfdContainer`（持有 `CftContainer`）注册所有 `.cfd` 模块并构建数据。

### CftContainer 接口

```
cft_container.add_module(id, source)
```

注册并解析一个 `.cft` 源文本。`id` 为宿主指定的模块标识。同一 `id` 重复注册，或任意 `.cft` 模块中出现与已注册 `type`/`enum` 同名的定义，立即报错。

```
cft_container.schema(id) -> SchemaModule
```

返回指定模块的类型/枚举定义集合，用于 schema 反射和代码生成。

```
cft_container.resolve_type(name) -> (ModuleId, SchemaType)
```

在全局类型表中查找指定名称的 `type`，找不到返回 None。

```
cft_container.resolve_enum(name) -> (ModuleId, SchemaEnum)
```

在全局类型表中查找指定名称的 `enum`，找不到返回 None。

### CfdContainer 接口

```
cfd_container.add_module(id, source)
```

注册并解析一个 `.cfd` 源文本。同一 `id` 重复注册是错误。

```
cfd_container.build_all() -> CfdResult
```

对 container 中所有已注册 `.cfd` 模块执行结构校验和对象图构建。使用 `CftContainer` 中的全局类型表解析类型标注，使用跨模块限定名解析数据引用。

`build_all()` 内置结构校验：必填字段检查、类型匹配、默认值填充、多余字段检查。结构不合法时报告错误，不返回对象图。

`build_all()` 的处理顺序：

1. 建立每个模块的顶层数据节点符号表。
2. 校验同名数据节点、未知类型引用、未知跨模块数据引用。
3. 为所有顶层命名对象、数组和字典创建占位节点，支持前向引用和循环引用。
4. 解析并连接所有数据引用边，包括跨模块限定名引用。
5. 执行结构校验：必填字段、多余字段、字段类型、数组/字典元素类型、枚举类型。
6. 填充默认值；默认值按值复制，不共享 identity。
7. 返回对象图。若任一步出现结构错误，报告错误且不返回对象图。

```
cfd_container.check(result) -> [CheckError]
```

对 `build_all()` 返回的对象图执行 `check` 块校验，返回错误列表。

### Rust reference API

模块标识使用强类型：

```rust
pub struct ModuleId(String);
```

`CftContainer` API：

```rust
impl CftContainer {
    pub fn new() -> Self;

    pub fn add_module(
        &mut self,
        id: ModuleId,
        source: impl Into<String>,
    ) -> Result<(), CftParseErrors>;

    pub fn schema(&self, id: &ModuleId) -> Option<CftSchemaModule>;
    pub fn resolve_type(&self, name: &str) -> Option<(ModuleId, CftSchemaType)>;
    pub fn resolve_enum(&self, name: &str) -> Option<(ModuleId, CftSchemaEnum)>;
}
```

`CfdContainer` API：

```rust
impl CfdContainer {
    pub fn new(type_ctx: CftContainer) -> Self;

    pub fn add_module(
        &mut self,
        id: ModuleId,
        source: impl Into<String>,
    ) -> Result<(), CfdParseErrors>;

    pub fn build_all(&self) -> Result<CfdResult, CfdBuildErrors>;

    pub fn check(&self, result: &CfdResult) -> Vec<CfdCheckError>;
}
```

错误类型：

```rust
pub struct CftParseErrors { pub errors: Vec<CftParseError> }
pub struct CfdParseErrors { pub errors: Vec<CfdParseError> }
pub struct CfdBuildErrors { pub errors: Vec<CfdBuildError> }
```

结果访问：

```rust
impl CfdResult {
    pub fn module(&self, id: &ModuleId) -> Option<&CfdModuleResult>;
    pub fn modules(&self) -> impl Iterator<Item = (&ModuleId, &CfdModuleResult)>;
}

impl CfdModuleResult {
    pub fn get(&self, name: &str) -> Option<CfdValueRef>;
    pub fn values(&self) -> impl Iterator<Item = (&str, CfdValueRef)>;
}
```

### check 校验接口（v2）

```
container.check(result) -> [CheckError]
```

对 `build()` 返回的对象图执行 `check` 块校验。返回错误列表而非抛异常，方便编辑器等工具场景一次性展示全部校验错误。

`check()` 只在 `build()` 成功后执行。`check()` 不修改对象图，不填充默认值，不重新执行结构校验。`type` 内 `check` 以每个已构建对象为上下文执行；顶层 `check` 以所在模块的顶层命名节点为上下文执行。

## CLI

CLI 作为独立 crate `coflow-cfc-cli` 实现，二进制名为 `cfc`。CLI 扫描指定目录，按相对路径（去掉扩展名）作为 ModuleId 批量注册所有 `.cft` 和 `.cfd` 文件，执行构建和校验。

```
cfc check <dir>           # 注册目录下所有 .cft / .cfd，执行 build_all 和 check
cfc get <dir> <module> <path>   # 构建后读取指定模块中的值路径
cfc type <name>           # 输出全局类型表中的 type 或 enum 定义
```

`check` 失败时返回非零退出码，并按阶段报告 parse/build/check 错误。`build_all()` 阶段会区分合法的 identity 引用环和非法的求值依赖环：对象、数组、字典命名节点可以形成引用环；标量、路径或字段求值形成的环会报 `Cycle` 错误。

`get` 的路径是有限路径求值，必须得到确定值，否则报错。第一版支持顶层数据名、对象字段和数字索引：

```
cfc get data slime.stats.hp
cfc get data data/monsters.monsters[0].drops[1]
```

输出使用保留 identity 的 JSON-like 图格式；对象、数组、字典包含 `$id`，重复引用和环用 `$ref` 表示，因此不会无限展开。

`type` 支持全局类型表中所有已注册的 `type` 和 `enum`：

```
cfc type Monster
cfc type Item
cfc type Rarity
```

## 错误阶段

错误按阶段划分：

- `CftContainer.add_module` 阶段：词法错误、语法错误、段落错误（含数据定义）、重复 ModuleId、全局 type/enum 重名。
- `CfdContainer.add_module` 阶段：词法错误、语法错误、段落错误（含类型定义）、重复 ModuleId。
- `CfdContainer.build_all` 阶段：重复数据节点名、未知类型引用、未知跨模块 ModuleId、未知数据节点名、类型不匹配、缺少必填字段、多余字段、无法推断空数组或空字典类型、枚举重复值、非法求值循环。
- `check` 阶段：条件表达式为假、`check` 表达式类型错误、`check` 中访问不存在的字段或越界数组下标。

`add_module` 和 `build_all` 的错误会阻止返回可用对象图。`check` 错误不改变对象图，返回错误列表供宿主或编辑器展示。

## v1 / v2 边界

| 能力 | v1 | v2 |
|------|:--:|:--:|
| 结构校验（必填字段、类型匹配、默认值填充、多余字段） | ✓ | |
| `.cft` / `.cfd` 双格式拆分 | ✓ | |
| 全局类型命名空间（无 `use`） | ✓ | |
| 跨模块 `moduleid.name` 限定名数据引用 | ✓ | |
| `CftContainer` + `CfdContainer` 双容器 API | ✓ | |
| CLI `check` / `fmt` | | 后续 |
| `type` 内 `check` 块语义校验 | | ✓ |
| 顶层 `check` 块 | | ✓ |
| `CfdContainer.check()` | | ✓ |
