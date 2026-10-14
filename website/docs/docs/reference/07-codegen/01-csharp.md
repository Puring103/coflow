# C# 代码生成与数据访问

C# 应用引用 `Coflow.Loader` 与生成源码，支持 .NET Standard 2.1、.NET 8 和 Unity 2022+。

```yaml
schema: schema/
data: data/
codegen:
  - language: csharp
    dir: generated/csharp
    namespace: Game.Config
```

运行 `coflow codegen --project .` 后，将 `runtimes/csharp/Coflow.Loader` 与生成文件加入同一 C# 项目。

```csharp
using Coflow.Loader;
using Game.Config;

var result = CoflowData.Load(new[] {
    new CfdSource("items.cfd", itemsText),
    new CfdSource("ja.cfd", japaneseText),
});
if (!result.Success) {
    foreach (var diagnostic in result.Diagnostics)
        System.Console.WriteLine(diagnostic);
    return;
}
var data = result.Data;
var items = data.Table<Item>();
var sword = items["sword"];
var settings = data.Singleton<Settings>();
var name = sword.name.For("ja");
```

`Table<T>()` 获取 table 声明对应的只读表，支持索引、TryGetValue、ContainsKey、Count、Keys、Values 和遍历。`Singleton<T>()` 获取单例。父类型表包含子类型记录，表与字段引用共享对象；重复获取返回同一表。data 类型用于内联值，不产生记录表。

## 类型与扩展

| CFT | C# |
| --- | --- |
| table / singleton | partial class |
| data | partial class |
| @struct sealed data | partial struct |
| int / float | long / double |
| string / fstring | string |
| enum | 以 long 为基础的 enum |
| `&TableName` 字段 | 对应业务对象引用 |
| data 类型字段 | 内联业务值 |
| `Option<T>` | `Coflow.Loader.Option<T>` |
| [T] / {K: V} | `IReadOnlyList<T>` / `IReadOnlyDictionary<K, V>` |
| 维度字段 | `DimensionValue<T>` |

记录和单例具有 string id，data 保存业务字段。生成类型保留 abstract、sealed 和继承约束。业务方法与计算属性可以写在独立 partial 文件中，扩展文件须位于同一 namespace 和程序集。

```csharp
public partial class Item
{
    public bool IsFree => price == 0;
}
```

## 枚举主键

```cft
enum ItemId {}
@idAsEnum(ItemId)
table Item { name: string; }
```

生成 ItemId 的稳定编号及对应表的强类型查询扩展：

```csharp
var items = data.Table<Item>();
Item sword = items.Get(ItemId.sword);
bool found = items.TryGet(ItemId.shield, out var shield);
```

Get 在 key 不存在时抛出 KeyNotFoundException，TryGet 返回 false。枚举扩展属于对应表，字符串索引仍可用于按 CFD key 查询。@idAsEnum 的映射只包含当前记录，编号在增删记录后保持稳定。

## 可选值、维度与错误

Option 通过 IsSome、IsNone、Value 访问，读取 None 的 Value 抛出异常。嵌套可选保留 None、Some(None) 和 Some(Some(value))。

维度字段使用 Default、For(variant)、Variants；未覆盖或未知变体回退基础值，显式 None 保持为空。fstring 在加载时生成文本，普通字符串保留花括号。

调用方提供互相引用的一组 CFD 内容，逻辑路径用于定位诊断。失败结果包含 Diagnostics，读取失败结果的 Data 抛出异常。CFT 用于声明校验与生成代码，编辑器执行 check。
