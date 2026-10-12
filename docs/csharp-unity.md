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

生成记录为 class；生成 struct 为 C# 结构体。引用保留记录对象身份，可形成环。
生成入口保存类型明确的表和 singleton；记录、继承表和 singleton 的引用共享同一份记录对象。

维度字段为 `DimensionValue<T>`，包含 `Default`、`For(variant)` 和 `Variants`。
未覆盖字段及未知变体回退到默认值，显式 `None` 覆盖保持为空。一次加载可读取多个变体。

格式化字符串使用 `f"..."`，只允许字段路径插值、基础值转文本及花括号转义。
模板在加载阶段求值；循环依赖或不合法路径生成带来源的加载诊断。普通字符串保留花括号原文。

加载结束后只保留生成类型组成的数据；解析树和构建缓存随加载过程结束释放。
详细构造协议见 [加载器设计](language-design/02-api-runtime-design.zh-CN.md)。
