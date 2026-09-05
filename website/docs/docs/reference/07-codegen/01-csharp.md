# C# 代码生成

C# generator 根据 CFT 生成配置类型和加载代码。生成目录只包含 `.cs` 文件，不复制 CFD 数据。

C# target 目前支持 `namespace` 选项：

```yaml
codegen:
  - language: csharp
    dir: Generated
    namespace: Game.Config
```

运行 `coflow codegen` 后，将生成目录和 `Coflow.Cfd.Runtime` 引入 C# 项目即可加载 CFD：

```csharp
var module = Game.Config.CoflowData.LoadAndCompile(new[] { itemsCfd, rulesCfd });

var items = module.Table(Item.Table);
var item = items.Get(ItemId.Sword);

var settings = module.Singleton<Settings>();
```

每个可查询记录类型都会生成 `Table`。字符串键直接传入字符串，使用 `@idAsEnum` 的类型传入对应
enum 值。找不到记录或 singleton 时返回 `Option<T>.None`。

同一次 `Load` 或 `LoadAndCompile` 调用中的 CFD 可以互相引用。不同 Module 之间不能建立记录引用。

需要统一查询多个独立 Module 时，可以创建 `CoflowModuleSet`：

```csharp
var view = Coflow.Modules(baseModule, featureModule);
var next = view.Replace(featureModule, replacement);
```

同一个 ModuleSet 中不能出现重复记录键、重复 singleton 或重复函数。`Replace` 返回新的
ModuleSet，不会修改原值。

`CoflowModule` 不提供原地 reload。需要更新数据时，重新加载一个新 Module，再用 `Replace` 发布
新的 ModuleSet；旧 Module 和旧查询结果保持有效。

Runtime 的应用 API 位于 `CoflowRuntime`。`CoflowRuntime.Generated` 是生成代码跨程序集调用的 ABI，
不属于应用 API，并从 IntelliSense 隐藏。

`@Host` 类型通过 singleton 获取并配置：

```csharp
var host = module.Singleton<HostServices>();
if (host.HasValue)
    host.Value.Configure(environment, log);
```

CFT `int` 生成 C# `long`，`float` 生成 C# `double`。CFT `@struct` 生成支持字段值相等比较的
C# 类。布尔值在 CFD 中写作小写 `true` 或 `false`。
