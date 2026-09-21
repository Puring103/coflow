# C# 接入

Coflow.Runtime 面向 Unity 2022+、.NET Standard 2.1 和 IL2CPP。
通过 Unity Package Manager 引入运行时包，并安装目标平台对应的原生插件。

配置生成代码的目录和命名空间：

```yaml
codegen:
  - language: csharp
    dir: Generated
    namespace: Game.Config
```

运行 `coflow codegen`。默认命名空间为 `Coflow.Generated`，生成字段保留 CFT 名称。
输出目录同时包含 C# 源码和 `coflow.contract`；需要把契约文件作为运行时资源部署。

## 构建与查询

对于 `table Item { name: string; }`：

```csharp
using Coflow;
using Game.Config;

using var contract = Generated.LoadContract(contractBytes);
using var builder = new RuntimeBuilder(contract);
builder.AddSource("sword: Item { name: \"Sword\" }");
using var runtime = builder.Build();

var sword = runtime.Table<Item>().Get("sword");
string id = sword.Id;
string name = sword.name;
```

`contractBytes` 是部署的 `coflow.contract` 内容，例如 Unity 中可使用 `TextAsset.bytes`。
契约由 Rust 加载并校验，使用结束后与 builder、Runtime 一起释放。
`AddSource(text, sourceName: "角色配置")` 可以附带诊断标签，不需要真实文件名。
省略名称时自动编号；每次调用都追加来源，同名不表示替换。Unity 中可以直接提交 `TextAsset.text`。

`AddSource` 和 `BindHost` 支持链式调用。一次提交全部互相引用的数据后调用 `Build()`。
成功构建消耗 builder，之后不能修改或再次构建；失败时保留输入，允许追加缺失来源再构建。
更新数据或绑定时创建新的 builder 和 Runtime。

```csharp
var items = runtime.Table<Item>();
bool found = items.TryGet("sword", out var item);
foreach (var entry in items)
    UnityEngine.Debug.Log(entry.name);

// Settings 必须声明为 singleton。
var settings = runtime.Get<Settings>();
```

`Table<T>()` 仅接受 table，包含其派生类型记录，自动返回实际子类型包装。
索引器查找失败抛出 `KeyNotFoundException`，`TryGet` 返回 false。
`Get<T>()` 仅接受 singleton。相同 Runtime 内的同一记录包装相等，不同 Runtime 的记录不相等。

## 值与生命周期

| Coflow 类型 | C# 读取类型 |
| --- | --- |
| int、float、bool、string、enum | 对应 C# 值 |
| table、singleton、data | 生成类型 |
| `@struct sealed data` | 生成的 readonly struct |
| `[T]` | `CoflowArray<T>` |
| `{K: V}` | `CoflowDictionary<K, V>` |
| 可选标量、enum、struct | `T?` |
| 可选对象、字符串、集合、函数 | 可为 null 的对应包装或字符串 |
| 维度字段 | `CoflowDimension<T>` |

数组支持索引和枚举；字典支持索引、枚举及 `TryGetValue`。
维度字段通过 `Default()` 读取基础值，通过 `For("zh")` 读取回退后的变体值。

对象、struct、集合和函数包装不需要单独 Dispose。只需释放契约、builder 和 Runtime。
显式释放 Runtime 后，已经取得的普通记录属性、struct 和集合仍可读取；函数调用、模板执行和未加载记录
查询需要有效 Runtime。

函数按签名生成为 `CoflowFunction<T1, ..., TResult>`，通过 `Invoke(...)` 调用；无返回值使用 `Unit`。
函数的 `Source` 提供源码。读取 fstring 属性会执行模板并返回字符串，
`Get_<字段名>_Template().ProgramSource` 可读取模板源码。

`runtime.RunChecks()` 执行全部记录规则和全局规则，返回结构化诊断和执行统计。
`CheckOptions` 可以选择记录、规则名称、是否包含全局规则及执行预算。

## Host 服务

```cft
@Host
singleton Services {
  environment: string;
  log: fn(message: string) -> ();
}
```

生成代码提供 `IServices` 接口和 `BindHost` 扩展：

```csharp
sealed class ServicesHost : IServices
{
    public string environment => "Unity";
    public Unit log(string message)
    {
        UnityEngine.Debug.Log(message);
        return default;
    }
}

// 在 Build() 前绑定。
builder.BindHost(new ServicesHost());
```

宿主实现强类型数据属性和函数，无需填写成员名、类型字符串或处理无类型参数数组。
同一服务重复绑定时报错；缺少绑定仍允许构建，实际读取时报错。
对象和集合等复杂值必须来自正在调用的同一 Runtime。

## 错误

构建失败抛出 `BuildException`，`Diagnostics` 提供错误代码、来源标签、消息和可用的 UTF-8 字节范围。
失败不返回部分 Runtime。其他原生访问错误通过 `CoflowException` 报告；访问已释放 Runtime 的包装抛出 `ObjectDisposedException`。
Runtime 遵循 Lua 式单线程约定：一个 Runtime 实例只能从创建线程访问，不提供跨线程并发访问；
需要并发时为每个线程创建独立 Runtime。同线程同步重入沿用同一执行预算，host 回调重入读取与
调用不阻塞。
