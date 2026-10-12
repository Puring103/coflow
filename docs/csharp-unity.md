# C# 数据加载

Coflow.Loader 支持 netstandard2.1、net8.0 与 Unity 2022+。应用引用加载器和生成源码，自行选择互相引用的一组 CFD 文件并提供逻辑路径与文本。

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

Table<T>() 返回只读表，提供索引、TryGetValue、ContainsKey、Count、Keys、Values 与遍历。父表包含子类型记录，字段引用与记录表共享对象身份。Singleton<T>() 获取唯一配置；data 不注册记录表。

@idAsEnum 的对应表生成 Get(enum) 与 TryGet(enum, out record) 强类型扩展，显式映射枚举到 CFD key，编号保持稳定。未知 key 的 Get 抛出异常，TryGet 返回 false。

table、singleton 和默认 data 生成 partial class，sealed/abstract/继承约束保留；@struct sealed data 生成 partial struct。struct data 使用封闭的值类型，无 class 基类。扩展文件与生成类型必须在同一 namespace 和程序集。

int/float 使用 long/double，Option<T> 提供 IsSome、IsNone、Value，并保留嵌套层。维度字段使用 DimensionValue<T>.Default、For、Variants，未覆盖和未知变体使用基础值，显式 None 保留为空。

fstring 在加载时按字段路径生成文本，普通字符串保持原文。加载结果只保留业务数据；解析材料释放。构造和注册契约见 [加载器设计](language-design/02-api-runtime-design.zh-CN.md)。
