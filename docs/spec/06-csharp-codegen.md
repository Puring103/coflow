# C# 代码生成规格

**依赖文档**：[01-cft.md](01-cft.md)、[02-data-model.md](02-data-model.md)、[05-json-export.md](05-json-export.md)、[08-messagepack-export.md](08-messagepack-export.md)

代码生成以 `CftContainer`（全局类型表）为输入，产出两类文件：

- **类型定义文件**：每个 CFT 类型对应的 C# class / struct / enum
- **数据库文件**：强类型的配置数据库类，包含加载器和查询 API

所有生成类均为 `partial`，允许用户在独立文件中扩展生成代码。

---

## 目录

1. [实现方案](#1-实现方案)
2. [命名约定](#2-命名约定)
3. [enum 生成](#3-enum-生成)
4. [type 生成](#4-type-生成)
5. [数据库类生成](#5-数据库类生成)
6. [加载器生成](#6-加载器生成)
7. [错误处理](#7-错误处理)
8. [完整示例](#8-完整示例)

---

## 1. 实现方案

C# codegen crate 接收已经编译完成的 `CftContainer` 和 C# codegen options，生成 C# 文件。它不读取 `coflow.yaml`，也不负责项目发现、路径解析或 CLI 编排；这些由 project pipeline 和 CLI 层负责。

```yaml
outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Example.Rpg.Config
```

上面的 YAML 是 CLI 项目配置示例：`coflow codegen csharp` 由 project pipeline 读取 `coflow.yaml`、发现并编译 schema、合并命令行选项，然后以 `CftContainer`、`outputs.data.type` 和 codegen options 调用 codegen crate，并把生成文件写入项目配置指定的输出目录。

`outputs.code` 只描述代码生成目标（语言、目录、命名空间等），不提供独立的 data format override。C# codegen 的运行时加载器类型由 `outputs.data.type` 唯一决定：

| `outputs.data.type` | 生成的运行时加载器 |
|---------------------|--------------------|
| `json` | 读取 `<TypeName>.json`，使用 `Newtonsoft.Json` |
| `messagepack` | 读取 `<TypeName>.msgpack`，使用 MessagePack-CSharp 和显式 `MessagePackReader` |

MessagePack loader 不使用 typeless、反射式或动态 resolver 反序列化；生成代码直接调用低层 `MessagePackReader`，以兼容普通 .NET 和 Unity/IL2CPP/AOT。

实现使用 Tera 渲染模板文件，但模板只负责文本展开，不承载 CFT 语义判断。codegen crate 内部流程为：

1. 接收 `CftContainer` 和 C# codegen options
2. 将 `CftContainer` 投影为 C# 专用 codegen model
3. 使用 Tera 模板渲染 `.cs` 文件
4. 返回或写出调用方指定目标中的生成文件

Codegen model 是 C# 视角的数据结构，而不是直接暴露 `CftSchemaType` 给模板：

```rust
struct CsharpProject {
    namespace: String,
    database_class: String,
    enums: Vec<CsharpEnum>,
    types: Vec<CsharpType>,
    database: CsharpDatabase,
}

struct CsharpType {
    name: String,
    kind: CsharpTypeKind, // class / abstract class / sealed class / struct
    parent: Option<String>,
    summary: Option<String>,
    obsolete: bool,
    fields: Vec<CsharpField>,
}

struct CsharpField {
    name: String,        // C# 属性名，如 SkillId
    source_name: String, // CFT 字段名，如 skill_id
    ty: String,          // C# 类型，如 string / long / IReadOnlyList<string>
    default: Option<String>,
    summary: Option<String>,
    obsolete: bool,
    ref_target: Option<String>,
    ref_id_property: Option<String>,
    ref_property: Option<String>,
}
```

Tera 模板只允许做简单的字段遍历、条件输出和命名空间包裹；类型映射、默认值、继承展开、`@ref`、`@id`、`@index`、`@display`、`@deprecated` 等规则必须在 Rust model 构建阶段完成。实现应补充 golden tests 固定复杂 schema 的输出形状。

当前实现位于 `crates/coflow-codegen-csharp`。codegen 根据项目 data format 生成 JSON 或 MessagePack loader；生成出的 C# 运行时代码只依赖对应的数据文件和运行时包，不依赖 CFT parser/compiler。

生成的 C# 是 trusted artifact loader。它只承诺加载官方 Coflow exporter 从已经通过 Rust pipeline 的数据生成的 JSON 或 MessagePack 产物，负责反序列化、构建运行时查找表并解析生成对象的引用。它不承诺为任意手写或损坏的数据文件提供稳定诊断，也不执行 CFT check。

建议文件布局：

```text
crates/coflow-codegen-csharp/
  src/lib.rs
  src/ir.rs
  src/model.rs
  src/schema_view.rs
  src/emit.rs
  src/names.rs
  src/render.rs
  templates/
    enum.cs.tera
    type.cs.tera
    database_json.cs.tera
    database_json_loaders.cs.tera
    database_json_readers.cs.tera
    database_messagepack.cs.tera
    database_messagepack_loaders.cs.tera
    database_messagepack_readers.cs.tera
    database_common_members.cs.tera
    database_common_resolve.cs.tera
    database_common_indexes.cs.tera
    load_exception.cs.tera
```

生成文件按类型拆分：

```text
generated/csharp/
  Rarity.cs
  Item.cs
  Monster.cs
  DropTable.cs
  GameConfig.cs
  CftLoadException.cs
```

第一版必须生成类型定义、枚举、继承、默认值、`@ref` 双属性、`@id` 主键查询和 `@index` 查询 API。加载器根据 `outputs.data.type` 生成 JSON 或 MessagePack 版本，但生成结构必须保持两遍加载：先构造对象和主键索引，再解析 `@ref`。

---

## 2. 命名约定

| CFT | C# |
|-----|----|
| 类型名 | 保持原名（PascalCase） |
| 字段名 | PascalCase（`snake_case` → `SnakeCase`） |
| 枚举变体名 | 保持原名（PascalCase） |
| 数据库类 | `{命名空间}Config`，如 `GameConfig` |
| `@display("text")` | 生成 `/// <summary>text</summary>` XML 注释，应用于 type、enum、field、enum variant |
| `@deprecated` | 生成 `[Obsolete]`，应用于 type、enum、field、enum variant；子类不自动继承父类的 `[Obsolete]` |

---

## 3. enum 生成

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

## 4. type 生成

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

当前 CFT 语义中，`@ref` 字段本身存储目标记录的 `@id` 值，字段类型必须是 `string` 或 `int`，也可以是对应 nullable 形式。JSON 和 MessagePack 导出仍然输出原始 ID 值，运行时加载器负责解析引用。

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

其中：

- `ItemId` 保留原始配置值，用于错误定位、调试、重新导出和与数据文件对应
- `Item` 是解析后的强类型引用，由 `GameConfig.Load` 在第二遍加载时填充

`@ref` 目标是 `abstract type` 或有子类的普通 `type` 时，引用属性类型为声明的目标父类：

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

未来允许增加直接引用写法，但普通对象字段不能自动变成引用：

```cft
type Monster {
  @ref
  skill: Skill;

  @ref
  optional_skill: Skill?;
}
```

这是一种预留语法，不是当前 CFT 必备能力。它的语义仍然是“Excel/导出数据中存目标 ID，运行时解析为对象引用”。普通字段：

```cft
type Monster {
  skill: Skill;
}
```

仍表示内联对象，不表示跨表引用。这样可以避免和 `stats: Stats` 这类值对象字段冲突。

直接引用写法未来生成时仍应保留原始 ID 属性：

```csharp
public string SkillId { get; init; } = "";
public Skill Skill { get; internal set; } = null!;

public string? OptionalSkillId { get; init; }
public Skill? OptionalSkill { get; internal set; }
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

## 5. 数据库类生成

数据库类聚合所有 table，提供强类型访问和查询 API：

`@id` 的核心作用是唯一定位一条记录，生成 `Find{Type}`。`@index` 的核心作用是声明某个字段需要生成“按字段值查记录”的快速查询入口，生成 `Get{Types}By{Field}`。`@index` 不是数据校验规则，也不是 Excel 解析规则；它把运行时常用查询写入 schema，使不同语言的生成器生成一致 API。

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
    public IReadOnlyList<Monster> GetMonstersByRarity(Rarity rarity)
        => _monstersByRarity.TryGetValue(rarity, out var list) ? list : [];

    private readonly Dictionary<string, Item> _itemIndex;
    private readonly Dictionary<string, Monster> _monsterIndex;
    private readonly Dictionary<Rarity, List<Monster>> _monstersByRarity;
}
```

---

## 6. 加载器生成

`coflow codegen csharp` 从项目配置的 `outputs.data.type` 选择运行时加载器。`outputs.data.type: json` 生成 JSON loader；`outputs.data.type: messagepack` 生成 MessagePack loader。`outputs.code` 不提供独立 data format override。

加载过程构造强类型对象，建立 `@id` 和 `@index` 查找表，并在第二遍解析 `@ref` 引用。生成的 C# 加载器是 trusted artifact loader，只支持官方 Coflow exporter 从已经通过 Rust pipeline 的数据生成的 JSON 或 MessagePack；它不承诺对任意手写或损坏数据提供稳定诊断，也不运行 CFT check。失败时抛出 `CftLoadException`。

### JSON loader

JSON loader 从 `coflow export json` 产出的 JSON 目录读取数据：每个 table 一个 `<TypeName>.json` 文件，文件内容是 JSON array。运行时 JSON 库固定为通用 .NET 包 `Newtonsoft.Json`：

```csharp
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

public partial class GameConfig
{
    public static GameConfig Load(string dataDir)
    {
        var items = LoadTable(Path.Combine(dataDir, "Item.json"), "Item", LoadItem);
        var monsters = LoadTable(Path.Combine(dataDir, "Monster.json"), "Monster", LoadMonster);

        var itemIndex = BuildUniqueIndex(items, x => x.Id, "Item", "id");
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

    private static List<T> LoadTable<T>(
        string file,
        string tableName,
        Func<JToken, string, T> loadRow)
    {
        var root = JToken.Parse(
            File.ReadAllText(file),
            new JsonLoadSettings
            {
                DuplicatePropertyNameHandling = DuplicatePropertyNameHandling.Error
            });

        if (root is not JArray array)
            throw new CftLoadException($"table `{tableName}` must be a JSON array", tableName);

        var result = new List<T>();
        for (var i = 0; i < array.Count; i++)
            result.Add(loadRow(array[i], $"{tableName}[{i}]"));
        return result;
    }
}
```

JSON 多态对象的 `$type` 分发：每个多态字段生成对应的分发方法：

```csharp
static Reward LoadRewardPolymorphic(JToken token, string path)
{
    var obj = RequireObject(token, path);
    var typeName = ReadRequired(obj, "$type", path, ReadString);

    return typeName switch
    {
        "CurrencyReward" => LoadCurrencyReward(token, path),
        "ItemReward"     => LoadItemReward(token, path),
        _ => throw new CftLoadException($"unknown polymorphic type `{typeName}`",
            $"{path}.$type", "CurrencyReward or ItemReward", typeName)
    };
}
```

### MessagePack loader

MessagePack loader 从 `coflow export messagepack` 产出的 MessagePack 目录读取数据：每个 table 一个 `<TypeName>.msgpack` 文件，文件内容是裸 MessagePack array，array 中每个元素是 record map。record map 的 key 是 CFT 源字段名；多态对象要求 `$type` 是 map 第一项。

生成代码依赖 MessagePack-CSharp，并使用低层 `MessagePackReader` 显式读取，不使用 typeless API、反射式 resolver 或运行时代码生成 resolver，因此兼容普通 .NET 和 Unity/IL2CPP/AOT。

```csharp
using System.Buffers;
using MessagePack;

public partial class GameConfig
{
    private delegate T MessagePackRowLoader<T>(ref MessagePackReader reader, string path);

    public static GameConfig Load(string dataDir)
    {
        var items = LoadTable(Path.Combine(dataDir, "Item.msgpack"), "Item", LoadItem);
        var monsters = LoadTable(Path.Combine(dataDir, "Monster.msgpack"), "Monster", LoadMonster);

        var itemIndex = BuildUniqueIndex(items, x => x.Id, "Item", "id");

        // 第二遍：解析 @ref 引用；缺失目标会抛 CftLoadException。
        foreach (var reward in monsters
            .SelectMany(m => m.Drops.Rewards)
            .OfType<ItemReward>())
        {
            reward.Item = ResolveRef(itemIndex, reward.ItemId, "ItemReward.item_id", "Item");
        }

        return new GameConfig(items, monsters, itemIndex);
    }

    private static List<T> LoadTable<T>(
        string file,
        string tableName,
        MessagePackRowLoader<T> loadRow)
    {
        var bytes = File.ReadAllBytes(file);
        var reader = new MessagePackReader(new ReadOnlySequence<byte>(bytes));
        var count = ReadArrayHeader(ref reader, tableName);

        var result = new List<T>(count);
        for (var i = 0; i < count; i++)
            result.Add(loadRow(ref reader, $"{tableName}[{i}]"));

        if (!reader.End)
            throw new CftLoadException($"table `{tableName}` MessagePack contains trailing data", tableName);

        return result;
    }
}
```

MessagePack object loader 读取 map header 后逐项读取 string 字段 key，并用 `switch` 分发到生成的字段 reader。未知字段通过 path-aware helper（例如 `SkipValue(ref reader, fieldPath)`）跳过；helper 内部包装底层 skip 调用的异常并转换成带字段路径的 `CftLoadException`。已知字段重复、必填字段缺失、ID 重复、字典 key 重复、`@ref` 目标缺失、`$type` 缺失或未知、MessagePack 类型不匹配，均抛出 `CftLoadException`。

MessagePack 多态对象的 `$type` 分发：loader 先读取 record map 的第一项，要求 key 为 `$type`，再读取实际类型名并分发到对应的 `Load<Type>Body`。这依赖 MessagePack exporter 按规格把 `$type` 写为多态 map 第一项。

---

## 7. 错误处理

加载器失败时抛出 `CftLoadException`，包含字段路径和详细信息：

```csharp
public sealed class CftLoadException : Exception
{
    /// <summary>字段路径，如 "Monster[3].drops.rewards[1].item_id"</summary>
    public string FieldPath { get; }

    /// <summary>期望的类型或值描述</summary>
    public string? Expected { get; }

    /// <summary>实际遇到的数据内容或格式描述</summary>
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

`CftLoadException` 用于定位受信导出产物加载过程中仍可能出现的问题，例如文件缺失、版本不匹配、格式损坏或引用解析失败。它不是任意手写 JSON 或 MessagePack 数据的稳定 validator 契约。

触发 `CftLoadException` 的情况：

| 情况 | 说明 |
|------|------|
| 字段缺失且无默认值 | 必填字段在 JSON object 或 MessagePack map 中不存在 |
| 字段值类型不匹配 | 期望 number/integer/string 等但得到其他类型 |
| `$type` 字段缺失 | 多态字段缺少类型标记 |
| `$type` 值不是合法子类 | 类型名不在继承树中 |
| `@ref` 目标 ID 不存在 | 外键指向不存在的记录 |
| 主键重复 | 同一类型或同一继承树索引中存在重复 ID |
| object/map key 重复 | 字典或对象字段出现重复 key |

---

## 8. 完整示例

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

    public static GameConfig Load(string dataDir) { ... }
}
```
