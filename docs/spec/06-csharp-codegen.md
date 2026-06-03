# C# 代码生成规格

**依赖文档**：[01-cft.md](01-cft.md)、[02-data-model.md](02-data-model.md)、[05-json-export.md](05-json-export.md)

代码生成以 `CftContainer`（全局类型表）为输入，产出两类文件：

- **类型定义文件**：每个 CFT 类型对应的 C# class / struct / enum
- **数据库文件**：强类型的配置数据库类，包含加载器和查询 API

所有生成类均为 `partial`，允许用户在独立文件中扩展生成代码。

---

## 目录

1. [命名约定](#1-命名约定)
2. [enum 生成](#2-enum-生成)
3. [type 生成](#3-type-生成)
4. [数据库类生成](#4-数据库类生成)
5. [加载器生成](#5-加载器生成)
6. [错误处理](#6-错误处理)
7. [完整示例](#7-完整示例)

---

## 1. 命名约定

| CFT | C# |
|-----|----|
| 类型名 | 保持原名（PascalCase） |
| 字段名 | PascalCase（`snake_case` → `SnakeCase`） |
| 枚举变体名 | 保持原名（PascalCase） |
| 数据库类 | `{命名空间}Config`，如 `GameConfig` |
| `@display("text")` | 生成 `/// <summary>text</summary>` XML 注释，应用于 type、enum、field、enum variant |
| `@deprecated` | 生成 `[Obsolete]`，应用于 type、enum、field、enum variant；子类不自动继承父类的 `[Obsolete]` |

---

## 2. enum 生成

### 普通枚举

```cft
enum Rarity {
  Common = 0,
  Rare   = 10,
  Epic   = 20,
}
```

生成：

```csharp
public enum Rarity
{
    Common = 0,
    Rare   = 10,
    Epic   = 20,
}
```

### `@flag` 枚举

```cft
@flag
enum Permission {
  Read    = 1,
  Write   = 2,
  Execute = 4,
}
```

生成：

```csharp
[Flags]
public enum Permission
{
    Read    = 1,
    Write   = 2,
    Execute = 4,
}
```

### `@display` 和 `@deprecated`

```cft
@display("物品稀有度")
enum Rarity {
  @deprecated
  @display("普通（已废弃）")
  Common = 0,
  Rare = 10,
}
```

生成：

```csharp
/// <summary>物品稀有度</summary>
public enum Rarity
{
    /// <summary>普通（已废弃）</summary>
    [Obsolete]
    Common = 0,
    Rare   = 10,
}
```

---

## 3. type 生成

### 普通 type

```cft
type Stats {
  hp:     int;
  attack: int;
  speed:  float = 1.0;
}
```

生成：

```csharp
public partial class Stats
{
    public long Hp { get; init; }
    public long Attack { get; init; }
    public float Speed { get; init; } = 1.0f;
}
```

### `abstract type`

```cft
abstract type Reward {
  id: string;
}
```

生成：

```csharp
public abstract partial class Reward
{
    public string Id { get; init; } = "";
}
```

### 继承

```cft
sealed type CurrencyReward : Reward {
  amount: int;
}
```

生成：

```csharp
public sealed partial class CurrencyReward : Reward
{
    public long Amount { get; init; }
}
```

### `@struct` + `sealed type`

```cft
@struct
sealed type Vector2 {
  x: float;
  y: float;
}
```

生成：

```csharp
public partial struct Vector2
{
    public float X { get; init; }
    public float Y { get; init; }
}
```

### `@deprecated` type 和 field

```cft
@deprecated
type OldReward { id: string; }

type Item {
  @deprecated
  @display("旧价格")
  old_price: int = 0;
}
```

生成：

```csharp
[Obsolete]
public partial class OldReward
{
    public string Id { get; init; } = "";
}

public partial class Item
{
    /// <summary>旧价格</summary>
    [Obsolete]
    public long OldPrice { get; init; } = 0;
}
```

### nullable 字段

```cft
type Drop {
  item:   Item? = null;
  backup: Item?;
}
```

生成：

```csharp
public partial class Drop
{
    public Item? Item { get; init; } = null;
    public Item? Backup { get; init; }
}
```

### 数组和字典字段

```cft
type Monster {
  tags:        [string] = [];
  resistances: {DamageType: float};
}
```

生成：

```csharp
public partial class Monster
{
    public IReadOnlyList<string> Tags { get; init; } = [];
    public IReadOnlyDictionary<DamageType, float> Resistances { get; init; }
        = new Dictionary<DamageType, float>();
}
```

### `@ref` 字段

`@ref` 字段生成两个属性：原始 ID 和解析后的引用：

```cft
type ItemReward : Reward {
  @ref(Item)
  item_id: string;
  count: int = 1;
}
```

生成：

```csharp
public partial class ItemReward : Reward
{
    public string ItemId { get; init; } = "";           // 原始 ID
    public Item Item { get; internal set; } = null!;    // 解析后的引用，由加载器填充
    public long Count { get; init; } = 1;
}
```

`@ref` 目标是 `abstract type` 时，引用属性类型为父类：

```cft
type Quest {
  @ref(Reward)
  reward_id: string;
}
```

生成：

```csharp
public partial class Quest
{
    public string RewardId { get; init; } = "";
    public Reward Reward { get; internal set; } = null!;
}
```

### 默认值生成规则

| CFT 默认值 | C# 生成 |
|-----------|---------|
| `0` | `= 0` |
| `1.0` | `= 1.0f` |
| `""` | `= ""` |
| `true`/`false` | `= true`/`= false` |
| `[]` | `= []` |
| `{}` | `= new Dictionary<K, V>()` |
| 枚举值 | `= Rarity.Common` |
| 无默认值 | 不生成初始化 |

---

## 4. 数据库类生成

数据库类聚合所有 table，提供强类型访问和查询 API：

```csharp
public partial class GameConfig
{
    // 每个 table 对应一个 IReadOnlyList 属性
    public IReadOnlyList<Item> Items { get; }
    public IReadOnlyList<Monster> Monsters { get; }

    // @id 字段生成按主键查找的方法，返回 null 表示不存在
    public Item? FindItem(string id) => _itemIndex.GetValueOrDefault(id);
    public Monster? FindMonster(string id) => _monsterIndex.GetValueOrDefault(id);

    // @index 字段生成按索引查询的方法，始终返回列表
    // nullable 字段的 null 值不加入索引
    public IReadOnlyList<Monster> GetMonstersByRarity(Rarity rarity)
        => _monstersByRarity.TryGetValue(rarity, out var list) ? list : [];

    private readonly Dictionary<string, Item> _itemIndex;
    private readonly Dictionary<string, Monster> _monsterIndex;
    private readonly Dictionary<Rarity, List<Monster>> _monstersByRarity;
}
```

---

## 5. 加载器生成

加载器从 JSON 文件读取数据，构造强类型对象，解析 `@ref` 引用。失败时抛出 `CftLoadException`：

```csharp
public partial class GameConfig
{
    public static GameConfig Load(string jsonPath)
    {
        var json = File.ReadAllText(jsonPath);
        return Load(JsonDocument.Parse(json));
    }

    public static GameConfig Load(JsonDocument doc)
    {
        var root = doc.RootElement;

        // 第一遍：构造所有对象（@ref 字段只填原始 ID）
        var items    = LoadItems(root.GetProperty("Item"));
        var monsters = LoadMonsters(root.GetProperty("Monster"));

        // 建立主键索引
        var itemIndex    = items.ToDictionary(x => x.Id);
        var monsterIndex = monsters.ToDictionary(x => x.Id);

        // 第二遍：解析 @ref 引用
        foreach (var reward in monsters
            .SelectMany(m => m.Drops.Rewards)
            .OfType<ItemReward>())
        {
            if (!itemIndex.TryGetValue(reward.ItemId, out var item))
                throw new CftLoadException($"@ref 解析失败：Item[{reward.ItemId}] 不存在",
                    fieldPath: $"ItemReward.item_id");
            reward.Item = item;
        }

        // 建立 @index 索引
        var monstersByRarity = monsters
            .GroupBy(m => m.Rarity)
            .ToDictionary(g => g.Key, g => g.ToList());

        return new GameConfig(items, monsters, itemIndex, monsterIndex, monstersByRarity);
    }
}
```

**多态对象的 `$type` 分发**：每个多态字段生成对应的分发方法：

```csharp
static Reward LoadReward(JsonElement el)
{
    var typeName = el.GetProperty("$type").GetString()
        ?? throw new CftLoadException("多态对象缺少 $type 字段", fieldPath: "reward");

    return typeName switch
    {
        "CurrencyReward" => LoadCurrencyReward(el),
        "ItemReward"     => LoadItemReward(el),
        _ => throw new CftLoadException($"未知类型 {typeName}", fieldPath: "reward.$type")
    };
}
```

---

## 6. 错误处理

加载器失败时抛出 `CftLoadException`，包含字段路径和详细信息：

```csharp
public sealed class CftLoadException : Exception
{
    /// <summary>字段路径，如 "Monster[3].drops.rewards[1].item_id"</summary>
    public string FieldPath { get; }

    /// <summary>期望的类型或值描述</summary>
    public string? Expected { get; }

    /// <summary>实际遇到的 JSON 内容</summary>
    public string? Actual { get; }

    public CftLoadException(string message, string fieldPath,
        string? expected = null, string? actual = null)
        : base(message)
    {
        FieldPath = fieldPath;
        Expected  = expected;
        Actual    = actual;
    }
}
```

触发 `CftLoadException` 的情况：

| 情况 | 说明 |
|------|------|
| JSON 字段缺失且无默认值 | 必填字段在 JSON 中不存在 |
| JSON 字段值类型不匹配 | 期望 number 但得到 string 等 |
| `$type` 字段缺失 | 多态字段缺少类型标记 |
| `$type` 值不是合法子类 | 类型名不在继承树中 |
| `@ref` 目标 ID 不存在 | 外键指向不存在的记录 |

---

## 7. 完整示例

CFT 输入：

```cft
enum Rarity { Common = 0, Rare = 10, Epic = 20, }

type Stats { hp: int; attack: int; speed: float = 1.0; }

abstract type Reward { id: string; }
sealed type CurrencyReward : Reward { amount: int; }
sealed type ItemReward : Reward {
  @ref(Item)
  item_id: string;
  count: int = 1;
}

@display("物品")
type Item {
  @id
  id: string;

  @display("名称")
  name: string;

  @index
  rarity: Rarity = Rarity.Common;
}

type Monster {
  @id
  id: string;

  @index
  rarity: Rarity;

  level: int;
  stats: Stats;
}
```

生成的类型定义：

```csharp
public enum Rarity
{
    Common = 0,
    Rare   = 10,
    Epic   = 20,
}

public partial class Stats
{
    public long Hp { get; init; }
    public long Attack { get; init; }
    public float Speed { get; init; } = 1.0f;
}

public abstract partial class Reward
{
    public string Id { get; init; } = "";
}

public sealed partial class CurrencyReward : Reward
{
    public long Amount { get; init; }
}

public sealed partial class ItemReward : Reward
{
    public string ItemId { get; init; } = "";
    public Item Item { get; internal set; } = null!;
    public long Count { get; init; } = 1;
}

/// <summary>物品</summary>
public partial class Item
{
    public string Id { get; init; } = "";
    /// <summary>名称</summary>
    public string Name { get; init; } = "";
    public Rarity Rarity { get; init; } = Rarity.Common;
}

public partial class Monster
{
    public string Id { get; init; } = "";
    public Rarity Rarity { get; init; }
    public long Level { get; init; }
    public Stats Stats { get; init; } = null!;
}
```

生成的数据库类：

```csharp
public partial class GameConfig
{
    public IReadOnlyList<Item> Items { get; }
    public IReadOnlyList<Monster> Monsters { get; }

    public Item? FindItem(string id) => _itemIndex.GetValueOrDefault(id);
    public Monster? FindMonster(string id) => _monsterIndex.GetValueOrDefault(id);

    public IReadOnlyList<Item> GetItemsByRarity(Rarity rarity)
        => _itemsByRarity.TryGetValue(rarity, out var list) ? list : [];

    public IReadOnlyList<Monster> GetMonstersByRarity(Rarity rarity)
        => _monstersByRarity.TryGetValue(rarity, out var list) ? list : [];

    private readonly Dictionary<string, Item> _itemIndex;
    private readonly Dictionary<string, Monster> _monsterIndex;
    private readonly Dictionary<Rarity, List<Item>> _itemsByRarity;
    private readonly Dictionary<Rarity, List<Monster>> _monstersByRarity;

    public static GameConfig Load(string jsonPath) { ... }
}
```
