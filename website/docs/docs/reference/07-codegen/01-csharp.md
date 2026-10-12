# C# 代码生成与加载

C# 应用引用 `Coflow.Loader` 与生成源码，支持 .NET Standard 2.1、.NET 8 和 Unity 2022+。调用方提供所选 CFD 文件的逻辑路径与内容，加载结果直接包含生成的业务对象。

```yaml
schema: schema/
data: data/
codegen:
  - language: csharp
    dir: generated/csharp
    namespace: Game.Config
```

运行 `coflow codegen .` 后，将 `runtimes/csharp/Coflow.Loader` 和生成文件加入 C# 项目。

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
var item = result.Data.Item["sword"];
var name = item.name.For("ja");
```

互相引用的记录应在同一次加载中提供。逻辑路径用于定位诊断；失败结果包含 Diagnostics，读取失败结果的 Data 抛出异常。

## 类型映射

| CFT | C# |
| --- | --- |
| int / float | long / double |
| bool / string / fstring | bool / string / string |
| enum | 以 long 为基础的 enum |
| 普通 Type | class 内联对象 |
| &Type | 对应记录 class 的对象引用 |
| @struct Type | 内联值为结构体，记录及引用为 TypeRecord class |
| `Option<T>` | `Coflow.Loader.Option<T>` |
| [T] / {K: V} | `IReadOnlyList<T>` / `IReadOnlyDictionary<K, V>` |
| 维度字段 | `DimensionValue<T>` |

普通 type 的记录与内联值使用同一业务 class。记录 id 为 CFD key，内联对象 id 为 null。`@struct` 的记录身份类具有相同业务字段，引用可以形成环。继承表与引用共享同一记录对象，结构体保留继承字段；结构体子类型放在 class 父类型字段时使用其类表示。

所有类型都有对应记录表。`@singleton` 通过同名属性读取唯一记录。

## 可选值与维度

```csharp
if (item.next.IsSome) {
    var next = item.next.Value;
}
```

Option 使用 `IsSome`、`IsNone`、`Value`；读取 None 的 Value 抛出异常。嵌套 `Option<Option<T>>` 区分 None、Some(None) 与 Some(Some(value))。

维度字段使用 `Default`、`For(variant)`、`Variants`。未覆盖字段及未知变体回退到基础值，显式 None 保持为空。一次加载可读取多个变体。

普通字符串保持文本，fstring 在加载时生成结果。输入类型、默认值、引用、分组和覆盖错误通过加载诊断返回。CFT 用于声明检查与代码生成，check 规则由编辑器执行。
