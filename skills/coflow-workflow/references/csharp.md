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

运行 `coflow codegen --project .` 后，将 `runtimes/csharp/Coflow.Loader` 与生成文件加入 C# 项目。

## 加载与查询

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
var baseName = sword.name;
var japaneseName = sword.For("language", "ja").name;
```

调用方提供一组相互引用的 CFD 内容，路径用于定位诊断。加载失败时查看 `Diagnostics`；只有 `Success` 为 true 时才能读取 `Data`。

`Table<T>()` 获取只读记录表，支持字符串索引、`TryGetValue`、`ContainsKey`、`Count`、`Keys`、`Values` 和遍历。`Singleton<T>()` 获取单例。父类型表包含子类型记录，基础表与基础引用共享对象。`data` 用于内联值，不产生记录表。

## 类型与扩展

| CFT | C# |
| --- | --- |
| table / singleton | partial class |
| data | partial class |
| @struct sealed data | partial struct |
| int / long / float / double / bool / string | 同名 C# 类型 |
| enum | 默认以 int 为基础；`@int64` 使用 long |
| `&TableName` | 对应业务对象引用 |
| data 类型字段 | 内联业务值 |
| `Option<T>` | `Coflow.Loader.Option<T>` |
| `[T]` / `{K: V}` | `IReadOnlyList<T>` / `IReadOnlyDictionary<K, V>` |

位枚举生成 `[Flags]`，并包含 `None = 0` 和随实际标志位计算的 `Everything`。`@int64` 可与 `@flag`、记录 ID 枚举配合使用。

记录和单例具有 `string id`。维度字段保持原本的业务类型，直接访问就是基础值。C# 关键字名称在生成源码中自动使用 `@` 转义。

生成类型保留 abstract、sealed 和继承约束。业务方法与计算属性放在独立 partial 文件中，使用相同 namespace 和程序集：

```csharp
public partial class Item
{
    public bool IsFree => price == 0;
}
```

## 枚举记录 id

```cft
enum ItemId {}
@idAsEnum(ItemId)
table Item { name: string; }
```

生成稳定编号枚举和对应表的查询扩展：

```csharp
Item sword = data.Table<Item>().Get(ItemId.sword);
bool found = data.Table<Item>().TryGet(ItemId.shield, out var shield);
```

`Get` 在 id 不存在时抛出 `KeyNotFoundException`，`TryGet` 返回 false。字符串索引仍可按 CFD id 查询。映射包含当前记录，已有编号在增删记录后保持稳定。

## 按使用位置选择维度

```csharp
var japanese = sword.For("language", "ja");
var mobileJapanese = japanese.For("platform", "mobile");

var ordinaryName = sword.name;
var translatedName = japanese.name;
bool translated = sword.HasVariant(nameof(sword.name), "ja");

foreach (var variant in sword.Variants("language")) {
    var text = sword.For("language", variant).name;
    var explicitlyProvided = sword.HasVariant(nameof(sword.name), variant);
}
```

`For` 返回独立记录视图，多个界面可同时使用不同变体。链式调用组合维度，重复选择同一维度会替换该维度的选择。未知维度报错；已知维度中缺少字段覆盖或变体未知时，使用基础表达式。

视图中的引用、内联数据、集合和模板使用同一选择上下文。只有 name 提供日文覆盖时，基础模板 `f"购买 {self.name}"` 在日文视图中仍会使用日文 name。

`HasVariant(field, variant)` 检查该字段是否明确赋值。显式 `None`、空字符串和与基础值相同的覆盖均算存在。字段必须绑定维度；仅由回退产生值时返回 false。

`Variants(dimension)` 返回项目中该维度所有已知变体，包含当前记录尚未填写的变体。使用 `HasVariant` 判断具体字段是否填写。

## 可选值、字符串与诊断

`Option<T>` 提供 `IsSome`、`IsNone` 和 `Value`；读取 None 的 Value 会抛出异常。嵌套可选值保留每一层状态。只有 CFT 字段写有默认值时才可以在 CFD 中省略；例如 `note: Option<string> = None;`。

`string` 同时支持普通字面量和显式 `f"..."` 插值。普通字符串的花括号保持原样，插值读取当前视图的字段值。

初次加载会验证基础数据和已声明的各维度变体。首次组合多个维度若出现循环依赖等错误，`For` 会抛出 `CfdLoadException`，可通过其 `Diagnostics` 定位问题。CFT 业务规则通过 CLI 校验、构建或编辑器执行。

## 应用资源与重新加载

生成源码和 `Coflow.Loader` 使用相同的发布版本。业务 partial 扩展与生成类型放在相同命名空间和程序集，生成目录交给 codegen/build 管理，业务扩展文件放在目录外。

应用资源系统负责读取 CFD 文本：桌面程序可使用文件，Unity 可使用 TextAsset 或资源包。将所有相互引用的基础文件和需要的覆盖文件一起传给 Load；逻辑路径用于诊断，文件传入顺序不决定优先级。应用运行时使用 CFD、加载库和生成类型，CFT 在构建期用于检查和生成。

Unity 中可直接把资源文本传入，例如：

```csharp
var loaded = CoflowData.Load(new[] {
    new CfdSource("data/items.cfd", itemsAsset.text),
    new CfdSource("language/ja.cfd", japaneseAsset.text),
});
```

这里 itemsAsset 与 japaneseAsset 是应用已取得的 TextAsset。资源下载、异步读取和更新时机由应用决定。

重新加载时构造新的结果，成功后再替换业务使用的数据根：

```csharp
// result 是上面已成功加载的结果。
var current = result.Data;
var candidate = CoflowData.Load(new[] {
    new CfdSource("items.cfd", updatedItemsText),
    new CfdSource("ja.cfd", updatedJapaneseText),
});
if (candidate.Success) {
    current = candidate.Data;
} else {
    foreach (var diagnostic in candidate.Diagnostics)
        System.Console.WriteLine(diagnostic);
}
```

失败时继续使用旧数据。新旧加载结果的记录与视图属于各自的数据上下文；交换数据根后，应用按 ID 从新根重新获取记录和维度视图。多线程应用通过自己的同步方式一次性交换数据根。

加载会检查结构、字段类型、引用和模板依赖。CFT 业务规则通过 CLI 的 check --rules、build 或编辑器执行；数据更新交付前仍应完成规则校验。改变 CFT 结构后，重新生成并编译应用源码。
