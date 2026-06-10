# JSON 导出格式

**依赖文档**：[02-data-model.md](02-data-model.md)

JSON 导出是 Excel 加载器的数据导出产物，供运行时加载器直接消费。格式以 `DataModel` 为输入，`@ref` 保留原始 ID，由运行时加载器负责解析引用。

JSON exporter 位于 `coflow-exporter-json`，与 MessagePack exporter 共用 `coflow-exporter-core` 的 schema-aware 遍历规则。

---

## 文件结构

JSON 导出产物是一个输出目录。每个 table（对应 CFT 类型名）导出为一个 `<TypeName>.json` 文件，文件内容是该 table 的记录数组，保持数据源顺序：

```text
out/
  Item.json
  Monster.json
  DropTable.json
```

```json
[
  { "id": "sword_01", "name": "铁剑" },
  { "id": "potion_01", "name": "药水" }
]
```

---

## 各类型的编码规则

| CFT 类型 | JSON 表示 | 示例 |
|---------|-----------|------|
| `int` | number | `42` |
| `float` | number | `3.14` |
| `bool` | boolean | `true` |
| `string` | string | `"sword_01"` |
| `null` / nullable 且为 null | null | `null` |
| `enum` | number（底层整数值） | `10`（对应 `Rarity.Rare = 10`） |
| `type`（非多态） | object，无 `$type` | `{ "hp": 100, "attack": 50 }` |
| `type`（多态） | object + `$type` | `{ "$type": "CurrencyReward", "amount": 100 }` |
| `[T]` | array | `[1, 2, 3]` |
| `{K: V}` | object，key 统一转为十进制字符串 | 见下方字典编码规则 |
| `@ref` 字段 | 原始 ID 值（string 或 number） | `"sword_01"` |

所有字段均显式导出，含有默认值的字段也写出，不依赖消费端自行填充默认值。
`float` 在数据模型阶段已保证是有限值，因此导出的 JSON number 不会包含 `NaN` 或 `Infinity`。

---

## 枚举编码

DataModel 内部的 enum value 会携带 enum 类型名；JSON 不重复写出类型名，只导出底层整数值，由字段 schema 决定 enum 类型。

---

## 字典的 key 编码

JSON 对象的 key 必须是字符串，三种 key 类型统一转为字符串，运行时加载器根据 schema 知道 key 类型，不从格式猜测：

| CFT key 类型 | JSON key 示例 | 说明 |
|-------------|--------------|------|
| `string` | `"alice"` | 直接作为 JSON string key |
| `int` | `"1"`、`"42"` | 十进制数字字符串 |
| `enum` | `"1"`、`"10"` | 枚举底层整数值的十进制字符串 |

```json
// {DamageType: float}，DamageType.Fire=1, Ice=2, Physical=0
"resistances": { "1": 0.5, "2": 0.2, "0": 1.0 }

// {int: string}
"names": { "1": "sword", "2": "shield" }

// {string: int}
"scores": { "alice": 10, "bob": 20 }
```

导出前的 DataModel 已保证字典 key 唯一。JSON 文本中如果出现重复 object key，运行时加载器必须按格式错误处理，不允许依赖 JSON parser 的后写覆盖行为。

---

## `$type` 标记规则

字段的**声明类型是父类**（`abstract type` 或有子类的普通 `type`）时，值需要附加 `$type` 字段标记实际类型。字段声明类型是 `sealed type` 或无子类的具体类型时，不需要 `$type`：

```json
// reward 字段类型是 abstract Reward，必须有 $type
"reward": {
  "$type": "CurrencyReward",
  "id": "r1",
  "amount": 100
}

// stats 字段类型是具体的 Stats（无子类），不需要 $type
"stats": {
  "hp": 100,
  "attack": 50,
  "speed": 1.0
}
```

多态数组中每个元素均需 `$type`：

```json
"rewards": [
  { "$type": "CurrencyReward", "id": "r1", "amount": 100 },
  { "$type": "ItemReward", "id": "r2", "item_id": "sword_01", "count": 1 }
]
```

---

## `@ref` 字段

`@ref` 字段导出为原始 ID 值，不内联目标对象，避免数据膨胀和循环引用问题：

```json
// CFT: @ref(Item) item_id: string;
"item_id": "sword_01"
```

运行时加载器根据 ID 和 schema 中 `@ref` 的目标类型解析引用。允许循环引用（加载器两遍设计天然支持）。

---

## 完整示例

`Item.json`：

```json
[
  {
    "id": "sword_01",
    "name": "铁剑",
    "rarity": 10,
    "tags": ["weapon", "melee"]
  },
  {
    "id": "potion_01",
    "name": "药水",
    "rarity": 0,
    "tags": ["consumable"]
  }
]
```

`Monster.json`：

```json
[
  {
    "id": "slime",
    "name": "绿史莱姆",
    "level": 5,
    "rarity": 0,
    "stats": { "hp": 100, "attack": 50, "speed": 1.0 },
    "drops": {
      "rewards": [
        { "$type": "CurrencyReward", "id": "r1", "amount": 10 },
        { "$type": "ItemReward", "id": "r2", "item_id": "potion_01", "count": 1 }
      ],
      "weights": [60, 40]
    },
    "boss_drop": null,
    "resistances": { "0": 1.0, "1": 0.5, "2": 0.2 },
    "skill": null
  }
]
```
