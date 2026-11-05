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
var baseName = sword.Base.name;
var japanese = sword.For("language", "ja");
var japaneseName = japanese.name;
```

加载失败不替换业务正在使用的数据。成功后由宿主一次性交换数据根；旧数据和其视图属于旧加载上下文，不能与新加载根的对象混作同一对象身份。

## 业务访问与维度选择

`Table<T>()` 返回只读维度表，元素为 `Dimension<T>`，父表包含派生记录，基础表和引用共享对象。`Singleton<T>()` 获取单例的维度容器。`@idAsEnum` 生成对应表的 `Get(enum)`、`TryGet(enum, out record)` 扩展，以稳定枚举值映射 CFD 业务 id。

加载默认完成基础数据与全部已声明单维变体；多维组合通过 `LoadOptions.Combinations` 明确指定。`Dimension<T>.For("language", "ja")` 只返回现成普通业务对象，`.Base` 返回基础对象。组合使用 `For(new DimensionSelection(("language", "ja"), ("platform", "mobile")))` 一次选择，需在加载时请求该组合。

配置根的 `Base` 和 `For` 返回完整 `CfdSnapshot`，其表和单例直接包含普通业务对象，与逐记录选择共享身份。`HasVariant(nameof(Item.name), "ja")` 查询显式覆盖，`Variants("language")` 获取可选变体。未知维度、未知变体和未构建组合报参数错误。

所有模板、集合和引用在加载成功前完成。加载错误通过 Diagnostics 报告，读取最终数据不会触发求值。完成的数据对象图不持有加载状态；应用只需管理最终数据根和引用的生命周期。

## 类型与扩展

记录、单例和默认 data 使用 partial class；`@struct sealed data` 使用 partial struct。int、long、float、double 对应同名 C# 类型，维度字段保持原业务类型。`Option<T>` 通过 IsSome、IsNone、Value 访问并保留嵌套层；字段能否省略只取决于 CFT 是否写有默认值。

普通字符串保留原文，`f"..."` 在所选上下文中求值为 string。业务 partial 文件只扩展方法和计算属性，重新生成源码不会覆盖扩展文件。

具体构造、缓存和类型契约见 [加载器设计](../codegen-design/csharp-runtime.zh-CN.md)。面向使用者的命令与 API 示例见 [C# 参考](../../website/docs/docs/reference/07-codegen/01-csharp.md)。
