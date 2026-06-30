# JSON 导出格式

JSON 导出是 Coflow 的文本数据产物，供运行时加载器读取。导出输入是已经通过 schema 编译、数据加载、引用解析和 `check {}` 校验的 DataModel。

## 文件布局

JSON 导出产物是一个目录。每个有记录的非 `abstract` CFT type 导出为一个文件：

```text
generated/data/
  Item.json
  Monster.json
  DropTable.json
```

文件内容是该 type 的 record 数组，记录顺序保持数据源顺序：

```json
[
  { "id": "sword_fire", "name": "Fire Sword" },
  { "id": "staff_ice", "name": "Ice Staff" }
]
```

没有记录的 table 不写空 `[]` 文件。

## 编码规则

| CFT 类型 | JSON 表示 | 示例 |
| --- | --- | --- |
| `int` | number | `42` |
| `float` | number | `3.14` |
| `bool` | boolean | `true` |
| `string` | string | `"sword_fire"` |
| `T?` 且值为 null | null | `null` |
| enum | number，底层整数值 | `10` |
| object | object | `{ "hp": 100, "attack": 20 }` |
| polymorphic object | object + `$type` | `{ "$type": "ItemReward", "count": 1 }` |
| `[T]` | array | `[1, 2, 3]` |
| `{K: V}` | object，key 转字符串 | `{ "1": 0.5 }` |
| 记录引用 | 目标 record key 字符串 | `"sword_fire"` |

每条顶层记录都会导出保留字段 `id`，值来自 record key。所有 CFT 字段都会显式导出，包括使用默认值填充的字段。

## 字段顺序

顶层 record 先写 `id`，再按 CFT schema 的继承展开顺序写字段：父类字段先于子类字段，同一 type 内按声明顺序输出。

这个顺序便于 diff 和人工检查，但消费端不应依赖 JSON object 的字段顺序表达语义。

## 枚举

enum 导出为底层整数值：

```json
{
  "rarity": 10
}
```

运行时 loader 根据 schema 知道该字段对应哪个 enum 类型。

## 字典 key

JSON object key 必须是字符串，因此 Coflow 会把字典 key 统一转成字符串：

| CFT key 类型 | JSON key 示例 |
| --- | --- |
| `string` | `"alice"` |
| `int` | `"1"` |
| enum | `"10"` |

示例：

```json
{
  "weaknesses": {
    "1": 1.25,
    "2": 1.0
  }
}
```

消费端需要根据 schema 把 key 解析回对应类型。

## 多态 `$type`

当字段声明类型是父类，实际值需要带 `$type` 标记：

```json
{
  "reward": {
    "$type": "ItemReward",
    "item": "sword_fire",
    "count": 1
  }
}
```

字段声明类型是具体类型且没有多态需求时，不写 `$type`：

```json
{
  "stats": {
    "hp": 100,
    "attack": 20
  }
}
```

## 引用字段

记录引用导出为目标 record key，不内联目标对象：

```json
{
  "featured_item": "sword_fire"
}
```

这样可以避免重复数据和循环引用膨胀。运行时 loader 根据字段 schema 和 table index 解析 key。

## 输出目录

`coflow export json` 和 `coflow build` 写入 JSON 时，会完整接管输出目录。写入先进入同级 staging 目录，全部成功后再替换目标目录。

不要把手写文件放进 JSON 输出目录。

## 示例

`Item.json`：

```json
[
  {
    "id": "sword_fire",
    "name": "Fire Sword",
    "rarity": 10,
    "tags": ["weapon", "melee"]
  }
]
```

`Monster.json`：

```json
[
  {
    "id": "basic_monster",
    "name": "Training Dummy",
    "stats": { "hp": 100, "attack": 5 },
    "drop": {
      "rewards": [
        { "$type": "ItemReward", "item": "sword_fire", "count": 1 },
        { "$type": "CurrencyReward", "amount": 10 }
      ],
      "weights": { "1": 10, "2": 5 }
    }
  }
]
```
