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

CLI 项目配置把 `outputs.code` 中除 `type`、`dir` 之外的字段作为 C# provider options 传入，其中 `namespace` 用于生成 C# 命名空间。数据库类名由 codegen options 提供，项目管线默认使用 `GameConfig`；底层 Rust API 可通过 `CsharpCodegenOptions::with_database_class` 改写数据库类名，但 `coflow.yaml` 没有对应字段。

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
    declaration: String, // class / abstract class / sealed class / struct
    summary: Option<String>,
    obsolete: bool,
    properties: Vec<CsharpProperty>,
}

struct CsharpProperty {
    visibility: String,      // public / internal
    name: String,            // C# 属性名，如 SkillId
    type_name: String,       // C# 类型，如 string / long / IReadOnlyList<string>
    setter: String,          // set / internal set
    initializer: Option<String>,
    summary: Option<String>,
    obsolete: bool,
}
```

模板只做简单的属性遍历、条件输出和命名空间包裹。类型映射、默认值、
setter 选择、继承展开、记录 key、引用解析、`@keyAsEnum`、`@display`、
`@deprecated` 等生成规则必须在 Rust model 构建阶段完成。

C# codegen 只消费已经通过编译的 schema。golden tests 需要固定复杂 schema
的输出形状，避免模板输入结构变化时悄悄改变生成代码。

当前实现按单一 C# provider 和内置格式模板组织：

- `crates/coflow-codegen-csharp` 负责 C# 类型模型、公共模板、JSON loader 模板、MessagePack loader 模板和渲染流程。
- crate 实现 `coflow-api::CodeGenerator`，provider id 为 `csharp`。

CLI 通过 builtin `ProviderRegistry` 调用 C# codegen provider，并把项目 data format 作为 provider context 传入。生成出的 C# 运行时代码只依赖对应的数据文件和运行时包，不依赖 CFT parser/compiler。

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
    database_common_members.cs.tera
    database_common_resolve.cs.tera
    database_common_indexes.cs.tera
    load_exception.cs.tera
  templates/
    json/
      database_json.cs.tera
      database_json_loaders.cs.tera
      database_json_readers.cs.tera
    messagepack/
      database_messagepack.cs.tera
      database_messagepack_loaders.cs.tera
      database_messagepack_readers.cs.tera
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

项目管线负责把 codegen crate 返回的文件落盘。C# 输出目录由 Coflow 完全接管；
成功写入时，pipeline 先把完整 `.cs` 产物写入同级临时 staging 目录，再用该目录
替换配置的输出目录。输出目录内旧 `.cs`、sidecar 文件、子目录和人工文件均不会
保留。不要把手写代码放进 `outputs.code.dir`。如果 codegen preflight 或 artifact
preflight 产生诊断，则不会读取/写入 lockfile，也不会替换输出目录。

当前生成器负责生成类型定义、枚举、继承、默认值、每个 table 的 `Id`
属性、按 key 查询 API，以及 object 字段的引用解析。加载器根据
`outputs.data.type` 生成 JSON 或 MessagePack 版本，但生成结构必须保持两遍加载：先构造对象和 key 索引，再解析引用。

### 目标约束

C# target 在生成前必须校验生成物契约：

- CFT `float` 固定映射为 C# `double`；JSON 和 MessagePack loader 均读取 64 位浮点值，不降为 `float` / `Single`。
- CFT 类型名、enum 名、enum variant 名和字段名作为公开 C# API 原样输出；任何不合法 C# 标识符必须在 preflight 阶段报错，而不是自动改名。
- 生成文件名必须唯一，且 `GameConfig.cs`、配置的数据库类文件和 `CftLoadException.cs` 为保留文件名。CFT 不能声明会生成这些文件名的类型或 enum。
- `@struct` 内含或嵌套对象引用是合法语义。resolver 对 struct 返回更新后的 struct value，并在父字段、数组元素和字典值位置写回。
- 生成代码兼容普通 .NET 和 Unity/IL2CPP 常见 C# 版本，不使用 `init` accessor 或 file-scoped namespace。

---

## 2. 命名约定

| CFT | C# |
|-----|----|
| 类型名 | 原样输出 |
| 字段名 | 原样输出 |
| enum 名 / enum variant 名 | 原样输出 |
| 数据库类 | 项目管线默认 `GameConfig`；底层 codegen options 可覆盖 |
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

### `@keyAsEnum`

类型级 `@keyAsEnum(EnumName)` 用于把某个 table 的 record key 填充进手动声明的空 enum，并在 C# 端强类型化。`EnumName` 必须是 schema 中已声明且没有手写 variant 的 enum；该 enum 可以被其他字段正常引用。它不改变 JSON 或 MessagePack 导出格式：数据加载和数据导出仍然把 record key 当作字符串。

C# codegen 会用数据记录 key 替换 `EnumName` 占位 enum 的空 variant 集合，并把该类型生成类的 `Id` 属性和相关 key 索引从 `string` 提升为 `EnumName`。

```cft
@keyAsEnum(GeneId)
type GeneConfig {
}

enum GeneId {}

type BioRemainsConfig {
  gene: GeneConfig?;
}
```

`coflow build` 已经加载数据，因此会按表中实际 record key 的出现顺序生成 enum 变体：

```csharp
public enum GeneId
{
    Gene_Spore = 0,
    Gene_Mating = 1,
}

public partial class GeneConfig
{
    public GeneId Id { get; internal set; }
}

public partial class BioRemainsConfig
{
    public GeneConfig? Gene { get; internal set; }
}
```

单独运行 `coflow codegen csharp` 不加载数据源，所以只能生成 schema
中声明的 `EnumName` 文件，并保留 lockfile 中已有的变体；没有 lockfile
时该 enum 为空。新增 record key 变体只由已加载数据模型的 `build` 路径提供。

项目管线会在 `coflow.yaml` 同级维护 `coflow.enum.lock.json`，用于稳定
`@keyAsEnum` 变体的整数值：

- 新出现的 record key 追加分配下一个未使用整数值。
- 若占位 enum 带 `@flag`，新出现的 record key 追加分配下一个未使用 bit 值（`1, 2, 4, ...`），不会自动生成 `None = 0`。
- 已存在的 record key 保持原有整数值，即使数据顺序变化。
- 若占位 enum 带 `@flag`，lockfile 中已有值必须为正的 2 的幂；否则 codegen/build 报 artifact diagnostic，要求用户清理或重建对应 lockfile 条目。
- 当前 schema 中不再声明的 `@keyAsEnum` enum 会从 lockfile 中移除。
- `coflow codegen csharp` 不加载数据源，只会保留当前 schema 声明的 enum 和 lockfile 中已有的变体。
- 如果 codegen preflight 有诊断，lockfile 不会被读取、写入或清理。
- lockfile 不在 C# 输出目录内，不会随生成目录替换而被删除。

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
    public long Hp { get; set; }
    public long Attack { get; set; }
    public double Speed { get; set; } = 1.0;
}
```

### `abstract type`

```cft
abstract type Reward {}
```

生成：

```csharp
public abstract partial class Reward
{
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
    public long Amount { get; set; }
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
    public double X { get; set; }
    public double Y { get; set; }
}
```

### `@deprecated` type 和 field

```cft
@deprecated
type OldReward { name: string; }

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
    public string Id { get; internal set; } = "";
}

public partial class Item
{
    /// <summary>旧价格</summary>
    [Obsolete]
    public long OldPrice { get; set; } = 0;
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
    public Item? Item { get; internal set; } = null;
    public Item? Backup { get; internal set; }
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
    public IReadOnlyList<string> Tags { get; set; } = new List<string>();
    public IReadOnlyDictionary<DamageType, double> Resistances { get; set; }
        = new Dictionary<DamageType, double>();
}
```

### 对象引用字段

当前 CFT 语义中，对象字段可以由导出数据中的目标 key 表示引用，也可以是内联对象。JSON 和 MessagePack 导出引用时输出目标 key 字符串，运行时加载器负责解析为对象引用。

```cft
type ItemReward : Reward {
  item: Item;
  count: int = 1;
}
```

生成：

```csharp
public partial class ItemReward : Reward
{
    public Item Item { get; internal set; } = null!;    // inline object 或解析后的引用
    public long Count { get; set; } = 1;
}
```

引用目标是 `abstract type` 或有子类的普通 `type` 时，属性类型为声明的目标父类：

```cft
type Quest {
  reward: Reward;
}
```

生成：

```csharp
public partial class Quest
{
    public Reward Reward { get; internal set; } = null!;
}
```

加载器内部会为引用占位对象保存 `__CoflowRefKey`，第二遍通过目标类型 key 索引替换为真实对象；inline object 则递归解析其内部引用。

属性 setter 规则：

- `Id` 始终生成 `{ get; internal set; }`。
- 字段类型是 CFT 对象类型（含 nullable 对象）时生成 `{ get; internal set; }`，因为加载器可能需要在第二遍写回解析后的引用或嵌套对象。
- 数组/字典字段只有在内部 struct 值含有需要 resolver 写回的对象引用时，才生成 `{ get; internal set; }`；普通集合字段生成 `{ get; set; }`，resolver 通过内部 `List` / `Dictionary` 写回元素。
- primitive、enum、纯标量数组和纯标量字典字段也生成 `{ get; set; }`。

### 默认值生成规则

| CFT 默认值 | C# 生成 |
|-----------|---------|
| `0` | `= 0` |
| `1.0` | `= 1.0` |
| `""` | `= ""` |
| `true`/`false` | `= true`/`= false` |
| `[]` | `= new List<T>()` |
| `{}` | `= new Dictionary<K, V>()` |
| 枚举值 | `= Rarity.Common` |
| 无默认值的 `string` | `= ""` |
| 无默认值的非 struct 对象 | `= null!` |
| 无默认值的数组/字典 | 对应空 `List` / `Dictionary` |
| 其他无默认值字段 | 不生成初始化 |

---

## 5. 数据库类生成

数据库类聚合所有 table，提供强类型访问和查询 API：

每个 table 的导出记录都有保留字段 `id`，生成类型包含 `Id` 属性，并生成 `Find{Type}(key)` 查询方法。key 类型默认为 `string`；如果类型带 `@keyAsEnum`，key 类型为生成 enum。

```csharp
public partial class GameConfig
{
    // 每个 table 对应一个 IReadOnlyList 属性
    public IReadOnlyList<Item> Items { get; }
    public IReadOnlyList<Monster> Monsters { get; }

    // record key 查找方法，返回 null 表示不存在
    public Item? FindItem(ItemId id) => _itemIndex.GetValueOrDefault(id);
    public Monster? FindMonster(string id) => _monsterIndex.GetValueOrDefault(id);

    private readonly Dictionary<ItemId, Item> _itemIndex;
    private readonly Dictionary<string, Monster> _monsterIndex;
}
```

---

## 6. 加载器生成

`coflow codegen csharp` 从项目配置的 `outputs.data.type` 选择运行时加载器。`outputs.data.type: json` 生成 JSON loader；`outputs.data.type: messagepack` 生成 MessagePack loader。`outputs.code` 不提供独立 data format override。

加载过程构造强类型对象，建立 record key 查找表，并在第二遍解析对象引用。生成的 C# 加载器是 trusted artifact loader，只支持官方 Coflow exporter 从已经通过 Rust pipeline 的数据生成的 JSON 或 MessagePack；它不承诺对任意手写或损坏数据提供稳定诊断，也不运行 CFT check。失败时抛出 `CftLoadException`。

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

        // 第二遍：解析对象引用
        foreach (var reward in monsters
            .SelectMany(m => m.Drops.Rewards)
            .OfType<ItemReward>())
        {
            reward.Item = ResolveRef(itemIndex, reward.Item, "ItemReward.item", "Item");
        }

        return new GameConfig(items, monsters, itemIndex, monsterIndex);
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

        // 第二遍：解析对象引用；缺失目标会抛 CftLoadException。
        foreach (var reward in monsters
            .SelectMany(m => m.Drops.Rewards)
            .OfType<ItemReward>())
        {
            reward.Item = ResolveRef(itemIndex, reward.Item, "ItemReward.item", "Item");
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

MessagePack object loader 读取 map header 后逐项读取 string 字段 key，并用 `switch` 分发到生成的字段 reader。未知字段通过 path-aware helper（例如 `SkipValue(ref reader, fieldPath)`）跳过；helper 内部包装底层 skip 调用的异常并转换成带字段路径的 `CftLoadException`。已知字段重复、必填字段缺失、ID 重复、字典 key 重复、引用目标缺失、`$type` 缺失或未知、MessagePack 类型不匹配，均抛出 `CftLoadException`。

MessagePack 多态对象的 `$type` 分发：loader 先读取 record map 的第一项，要求 key 为 `$type`，再读取实际类型名并分发到对应的 `Load<Type>Body`。这依赖 MessagePack exporter 按规格把 `$type` 写为多态 map 第一项。

---

## 7. 错误处理

加载器失败时抛出 `CftLoadException`，包含字段路径和详细信息：

```csharp
public sealed class CftLoadException : Exception
{
    /// <summary>字段路径，如 "Monster[3].drops.rewards[1].item"</summary>
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
| 引用目标 key 不存在 | 外键指向不存在的记录 |
| record key 重复 | 同一类型或同一继承树索引中存在重复 key |
| object/map key 重复 | 字典或对象字段出现重复 key |

---

## 8. 完整示例

CFT 输入：

```cft
enum Rarity { Common = 0, Rare = 10, Epic = 20, }

type Stats { hp: int; attack: int; speed: float = 1.0; }

abstract type Reward {}
sealed type CurrencyReward : Reward { amount: int; }
sealed type ItemReward : Reward {
  item: Item;
  count: int = 1;
}

@display("物品")
@keyAsEnum(ItemId)
type Item {
  @display("名称")
  name: string;

  rarity: Rarity = Rarity.Common;
}

enum ItemId {}

type Monster {
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
    public long Hp { get; set; }
    public long Attack { get; set; }
    public double Speed { get; set; } = 1.0;
}

public abstract partial class Reward
{
}

public sealed partial class CurrencyReward : Reward
{
    public long Amount { get; set; }
}

public sealed partial class ItemReward : Reward
{
    public Item Item { get; internal set; } = null!;
    public long Count { get; set; } = 1;
}

/// <summary>物品</summary>
public partial class Item
{
    public ItemId Id { get; internal set; }
    /// <summary>名称</summary>
    public string Name { get; set; } = "";
    public Rarity Rarity { get; set; } = Rarity.Common;
}

public partial class Monster
{
    public string Id { get; internal set; } = "";
    public Rarity Rarity { get; set; }
    public long Level { get; set; }
    public Stats Stats { get; internal set; } = null!;
}
```

生成的数据库类：

```csharp
public partial class GameConfig
{
    public IReadOnlyList<Item> Items { get; }
    public IReadOnlyList<Monster> Monsters { get; }

    public Item? FindItem(ItemId id) => _itemIndex.GetValueOrDefault(id);
    public Monster? FindMonster(string id) => _monsterIndex.GetValueOrDefault(id);


    private readonly Dictionary<ItemId, Item> _itemIndex;
    private readonly Dictionary<string, Monster> _monsterIndex;

    public static GameConfig Load(string dataDir) { ... }
}
```
