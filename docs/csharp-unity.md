# C# 数据加载

`Coflow.Loader` 支持 netstandard2.1 和 net8.0。应用引用加载器程序集和生成的 C# 源码，提供自己选择的 CFD 文件内容。
C# 不读取 CFT，不执行 check，不需要原生插件或运行时契约文件。

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
var sword = data.Item["sword"];
var name = sword.name.For("ja");
```

逻辑文件路径用于诊断定位。文件选择与读取由调用方负责；互相引用的记录应当放在同一次加载中。

普通 type 的内联值与记录均生成 class；内联对象的 `id` 为 null，记录的 `id` 为 CFD key。
`@struct` 类型的内联值生成结构体，顶层记录与 `&Type` 使用 `TypeRecord` 类；继承字段保留在结构体与身份类中。
引用保留记录对象身份，可形成环。生成入口保存全部类型的记录表和 singleton；继承表与引用共享对象。

`int` 生成 `long`，`float` 生成有限 `double`。`Option<T>` 使用 `IsSome`、`IsNone`、`Value`，嵌套可选保留 None、Some(None) 与 Some(Some(value)) 的区别。读取 None 的 Value 抛出异常。

维度字段为 `DimensionValue<T>`，包含 `Default`、`For(variant)` 和 `Variants`。
未覆盖字段及未知变体回退到默认值，显式 `None` 覆盖保持为空。一次加载可读取多个变体。

格式化字符串使用 `f"..."`，只允许字段路径插值、基础值转文本及花括号转义。
模板在加载阶段求值；循环依赖或不合法路径生成带来源的加载诊断。普通字符串保留花括号原文。

加载结束后只保留生成类型组成的数据；解析树和构建缓存随加载过程结束释放。
详细构造协议见 [加载器设计](language-design/02-api-runtime-design.zh-CN.md)。
