# C# 与 Unity 集成

## 依赖与构建产物

`Coflow.Loader` 面向 netstandard2.1、net8.0 和 Unity 2022+。应用引用加载器程序集和项目生成的 C# 源码。生成文件与业务 partial 扩展放在同一 namespace 和程序集；加载器可作为独立依赖程序集。

使用 `coflow codegen --project <project>` 更新生成源码。业务 `check` 在项目校验或构建阶段执行，C# 加载器负责 CFD 结构、值、引用和模板校验。生成产物不包含 CFT 规则执行器。

## 资源加载边界

应用资源系统读取基础与覆盖 CFD 文本，并构造 `CfdSource`。逻辑路径用于诊断；目录发现、资源下载、热更新调度和资源生命周期由宿主负责。一次加载提供相互引用闭合的一组源，基础和覆盖的传入顺序不影响结果。

```csharp
using Coflow.Loader;
using Game.Config;

var loaded = CoflowData.Load(new[] {
    new CfdSource("items.cfd", itemsText),
    new CfdSource("ja.cfd", japaneseText),
});
if (!loaded.Success) {
    foreach (var diagnostic in loaded.Diagnostics)
        System.Console.WriteLine(diagnostic);
    return;
}
var data = loaded.Data;
var sword = data.Table<Item>()["sword"];
var baseName = sword.name;
var japanese = sword.For("language", "ja");
var japaneseName = japanese.name;
```

加载失败不替换业务正在使用的数据。成功后由宿主一次性交换数据根；旧数据和其视图属于旧加载上下文，不能与新加载根的对象混作同一对象身份。

## 业务访问与维度选择

`Table<T>()` 返回只读表，父表包含派生记录，基础表和引用共享对象。`Singleton<T>()` 获取单例。`@idAsEnum` 生成对应表的 `Get(enum)`、`TryGet(enum, out record)` 扩展，以稳定枚举值映射 CFD 业务 id。

每个界面、场景或服务按需要调用 `record.For("language", "ja")`。视图保持独立，基础对象不发生切换。多个维度可链式组合；视图内引用、内联数据和模板继续使用该上下文。循环引用在同一视图内仍保持身份。

`record.HasVariant(nameof(record.name), "ja")` 判断某字段是否有显式覆盖，包含显式 None 和同值覆盖。`record.Variants("language")` 列出该维度所有已知变体，可用于语言菜单和覆盖检查。遍历变体不意味着每个字段都提供了覆盖。

视图缓存与读取材料随加载结果存活。应用释放旧数据时也应释放其视图引用。首次构造新的多维度组合可能报告组合依赖错误，调用方可捕获 `CfdLoadException` 并展示其 `Diagnostics`。

## 类型与扩展

记录、单例和默认 data 使用 partial class；`@struct sealed data` 使用 partial struct。int、long、float、double 对应同名 C# 类型，维度字段保持原业务类型。`Option<T>` 通过 IsSome、IsNone、Value 访问并保留嵌套层；字段能否省略只取决于 CFT 是否写有默认值。

普通字符串保留原文，`f"..."` 在所选上下文中求值为 string。业务 partial 文件只扩展方法和计算属性，重新生成源码不会覆盖扩展文件。

具体构造、缓存和类型契约见 [加载器设计](../language-design/02-api-runtime-design.zh-CN.md)。面向使用者的命令与 API 示例见 [C# 参考](../../website/docs/docs/reference/07-codegen/01-csharp.md)。
