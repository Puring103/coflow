# C# 代码生成规格

**依赖文档**：[01-cft.md](01-cft.md)、[02-data-model.md](02-data-model.md)、[05-json-export.md](05-json-export.md)、[08-messagepack-export.md](08-messagepack-export.md)

C# codegen 以编译后的 `CftContainer` 为输入，生成只读运行时数据模型和表访问 API。生成代码不依赖 CFT parser/compiler，只加载官方 Coflow exporter 输出的 JSON 或 MessagePack 数据。

---

## 1. 输出形态

默认入口类是 `CoflowTables`，默认入口文件是 `CoflowTables.cs`。底层 Rust API 可通过 `CsharpCodegenOptions::with_database_class` 改入口类名；`coflow.yaml` 当前没有 database class 配置项。

生成目录示例：

```text
generated/csharp/
  CoflowTables.cs
  Rarity.cs
  Reward.cs
  Item.cs
```

不生成 `GameConfig.cs` 兼容别名，不生成 `CftLoadException.cs`。

`CoflowTables` 为每张 concrete table 暴露 `Tb{TypeName}` 访问器：

```csharp
var tables = CoflowTables.Load(dataDir);

var item = tables.TbItem.Get("potion");
var maybeItem = tables.TbItem.Find("potion");

if (tables.TbItem.TryGet("potion", out var found))
{
    Console.WriteLine(found.DisplayName);
}
```

访问器类型是 `CoflowTables.Table<TKey, TRecord>`，实现 `IReadOnlyList<TRecord>`，并提供：

- `Get(TKey id)`：找不到时抛 `KeyNotFoundException`。
- `Find(TKey id)`：找不到时返回 `default`。
- `TryGet(TKey id, out TRecord value)`。
- `Count`、索引器和枚举器。

---

## 2. 命名约定

公开 C# API 使用 C# 风格命名：

| CFT | C# |
|-----|----|
| 类型名 | PascalCase |
| enum 名 / enum variant 名 | PascalCase |
| 字段名 | PascalCase 属性 |
| table accessor | `Tb{TypeName}` |
| record key | `Id` |

源字段名只用于读取导出数据。例如 `type item_config { display_name: string; }` 生成 `ItemConfig.cs`、`ItemConfig.DisplayName`，但 JSON/MessagePack reader 仍读取 `display_name`。

`@display("text")` 生成 XML summary；`@deprecated` 生成 `[Obsolete]`；`@flag` enum 生成 `[Flags]`。

生成前会校验 C# 标识符、生成文件名和成员名碰撞。默认配置下 schema 类型或 enum 不能生成 `CoflowTables.cs`；`GameConfig.cs` 和 `CftLoadException.cs` 不再是固定保留名，可以由 schema 类型生成。

---

## 3. 类型生成

每个 CFT type 生成一个 partial C# 类型。非 abstract 类型实现 `IEquatable<T>`，并生成基于 `Id` 的 `ToString()`、`Equals`、`GetHashCode()`。

数据属性只读：

```csharp
public sealed partial class Item : IEquatable<Item>
{
    public string Id { get; }
    public string DisplayName { get; }
    public Reward Reward { get; }

    private Item(string id, string displayName, Reward reward)
    {
        Id = id;
        DisplayName = displayName;
        Reward = reward;
    }
}
```

不会生成 public/internal setter。object 字段不生成 `xxxKey`、`__CoflowIsRef`、`__CoflowRefKey` 或引用占位对象。

类型映射：

| CFT | C# |
|-----|----|
| `int` | `long` |
| `float` | `double` |
| `bool` | `bool` |
| `string` | `string` |
| `T?` | `T?` |
| `[T]` | `IReadOnlyList<T>` 属性，loader 内部用 `List<T>` |
| `{K: V}` | `IReadOnlyDictionary<K, V>` 属性，loader 内部用 `Dictionary<K, V>` |
| object type | 目标 C# 类型 |

schema 可空字段生成 `T?`。不可空集合缺省为空集合；JSON loader 在字段缺失时使用生成的默认表达式。JSON 空表文件可以不存在，loader 会把它视为空表。

`@keyAsEnum(EnumName)` 会把对应 table 的 record key 类型从 `string` 提升为 `EnumName`，并把引用 key reader 也切换为该 enum。

> **状态**：当前实现的范围 — 单例 type 不会被生成 `Tb*` table 访问器（schema_view 已正确过滤），但入口类上的单例属性、`Localized<T>` 包装类型与运行时 helper 文件尚未集成到模板渲染层。详见各小节末尾的"已知限制"。

### 3.1 `@singleton`

被 `@singleton` 标记的 type 不生成 `Tb*` table 访问器。入口类直接挂一个以该 type 唯一 record 的 key 命名的属性，类型为 type 本身。属性名直接使用 record key 原文，不做 PascalCase 转换：

```cft
@singleton
type GameConfig { max_level: int; }   # 数据源里 record key = "main_config"
```

```csharp
public sealed partial class CoflowTables
{
    public GameConfig main_config { get; }
    // 无 TbGameConfig
}
```

撞名校验：所有 singleton 的 record key 之间、与普通 type 生成的 `Tb*` 访问器之间均不可冲突；冲突在 `CftSchemaType.is_singleton` 校验阶段已由 data model build 处理（`CFD-DATA-017 SingletonKeyCollision`），codegen 在生成期再次去重以避免成员名碰撞。

`@singleton` type 不会被任何字段类型引用（CFT 编译期已禁止），因此 codegen 不为其生成 inline / record-ref loader 路径。

### 3.2 `@localized`

被 `@localized` 标记的字段类型在 C# 端统一包装为 `Localized<T>`，`T` 为字段原 CLR 类型（按 §3 类型映射推导）：

```csharp
public sealed partial class Item : IEquatable<Item>
{
    public string Id { get; }
    public Localized<string> DisplayName { get; }
    public Localized<long> SortOrder { get; }
    public Localized<IReadOnlyList<string>> Tags { get; }
}
```

构造时传入 `Key` 与 `Default` 两个值：`Key` 由 codegen 静态拼接（形如 `"Item/potion/name"`），`Default` 来自数据源原始字面量。

运行时 helper（`Localized<T>` 与 `Localization` 入口）一次性生成到 `Coflow.Runtime/Localization.cs`，宿主可替换实现。详见 [13-localization.md](13-localization.md)。

---

## 4. 加载器生成

`coflow codegen csharp` 根据项目 `outputs.data.type` 选择运行时加载器：

| `outputs.data.type` | 生成加载器 |
|---------------------|------------|
| `json` | 读取 `<TypeName>.json`，使用 `Newtonsoft.Json.Linq` |
| `messagepack` | 读取 `<TypeName>.msgpack`，使用 MessagePack-CSharp `MessagePackReader` |

每个数据类型文件承载自己的 `LoadTable`、row/inline loader 和 `BuildIndex`。`CoflowTables.cs` 只负责按依赖顺序编排加载、保存表访问器、定义 `LoadContext` 和通用 helper。

object 字段在读取字段时直接解析为最终对象：

- inline object：递归构造对象。
- exporter 输出的 raw key reference：通过 `LoadContext` 查目标表索引。
- nullable object：`null` 保持为 `null`。

生成器按 object 字段引用关系对表排序，确保被引用表先加载并建立索引。如果 table 之间存在循环引用，C# codegen 报错；当前只读即时解析模式不支持表级循环引用。

生成代码不得依赖反射，不使用 `System.Reflection`、`Activator`、`PropertyInfo`、`FieldInfo`、`Type.GetType`、`GetProperties(` 或 `GetFields(`。

---

## 5. 错误处理

生成的 C# loader 是 trusted artifact loader，只面向官方 Coflow exporter 输出。schema 校验、数据模型构建、引用检查和业务 check 已在 Rust pipeline 完成，C# loader 不重复提供稳定 validator 契约。

不生成 `CftLoadException`。文件损坏、字段缺失、重复 key、引用缺失等可信产物破坏场景允许由底层库或 BCL 异常自然抛出。`Table.Get` 的未找到行为是公开 API 契约，固定抛 `KeyNotFoundException`。

---

## 6. 示例

输入：

```cft
type Reward {
  amount: int;
}

type Item {
  display_name: string;
  reward: Reward;
}
```

生成入口：

```csharp
public sealed partial class CoflowTables
{
    public Table<string, Reward> TbReward { get; }
    public Table<string, Item> TbItem { get; }

    public static CoflowTables Load(string dataDir)
    {
        var rewards = Reward.LoadTable(Path.Combine(dataDir, "Reward.json"), LoadContext.Empty);
        var rewardIndex = Reward.BuildIndex(rewards);

        var items = Item.LoadTable(Path.Combine(dataDir, "Item.json"), new LoadContext(rewardIndex));
        var itemIndex = Item.BuildIndex(items);

        return new CoflowTables(
            new Table<string, Reward>(rewards, rewardIndex),
            new Table<string, Item>(items, itemIndex));
    }
}
```

生成类型：

```csharp
public sealed partial class Item : IEquatable<Item>
{
    public string Id { get; }
    public string DisplayName { get; }
    public Reward Reward { get; }

    private Item(string id, string displayName, Reward reward)
    {
        Id = id;
        DisplayName = displayName;
        Reward = reward;
    }

    private static Item LoadRow(JToken token, CoflowTables.LoadContext context)
    {
        var obj = CoflowJson.RequireObject(token);
        var id = CoflowJson.ReadRequired(obj, "id", token => CoflowJson.ReadString(token));
        var displayName = CoflowJson.ReadRequired(obj, "display_name", token => CoflowJson.ReadString(token));
        var reward = CoflowJson.ReadRequired(obj, "reward", token => context.GetReward(CoflowJson.ReadString(token)));

        return new Item(id, displayName, reward);
    }
}
```
