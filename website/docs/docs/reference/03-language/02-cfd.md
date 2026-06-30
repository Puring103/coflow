# CFD 语法参考

CFD（Coflow Data File，`.cfd`）是 coflow 的文本数据文件格式，用来编写 CFT schema 定义下的配置记录。

它适合承载表格不容易表达的数据：嵌套对象、数组、字典、多态对象、路径引用和覆盖模板。Excel / CSV 更适合大量同构记录；CFD 更适合结构复杂、层级较深、需要手写维护的配置。两者最终都会进入同一个 DataModel，因此可以互相引用。

下面是一个简单 CFD 文件：

```text
Item {
  sword_fire {
    name: "Fire Sword",
    element: Fire,
    price: 100,
    tags: ["weapon", "melee"],
  }
}

basic_monster: Monster {
  name: "Training Dummy",
  stats: { hp: 100, attack: 5 },
  drop: {
    rewards: [
      ItemReward { item: &sword_fire, count: 1 },
      CurrencyReward { amount: 10 },
    ],
  },
}
```

## 文件与注释

CFD 文件只保存数据，不声明 schema。字段、类型、枚举、默认值、引用规则和 check 规则来自 CFT。

注释使用 `#`：

```text
# 整行注释
sword_fire: Item {
  name: "Fire Sword", # 行尾注释
}
```

字段、数组元素和字典条目使用 `,` 分隔，允许尾逗号。

```text
tags: ["weapon", "melee",]
```

## 记录

记录是 CFD 的顶层数据单元。每条记录都有 record key 和 CFT 类型。

```text
sword_01: Item {
  name: "Iron Sword",
  price: 100,
}
```

规则：

- `sword_01` 是 record key。
- `Item` 是 CFT 类型名。
- record key 承担 `id` 语义，不要在顶层记录块里再写 `id` 字段。
- 记录块由 `{ ... }` 包裹，结束后不需要 `;`。
- 记录里的字段名必须来自目标 CFT type，不能随意添加 schema 外字段。

### 分组记录

同类型记录可以放在同一个类型分组下，减少重复类型名：

```text
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

这里的 `sword_fire` 和 `staff_ice` 都是 `Item` 记录。

### 多态分组

如果分组类型是抽象类型，分组内的记录需要显式写出具体子类型：

```text
Reward {
  sword_reward: ItemReward {
    item: &sword_fire,
    count: 1,
  }

  coin_reward: CurrencyReward {
    amount: 50,
  }
}
```

`Reward` 是统一的分组入口，`ItemReward` 和 `CurrencyReward` 是实际实例化的 CFT 子类型。

## 字段值

CFD 是 schema-guided 解析：同一段文本会按照 CFT 字段类型解释。

```text
monster_01: Monster {
  name: "Slime",
  level: 3,
  boss: false,
  stats: { hp: 100, attack: 20 },
  tags: ["early", "forest"],
  weaknesses: { Fire: 1.25, Ice: 1.0 },
}
```

### 标量

常见标量按目标字段类型解析：

| 写法 | 目标类型 | 说明 |
| --- | --- | --- |
| `100` | `int` / `float` | 整数或浮点数字面量 |
| `1.25` | `float` | 浮点数字面量 |
| `true` / `false` | `bool` | 布尔值 |
| `"Fire Sword"` | `string` | 双引号字符串 |
| `Fire Sword` | `string` | 简单裸字符串 |
| `null` | `T?` | 只允许用于 nullable 字段 |

字符串建议优先使用双引号。裸字符串适合简单文本，遇到空格、标点、转义需求或容易和枚举/关键字混淆时，使用双引号更清晰。

### 枚举

枚举值可以只写变体名，也可以写完整枚举名：

```text
element: Fire
element: Element.Fire
```

当上下文清楚时，短写法更简洁；当多个枚举有相同变体名时，完整写法更明确。

### 数组

数组使用 `[...]`，元素之间用 `,` 分隔：

```text
tags: ["weapon", "melee"]
```

对象数组可以包含内联对象、记录引用或多态对象：

```text
rewards: [
  @Reward.sword_reward,
  ItemReward { item: &sword_fire, count: 1 },
  CurrencyReward { amount: 10 },
]
```

### 对象

对象字段可以写内联对象：

```text
stats: {
  hp: 100,
  attack: 20,
}
```

如果字段类型是抽象父类型，需要写具体子类型：

```text
reward: ItemReward {
  item: &sword_fire,
  count: 1,
}
```

### 字典

字典使用 `{ key: value }`：

```text
weaknesses: {
  Fire: 1.25,
  Ice: 1.0,
}
```

字典 key 类型由 CFT 字段类型决定。常见 key 类型是 `string`、`int` 或 enum。

## 引用

引用用于让一条记录或对象指向另一条记录，也可以指向某条记录内部的字段、数组元素或字典条目。

```text
item: &sword_fire
item: @Item.sword_fire
first_reward: @DropTable.default_drops.rewards[0]
weakness_hint: @Monster.basic_monster.weaknesses[Fire]
```

### `&key`

`&key` 是直接记录引用简写：

```text
featured_item: &sword_fire
```

规则：

- 只能用于对象字段。
- 目标类型来自 CFT 字段类型。
- 只能引用记录本身，不支持 `.field`、`[index]` 这类路径访问。

### `@Type.key`

`@Type.key` 是显式 typed record reference：

```text
featured_item: @Item.sword_fire
monster: @Monster.basic_monster
```

`Type` 是 CFT 类型名，`key` 是目标 record key。显式写法更适合跨类型、数组元素、字典值和容易产生歧义的场景。

### 路径引用

路径引用可以访问记录内部的字段、数组元素或字典条目：

```text
first_reward: @DropTable.default_drops.rewards[0]
featured_item: @ItemReward.sword_reward.item
weakness_hint: @Monster.basic_monster.weaknesses[Fire]
label: @TextTable.main.labels["start"]
```

常见路径形式：

| 写法 | 说明 |
| --- | --- |
| `.field` | 访问对象字段 |
| `[0]` | 访问数组元素 |
| `[Fire]` | 访问 enum key 字典条目 |
| `["start"]` | 访问 string key 字典条目 |

引用会在 DataModel 阶段解析和检查：目标必须存在，路径必须合法，最终值也必须能赋给目标字段类型。子类可以赋给父类字段，父类不能直接赋给更窄的子类字段。

裸 record key 不会被当成对象引用：

```text
# 错误：对象字段中不要只写 key
featured_item: sword_fire

# 正确
featured_item: &sword_fire
featured_item: @Item.sword_fire
```

### `@ref` 与 `@inline`

CFT 对象字段默认既可以写记录引用，也可以写内联对象。

```text
type Drop {
  item: Item;
  reward: Reward;
}
```

如果字段在 CFT 中标了 `@ref`，CFD 必须写引用：

```text
@ref
item: Item;
```

```text
item: @Item.sword_fire
```

如果字段在 CFT 中标了 `@inline`，CFD 必须写内联对象：

```text
@inline
reward: Reward;
```

```text
reward: ItemReward {
  item: &sword_fire,
  count: 1,
}
```

`@ref` / `@inline` 用在数组或字典字段上时，会约束数组元素或字典 value。

## 覆盖

CFD 支持 `...source` 覆盖语法，用于复用已有对象或字典，再局部改写字段。

```text
elite_monster: Monster {
  ...@Monster.basic_monster,
  name: "Elite Training Dummy",
  stats: {
    ...@Monster.basic_monster.stats,
    hp: 250,
  },
  weaknesses: {
    ...@Monster.basic_monster.weaknesses,
    Ice: 1.5,
  },
}
```

规则：

- spread 按出现顺序合并。
- 后面的 spread 覆盖前面的 spread。
- 本地字段或本地字典条目覆盖所有 spread 来源。
- 对象 spread 的来源必须是可赋值对象。
- 字典 spread 的来源必须与目标字典类型一致。
- spread 来源可以是内联对象、内联字典、`&key`、`@Type.key` 或路径引用。

对象字段和字典都可以继续嵌套 spread：

```text
elite_drop: DropTable {
  ...@DropTable.base_drop,
  weights: {
    ...@DropTable.base_drop.weights,
    Fire: 20,
  },
}
```

## 和 CFT 的关系

CFD 只描述数据值，具体语义由 CFT 决定：

- 顶层记录类型必须是 CFT 中存在的 `type`。
- 字段名必须来自目标 type 或其父类。
- 字段值会按照 CFT 字段类型解析。
- 未填写字段会使用 CFT 默认值。
- 对象引用会按照 CFT 继承关系检查可赋值性。
- `check` 块会在对象构建、默认值填充和引用解析后执行。

因此，修改 CFT 字段类型、默认值、继承关系或 `@ref` / `@inline` 注解，都可能影响 CFD 文件是否仍然通过检查。

## 和表格数据源的关系

Excel / CSV 的一行等价于 CFD 的一条顶层记录；表格里的 `id` 列等价于 CFD 的 record key。

```text
shop_01: Shop {
  featured_item: @Item.sword_fire,
}
```

只要 `Item.sword_fire` 最终由任意数据源加载到同一个 DataModel，CFD 就可以引用它。目标记录可以来自 CFD，也可以来自 Excel、CSV 或其他 Provider。

## 完整示例

```text
Item {
  sword_fire {
    name: "Fire Sword",
    element: Fire,
    price: 100,
    tags: ["weapon", "melee"],
  }

  staff_ice {
    name: "Ice Staff",
    element: Ice,
    price: 150,
    tags: ["weapon", "magic"],
  }
}

basic_monster: Monster {
  name: "Training Dummy",
  stats: { hp: 100, attack: 5 },
  weaknesses: { Fire: 1.25, Ice: 1.0 },
  drop: {
    rewards: [
      ItemReward { item: &sword_fire, count: 1 },
      CurrencyReward { amount: 10 },
    ],
    weights: { Fire: 10, Ice: 5 },
  },
}

Reward {
  sword_reward: ItemReward {
    item: &sword_fire,
    count: 1,
  }

  coin_reward: CurrencyReward {
    amount: 50,
  }
}

default_drops: DropTable {
  rewards: [
    @Reward.sword_reward,
    @Reward.coin_reward,
  ],
  weights: {
    Fire: 70,
    Ice: 30,
  },
}

elite_monster: Monster {
  ...@Monster.basic_monster,
  name: "Elite Training Dummy",
  stats: {
    ...@Monster.basic_monster.stats,
    hp: 250,
  },
  weaknesses: {
    ...@Monster.basic_monster.weaknesses,
    Ice: 1.5,
  },
}

fire_encounter: Encounter {
  monster: &elite_monster,
  first_reward: @DropTable.default_drops.rewards[0],
  featured_item: @ItemReward.sword_reward.item,
  weakness_hint: @Monster.elite_monster.weaknesses[Ice],
}
```

## 常见错误

| 错误写法 | 为什么错 | 推荐做法 |
| --- | --- | --- |
| `sword_01 Item { ... }` | 记录 key 和类型之间缺少 `:` | 写 `sword_01: Item { ... }` |
| 在顶层记录里写 `id: "sword_01"` | record key 已承担 `id` 语义 | 把 key 写在记录开头 |
| `featured_item: sword_fire` | 裸 key 不会被解析为对象引用 | 写 `&sword_fire` 或 `@Item.sword_fire` |
| `Reward { r1 { ... } }` 且 `Reward` 是抽象类型 | 抽象类型不能直接实例化 | 写 `r1: ItemReward { ... }` |
| `@Monster.basic.stats[0]` | 路径访问方式和目标字段类型不匹配 | 按字段、数组、字典实际类型写路径 |
| `...@Item.sword_fire` spread 到 `Stats` | spread 来源类型不能赋给目标对象类型 | 使用同类型或可赋值对象来源 |
| `name: null` 且 `name` 不是 nullable | `null` 只能赋给 `T?` | 改字段类型为 `string?` 或提供字符串 |
| `element: Flame` | enum variant 不存在 | 检查 CFT enum 定义并写正确 variant |
