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

运行 `coflow codegen --project .` 后，将匹配版本的 Loader 与全部生成文件加入 C# 项目。生成的类型、字段与命名空间需要符合 C# 标识符规则；不能表示的名称会在生成时报告错误，请修改 CFT 名称后重新生成。

## 获取 Loader

先用 `coflow --version` 确认版本，从 [Coflow 发布页](https://github.com/Puring103/coflow/releases)下载同版本 `vX.Y.Z` 的源码，或执行：

```sh
git clone --depth 1 --branch vX.Y.Z https://github.com/Puring103/coflow.git dependencies/coflow
dotnet add app/App.csproj reference dependencies/coflow/runtimes/csharp/Coflow.Loader/Coflow.Loader.csproj
```

将 `X.Y.Z` 替换为实际 CLI 版本，将应用路径替换为自己的项目。CLI 安装包不包含 Loader 源码。将生成目录放入应用的编译范围；目录在应用外时用 `Compile Include` 显式包含全部生成 `.cs` 文件。

Unity 2022+ 可从同版本源码构建 `netstandard2.1` Loader DLL，加入项目并配置程序集引用，同时导入全部生成源码。已有 Loader 时保持单份引用。

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
var settings = data.Singleton<Settings>().Base;
var baseName = sword.Base.name;
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

生成字段对外只读，直接保存最终值。记录和单例具有 `string id`。维度字段保持原本的业务类型，通过维度容器选择基础或变体对象。C# 关键字名称在生成源码中自动使用 `@` 转义。

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
Dimension<Item> sword = data.Table<Item>().Get(ItemId.sword);
bool found = data.Table<Item>().TryGet(ItemId.shield, out var shield);
```

`Get` 在 id 不存在时抛出 `KeyNotFoundException`，`TryGet` 返回 false。字符串索引仍可按 CFD id 查询。映射包含当前记录，已有编号在增删记录后保持稳定。

将 `coflow.enum.lock.json` 纳入版本控制。同一枚举中的记录 ID 必须具有不同编号；合并锁文件时保留已发布映射并解决编号冲突，再重新生成。

## 按使用位置选择维度

```csharp
var japanese = sword.For("language", "ja");
var ordinaryName = sword.Base.name;
bool translated = sword.HasVariant(nameof(Item.name), "ja");
var japaneseData = data.For("language", "ja");
Item sameSword = japaneseData.Table<Item>()["sword"];
```

`Table<T>()` 中的记录是 `Dimension<T>`，`Singleton<T>()` 返回 `Dimension<T>`。`.Base` 获取基础数据，`.For` 获取已构建的变体。数据根的 `Base` 和 `For` 返回完整数据集，其中的记录与单例是普通业务对象。两个入口共享对象身份，读取与选择都不会构建业务数据。

加载默认构建基础数据和全部已声明单维变体。附加多维组合在加载时明确指定：

```csharp
var selection = new DimensionSelection(
    ("language", "ja"), ("platform", "mobile"));
var result = CoflowData.Load(sources, new LoadOptions {
    Combinations = new[] { selection }
});
if (result.Success) {
    Item item = result.Data.Table<Item>()["sword"].For(selection);
}
```

组合顺序不影响结果。未知维度、未知变体和未构建组合抛出 `ArgumentException`；`TryFor(selection, out value)` 返回 false。未知变体错误列出已加载变体。已知变体缺少覆盖时，加载阶段使用基础表达式并在所选上下文完成计算。

`Variants(dimension)` 列出已加载变体，包含空覆盖块。`HasVariant(field, variant)` 判断显式赋值，包含 None、空字符串和同值覆盖；字段名使用 `nameof(Item.name)`。仅由回退产生值时返回 false。

## 可选值、字符串与诊断

`Option<T>` 提供 `IsSome`、`IsNone` 和 `Value`；读取 None 的 Value 会抛出异常。嵌套可选值保留每一层状态。只有 CFT 字段写有默认值时才可以在 CFD 中省略；例如 `note: Option<string> = None;`。

`string` 同时支持普通字面量和显式 `f"..."` 插值。普通字符串的花括号保持原样，插值读取当前视图的字段值。

加载完成所有请求的数据，包括引用、集合和模板结果。任一请求组合出现依赖错误时，通过 `LoadResult.Diagnostics` 报告，不交付部分数据。CFT 业务规则通过 CLI 校验、构建或编辑器执行。

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
