# C# Runtime API

> 状态：当前实现契约

`Coflow.Cfd.Runtime` 负责根据构建期生成的 contract 解析 CFD、构造强类型对象，并可选地编译和
执行 CFD 函数。Runtime 不加载 CFT，不扫描目录，也不读取 JSON、MessagePack 或字节码 artifact。

## 1. API 边界

公开类型分为两个明确层次：

- `CoflowRuntime` 是应用 API，包括 Module、Table、`Option<T>`、`Result<T, TError>`、诊断和 fault。
- `CoflowRuntime.Generated` 是生成代码 ABI，包括 CFD AST、reader、metadata、字段 binding 和函数
  adapter。该命名空间的类型为了支持跨程序集生成代码而保持 `public`，但不属于应用 API，并从
  IntelliSense 隐藏。

Runtime 的公开 API 由 `PublicAPI.Shipped.txt` 固定。修改应用 API 或生成代码 ABI 都必须显式更新
基线，并按兼容性影响选择版本号。

## 2. 加载入口

每个 C# codegen 目标在配置的命名空间中生成唯一入口 `CoflowData`：

```csharp
public static class CoflowData
{
    public static CoflowModule Load(string cfd);
    public static CoflowModule Load(string[] cfdSources);
    public static CoflowModule LoadAndCompile(string cfd);
    public static CoflowModule LoadAndCompile(string[] cfdSources);
}
```

参数是 CFD 文本，不是文件路径。调用方负责读取文件、下载文本或从资源系统取得内容：

```csharp
var module = Game.Config.CoflowData.LoadAndCompile(new[]
{
    File.ReadAllText("data/items.cfd"),
    File.ReadAllText("data/rules.cfd"),
});
```

`Load` 构造全部普通数据，但跳过 CFD 函数的解析、检查和编译。调用函数字段会抛出
`CoflowFunctionNotCompiledException`，配置 `@Host` 也会失败。

`LoadAndCompile` 在加载数据后检查并编译所有函数。任何解析、数据构造或函数编译错误都会抛出
`CfdLoadException` 的具体子类，不会返回部分可用的 Module。语法错误使用 `CfdParseException`，
contract 驱动的数据或函数错误使用 `CoflowLoadException`。

`Coflow.LoadData` 和 `Coflow.LoadAndCompile` 是 `CoflowData` 内部调用的生成 ABI，不是应用入口。

## 3. 类型和查询

主要类型映射如下：

| CFT | C# |
| --- | --- |
| `()` | `Unit` |
| `int` | `long` |
| `float` | `double` |
| `bool` | `bool` |
| `string` | `string` |
| `[T]` | `IReadOnlyList<T>` |
| `{K: V}` | `IReadOnlyDictionary<K, V>` |
| `Option<T>` | `Option<T>` |
| `Result<T, E>` | `Result<T, E>` |
| `fn(A) -> R` | 精确的 `Func<A, R>`、`Action<A>` 或生成 delegate |

每个可查询 record type 生成一个强类型 table token：

```csharp
var items = module.Table(Item.Table);       // CoflowEnumTable<Item, ItemId>
var rules = module.Table(Rule.Table);       // CoflowStringTable<Rule>

Option<Item> item = items.Get(ItemId.Sword);
Option<Rule> rule = rules.Get("default");
```

找不到 table 时返回共享空表，找不到 key 时返回 `Option<T>.None`。Table 同时实现
`IReadOnlyList<T>`，整数索引只表示稳定的记录顺序。

普通 singleton 和 `@Host` singleton 都通过以下入口查询：

```csharp
Option<Settings> settings = module.Singleton<Settings>();
```

## 4. Module 与 ModuleSet

`CoflowModule` 是一次独立加载产生的不可变数据和函数快照。同一次加载的多个 CFD source 可以互相
引用；不同 Module 不能建立记录、常量或函数引用。

`CoflowModuleSet` 提供多个独立 Module 的组合查询视图：

```csharp
var current = Coflow.Modules(baseModule, featureModule);
var next = current.Replace(featureModule, replacement);
```

`Add`、`Remove` 和 `Replace` 都返回新的 ModuleSet，不修改旧视图或 Module。组合要求所有 Module
使用同一个 generated contract，并拒绝重复 record key、singleton 或函数身份。

Runtime 不提供原地 reload、generation gate 或 `IDisposable` 生命周期。重新加载由应用创建新
Module，然后通过 `Replace` 发布新的 ModuleSet；此前取得的对象和旧 ModuleSet 继续保持有效。

## 5. Host

`@Host @singleton` 由 Runtime 创建，不允许在 CFD 中声明。生成类型提供强类型 `Configure`：

```csharp
var host = module.Singleton<HostServices>();
if (host.TryGetValue(out var services))
    services.Configure(environment, log, calculate);
```

配置可以重复执行，已有生成对象会读取更新后的字段和 delegate。Host 配置不提供跨线程同步；调用方
必须保证 `Configure` 不与读取或函数调用并发执行。

## 6. 函数与 fault

生成类型只公开按 CFT 签名生成的强类型方法。内部 `CoflowFunctionEntry<TDelegate>` 属于
`CoflowRuntime.Generated` ABI，不是应用查询接口。

语言值 `Result<T, E>` 不会转换为异常。整数溢出、除零、无效 VM 状态或未处理的 Host delegate
异常产生 `CoflowFaultException`；fault 包含函数身份、source span、来源路径和 Coflow 调用栈。

所有 CFD 和 Host 函数同步执行。Runtime 不提供 `Task<T>`、continuation、取消或 scheduler API。

## 7. 固定边界

Runtime 不公开或支持：

- CFT parser、CFT check 或 schema hash；
- 按字符串读取 type、field 或 function；
- 跨 Module 引用或组合时重定位；
- 原地 reload、自动 Host 状态迁移或 Module disposal；
- JSON、MessagePack、数据 artifact 或字节码 artifact；
- 异步 CFD 函数。
