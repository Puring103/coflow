# 首次接入 C# / Unity

目标是让应用引用 Loader 与生成类型，提供 CFD 文本并成功访问记录。先确认应用目标框架、配置项目位置和资源读取方式。

## 获取匹配的 Loader

1. 用 `coflow --version` 确认生成器版本，查看应用是否已引用 Coflow.Loader。
2. 若尚未取得 Loader，从 [Coflow 仓库](https://github.com/Puring103/coflow) 的同版本 tag `vX.Y.Z` 获取源码；只安装 CLI 或 skills 不会提供这个源码目录。可下载该 tag 源码，或在依赖目录执行 `git clone --depth 1 --branch vX.Y.Z https://github.com/Puring103/coflow.git <依赖目录>`，将 X.Y.Z 替换为实际版本。
3. Loader 位于所取源码的 `runtimes/csharp/Coflow.Loader/`。在 .NET 应用中引用其中的 Coflow.Loader.csproj，例如 `dotnet add <应用.csproj> reference <源码目录>/runtimes/csharp/Coflow.Loader/Coflow.Loader.csproj`。

该项目面向 netstandard2.1 与 net8.0。这里使用源码项目引用；仓库中的 PackageId 不等于已发布、可用的 NuGet 包。版本 tag 不存在时先核对 CLI 来源与版本，不猜测替代包。

Unity 2022+ 可使用相同版本源码编译出的 netstandard2.1 Loader DLL，并把生成 C# 源码放入业务程序集。按应用现有 asmdef 和插件导入设置连接引用；已有 Loader 时避免重复导入。需要 DLL 时可执行 `dotnet build <Loader.csproj> -c Release -f netstandard2.1`。

## 生成与编译

在 coflow.yaml 中配置 C# namespace 和专用输出目录，例如 Game.Config 与 generated/csharp，然后：

```sh
coflow build --project <配置项目> --json
```

将生成的全部 .cs 文件加入应用编译。若输出在应用目录之外，通过 csproj Compile Include 或现有资源流程显式包含；业务 partial 扩展和生成类型使用同 namespace、同程序集，并保存在生成目录外。生成源码和 Loader 使用同发布版本。

## 提供数据并访问

以下示例假定已生成 Game.Config.Item，CFD 中有 sword 记录、name 字段，应用取得完整基础数据与覆盖文本：

```csharp
using Coflow.Loader;
using Game.Config;

var loaded = CoflowData.Load(new[] {
    new CfdSource("data/items.cfd", itemsText),
    new CfdSource("language/ja.cfd", japaneseText),
});
if (!loaded.Success) {
    foreach (var diagnostic in loaded.Diagnostics)
        System.Console.WriteLine(diagnostic);
    return;
}
// 只有加载成功后才能使用数据根。
var data = loaded.Data;
var sword = data.Table<Item>()["sword"];
var baseName = sword.name;
var japaneseName = sword.For("language", "ja").name;
```

itemsText/japaneseText 由应用读取，不是文件路径字符串。桌面应用可读文件，Unity 可使用已有 TextAsset.text；若将 CFD 作为 TextAsset 导入，可由资源流程复制成 Unity 支持的文本资产扩展名，CfdSource 的逻辑路径仍用于诊断。将全部相互引用的基础与所需覆盖文件一起提供，传入顺序不决定覆盖优先级。

`Table<T>()` 提供索引、TryGetValue、Count 和遍历，父类型表包含派生记录。singleton 用 `data.Singleton<Settings>()`。data 是内联对象，不存在对应记录表。

## 类型和维度

- table、singleton、默认 data 生成 partial class，`@struct sealed data` 生成 partial struct。
- 数值、bool、string 映射为同名 C# 类型；数组、字典为只读集合，引用为业务对象。
- Option 使用 `Coflow.Loader.Option<T>` 的 IsSome、IsNone、Value；读取 None.Value 会抛出异常，嵌套层需逐层判断。
- `@idAsEnum(ItemId)` 生成表的 `Get(ItemId.sword)` 与 `TryGet(ItemId.sword, out var item)` 扩展；前提是 CFT 已声明对应空 enum 和标注 table。
- `For("language", "ja")` 返回独立视图，链式 For 组合维度。引用、内联值和模板随视图上下文读取；基础对象不会被切换。
- `HasVariant(nameof(sword.name), "ja")` 判断显式覆盖；`Variants("language")` 列出维度已知变体，不代表每个字段已填写。

已知维度中的未知变体回退基础表达式，未知维度报错。首次构造多维组合可能抛出带 Diagnostics 的 CfdLoadException，应按应用错误处理方式呈现。

## 重新加载和验收

重新读取资源并创建新的 Load 结果；仅在 Success 时一次性交换业务数据根，失败保留旧根。旧记录、引用和视图属于旧上下文，交换后按 ID 从新根重新取得，不混用对象身份。

Loader 校验结构、字段、引用和模板，CFT 业务 check 在交付前由 CLI/editor 执行。验收包括：配置 build 成功、应用编译成功、实际 CFD 加载成功、预期记录及维度值可读。仅 codegen 成功不足以说明应用已接通。
