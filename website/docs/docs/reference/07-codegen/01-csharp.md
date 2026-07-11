# C# 代码生成

C# codegen 根据编译后的 CFT schema 生成只读运行时 API。生成代码不包含 CFT parser，不重新校验配置数据，只读取 Coflow 导出的 JSON 或 MessagePack 产物。

## 启用方式

在 `coflow.yaml` 中配置代码输出：

```yaml
outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
    int_32: false
    float_32: false
```

运行：

```powershell
coflow codegen csharp
```

或在完整构建中生成：

```powershell
coflow build
```

命令行可以覆盖输出目录和命名空间：

```powershell
coflow codegen csharp --out generated/csharp --namespace Game.Config
```

## 输出目录

生成目录示例：

```text
generated/csharp/
  CoflowTables.cs
  Item.cs
  Reward.cs
  Rarity.cs
  Coflow.Runtime/
    Localization.cs
```

`CoflowTables.cs` 是默认入口。每个 CFT type / enum 会生成对应 C# 文件。本地化字段存在时，会额外生成运行时 helper。

`outputs.code.dir` 或 `--out` 是不可变 generation 的放置锚点。生成成功信息会输出实际 generation 目录；当前目录也记录在 `.coflow/artifacts/active.json` 的 `outputs.code.generation_dir`。后续 codegen 创建新 generation，不会修改或删除旧 generation。不要在 generation 中放置或修改手写代码。

## 入口类

默认入口类是 `CoflowTables`：

```csharp
var tables = CoflowTables.Load(dataDir);

var item = tables.TbItem.Get("sword_fire");
var maybeItem = tables.TbItem.Find("missing");

if (tables.TbItem.TryGet("sword_fire", out var found))
{
    Console.WriteLine(found.Name);
}
```

每张 concrete table 生成一个 `Tb{TypeName}` 访问器。

访问器类型实现只读列表能力，并提供：

| API | 说明 |
| --- | --- |
| `Get(id)` | 找不到时抛 `KeyNotFoundException` |
| `Find(id)` | 找不到时返回 `default` |
| `TryGet(id, out value)` | 尝试读取 |
| `Count` | record 数量 |
| 索引器 | 按导出顺序读取 |
| 枚举器 | 遍历所有 record |

## 命名规则

公开 C# API 使用 C# 风格命名：

| CFT | C# |
| --- | --- |
| type 名 | PascalCase class / struct |
| enum 名 | PascalCase enum |
| enum variant | PascalCase member |
| 字段名 | PascalCase property |
| record key | `Id` |
| table accessor | `Tb{TypeName}` |

示例：

```text
type item_config {
  display_name: string;
}
```

生成：

```csharp
public sealed partial class ItemConfig
{
    public string Id { get; }
    public string DisplayName { get; }
}
```

导出数据仍使用原始字段名，例如 JSON 中读取 `display_name`。

## 类型映射

| CFT | C# |
| --- | --- |
| `int` | `long` |
| `float` | `double` |
| `bool` | `bool` |
| `string` | `string` |
| `T?` | `T?` |
| enum | C# enum |
| `[T]` | `IReadOnlyList<T>` |
| `{K: V}` | `IReadOnlyDictionary<K, V>` |
| object type | 对应 C# 类型 |

集合属性只读，loader 内部使用 `List<T>` 和 `Dictionary<K, V>` 构造。

`outputs.code.int_32: true` 时，`int` 会生成 `int`；`outputs.code.float_32: true` 时，`float` 会生成 `float`。省略时分别使用 `long` 和 `double`。

## 类型与字段

每个 CFT type 生成一个 partial C# 类型。非 abstract 类型会实现 `IEquatable<T>`，并基于 `Id` 生成 `ToString()`、`Equals` 和 `GetHashCode()`。

生成属性只有 getter：

```csharp
public sealed partial class Item : IEquatable<Item>
{
    public string Id { get; }
    public string Name { get; }
    public Reward Reward { get; }

    private Item(string id, string name, Reward reward)
    {
        Id = id;
        Name = name;
        Reward = reward;
    }
}
```

object 字段会在加载时直接解析为最终对象，不生成 `xxxKey` 或引用占位对象。

## 注解影响

| CFT 注解 | C# 输出 |
| --- | --- |
| `@flag` | `[Flags]` enum |
| `@struct` | value-like struct |
| `@idAsEnum(EnumName)` | record key 使用强类型 enum |
| `@singleton` | 入口类直接暴露 singleton 属性 |
| `@localized` | 字段包装为 `Localized<T>` |

## `@idAsEnum`

`@idAsEnum` 会把指定 type 的 record key 转成 enum：

```text
@idAsEnum(ItemId)
type Item {
  name: string;
}

enum ItemId {}
```

生成后，`TbItem` 的 key 类型会从 `string` 变为 `ItemId`，引用该 type 的字段也会使用对应 enum。

`@idAsEnum` lock state 位于 `.coflow/artifacts/active.json`，与 data/code generation 在同一次 manifest 激活中发布，用来稳定 enum variant 的整数值。Coflow 在最终激活前原子更新 `coflow.yaml` 同级的 `coflow.enum.lock.json`，它是可提交到版本库的非权威镜像；已有 active manifest 始终优先，没有本地 manifest 的干净 clone 才从该文件恢复 lock state。`coflow build` 会加载数据并补全新增 variant；单独运行 `codegen csharp` 不加载数据源，因此只读取已有 lock state。

## `@singleton`

`@singleton` type 不生成 `Tb*` table 访问器，而是在入口类上直接生成属性：

```text
@singleton
type GameConfig {
  max_level: int;
}
```

如果数据源中的 record key 是 `main_config`，生成：

```csharp
public sealed partial class CoflowTables
{
    public GameConfig main_config { get; }
}
```

singleton 属性名使用 record key 原文。

## `@localized`

`@localized` 字段生成 `Localized<T>`：

```text
type Item {
  @localized
  name: string;
}
```

生成：

```csharp
public Localized<string> Name { get; }
```

`Localized<T>` 保存 key 和默认值，运行时可以按当前语言取值：

```csharp
var displayName = item.Name.Value;
var englishName = item.Name.For("en");
```

详见 [本地化与维度](../10-localization.md)。

## Loader 选择

`outputs.data.type` 决定生成哪种 loader：

| `outputs.data.type` | 生成 loader |
| --- | --- |
| `json` | Newtonsoft.Json loader |
| `messagepack` | MessagePack-CSharp loader |

生成代码按 object 字段引用关系排序加载 table，确保被引用 table 先建立索引。存在 table 级循环引用时，C# codegen 会报错。

## 生成前检查

codegen 会在写文件前检查：

- namespace 是否合法。
- C# 类型名、enum 名、字段属性名是否合法。
- 生成文件名是否冲突。
- 成员名是否冲突。
- `@idAsEnum` variant 是否能生成合法 C# enum member。
- 输出目录是否可安全接管。

存在诊断时，不会激活新的 generation，也不会更新 active manifest 中的 lock state。

## 运行时定位

生成的 C# loader 是 trusted artifact loader：它面向 Coflow 官方 exporter 输出，不重新提供完整 validator。schema 校验、数据模型构建、引用检查和业务 check 应在 `coflow check` / `coflow build` 阶段完成。

如果导出文件被手动破坏，loader 可能抛出底层 JSON、MessagePack 或 BCL 异常。公开 API 中，`Table.Get` 找不到 key 固定抛 `KeyNotFoundException`。
