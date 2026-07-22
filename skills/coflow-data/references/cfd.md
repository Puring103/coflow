# CFD 语法参考

CFD（Coflow Data File，`.cfd`）是 coflow 的文本数据文件格式，用来编写 CFT schema 定义下的配置记录。

它适合承载表格不容易表达的数据：嵌套对象、数组、字典、多态对象、记录引用和覆盖模板。Excel / CSV 更适合大量同构记录；CFD 更适合结构复杂、层级较深、需要手写维护的配置。项目中的不同数据源会合并检查，因此可以互相引用。

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

注释只使用 `#`：

```text
# 整行注释
sword_fire: Item {
  name: "Fire Sword", # 行尾注释
}
```

字段、数组元素和字典条目使用 `,` 分隔，允许尾逗号。
分组记录之间的逗号是可选的。

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
- 记录块由 `{ ... }` 包裹；普通记录字段、数组元素和字典条目使用 `,` 分隔。
- 分组记录之间可以写 `,`，也可以仅用空白或换行分隔。
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

分组记录之间的逗号可选，以下写法等价：

```text
Item {
  sword_fire {
    name: "Fire Sword",
    price: 100,
  },

  staff_ice {
    name: "Ice Staff",
    price: 150,
  },
}
```

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
| `"Fire Sword"` | `string` | 双引号字符串，string 值必须使用这种写法 |
| `null` | `T?` | 只允许用于 nullable 字段 |

字符串必须使用双引号。支持 `\"`、`\\`、`\n`、`\r` 和 `\t` 转义。
数字、布尔值和 enum 值仍使用各自的裸字面量写法；它们不会被当作字符串。

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

对象数组可以包含内联对象或多态对象；如果 CFT 元素类型是 `&Type`，数组元素写 `&key`：

```text
rewards: [
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

引用用于让一条记录或对象指向另一条顶层记录。CFT 字段类型必须写成 `&Type`、`&Type?`、`[&Type]` 或 `{key: &Type}`。

```text
item: &sword_fire
```

### `&key`

`&key` 是唯一的记录引用值语法：

```text
featured_item: &sword_fire
```

规则：

- 只能用于期望类型为 `&Type` 的位置。
- 目标类型来自 CFT 字段类型。
- 只能引用记录本身，不支持 `.field`、`[index]` 这类路径访问。
- `&Reward` 可以引用实际类型为 `Reward` 或其子类的 record；不能引用父类、兄弟类型或无关 type。

加载项目时会检查引用：目标必须存在，目标 record 的实际类型必须能赋给字段声明的引用类型。子类可以赋给父类字段，父类不能直接赋给更窄的子类字段。

裸 record key 不会被当成对象引用：

```text
# 错误：对象字段中不要只写 key
featured_item: sword_fire

# 正确
featured_item: &sword_fire
```

### 引用与内联对象

```text
type Drop {
  item: &Item;
  reward: Reward;
}
```

字段类型为 `&Item` 时，CFD 必须写引用：

```text
item: &sword_fire
```

字段类型为 `Reward` 时，CFD 必须写内联对象。数组和字典会递归应用内层类型。

## 覆盖

CFD 支持 `...source` 覆盖语法，用于复用已有对象或字典，再局部改写字段。

```text
elite_monster: Monster {
  ...&basic_monster,
  name: "Elite Training Dummy",
  stats: { hp: 250, attack: 5 },
}
```

规则：

- spread 按出现顺序合并。
- 后面的 spread 覆盖前面的 spread。
- 本地字段或本地字典条目覆盖所有 spread 来源。
- 对象 spread 的来源必须是可赋值对象，`...&key` 的目标类型来自外层对象上下文。
- 字典 spread 的来源必须与目标字典类型一致。
- spread source 写成 `...&key` 时，source record 的实际类型必须是外层期望类型本身或其子类。

字典可以继续嵌套 spread：

```text
elite_drop: DropTable {
  ...&base_drop,
  weights: {
    ...{Ice: 5},
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
- `&Type` 引用和 `...&key` spread source 会按照 CFT 继承关系检查可赋值性。
- `check` 块会在对象构建、默认值填充和引用解析后执行。

因此，修改 CFT 字段类型、默认值或继承关系，都可能影响 CFD 文件是否仍然通过检查。

## 和表格数据源的关系

Excel / CSV 的一行等价于 CFD 的一条顶层记录；表格里的 `id` 列等价于 CFD 的 record key。

```text
shop_01: Shop {
  featured_item: &sword_fire,
}
```

只要 `Item.sword_fire` 存在于当前项目中，CFD 就可以引用它。目标记录可以来自 CFD，也可以来自 Excel、CSV 或其他数据源。

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
    ItemReward { item: &sword_fire, count: 1 },
    CurrencyReward { amount: 50 },
  ],
  weights: {
    Fire: 70,
    Ice: 30,
  },
}

elite_monster: Monster {
  ...&basic_monster,
  name: "Elite Training Dummy",
  stats: { hp: 250, attack: 5 },
  weaknesses: { Fire: 1.25, Ice: 1.5 },
}

fire_encounter: Encounter {
  monster: &elite_monster,
  featured_item: &sword_fire,
  weakness_hint: 1.5,
}
```

## 常见错误

| 错误写法 | 为什么错 | 推荐做法 |
| --- | --- | --- |
| `sword_01 Item { ... }` | 记录 key 和类型之间缺少 `:` | 写 `sword_01: Item { ... }` |
| 在顶层记录里写 `id: "sword_01"` | record key 已承担 `id` 语义 | 把 key 写在记录开头 |
| `featured_item: sword_fire` | 裸 key 不会被解析为对象引用 | 写 `&sword_fire` |
| `name: Fire Sword` | string 值必须使用引号 | 写 `name: "Fire Sword"` |
| `Reward { r1 { ... } }` 且 `Reward` 是抽象类型 | 抽象类型不能直接实例化 | 写 `r1: ItemReward { ... }` |
| `...&sword_fire` spread 到 `Stats` | spread 来源类型不能赋给目标对象类型 | 使用同类型或可赋值对象来源 |
| `name: null` 且 `name` 不是 nullable | `null` 只能赋给 `T?` | 改字段类型为 `string?` 或提供字符串 |
| `element: Flame` | enum variant 不存在 | 检查 CFT enum 定义并写正确 variant |
