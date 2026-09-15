# C# 代码生成

Coflow 为 Unity 2022+ 生成强类型 C# 包装，支持 .NET Standard 2.1 和 IL2CPP。
通过 Unity Package Manager 引入 Coflow.Runtime，并安装目标平台对应的原生库。
生成目录包含类型声明和加载契约的入口，CFD 数据由应用提供。

在项目配置中指定输出目录和 C# 命名空间：

```yaml
codegen:
  - language: csharp
    dir: Generated
    namespace: Game.Config
```

运行 `coflow codegen` 更新生成目录。默认命名空间为 `Coflow.Generated`；
类型、字段和 enum 成员保留 CFT 名称，CFT 命名空间映射为嵌套的 C# 命名空间。

## 加载与读取

对于 `table Item { name: string; }`：

```csharp
using Coflow.Runtime;
using Game.Config;

using var contract = CoflowSchema.Load();
using var builder = contract.CreateBuilder();
builder.AddSource("items.cfd", "sword: Item { name: \"Sword\" }");
using var runtime = builder.Build();
using var sword = Item.Wrap(runtime.Record("Item", "sword"));
string name = sword.name;
string id = sword.Id;
```

一次提交所有需要互相引用的 CFD 来源，再调用 `Build()`。构建成功后数据只读；
修改来源或 Host 绑定时创建新的构建器和运行时。生成类型必须与加载的契约匹配。

## 值与生命周期

普通字符串、数字、布尔和 enum 按值读取。对象、数组、字典、可选值与函数包装
支持 `Dispose()`，使用结束后释放。显式释放运行时后，属于它的包装不能继续访问。

数组支持索引和枚举；字典支持索引、枚举及 `TryGetValue`。
可选值通过 `HasValue` 和 `GetValue()` 访问。
维度字段通过 `Default()` 读取基础值，通过 `For("zh")` 读取回退后的指定变体。

函数和模板保留源码。当前版本暂不提供函数执行、模板求值和 check 执行。
读取 fstring 文本会报告未实现；调用生成的 `Get_<字段名>_Template()`
可获取模板包装，通过 `ProgramSource` 查看源码。

## Host 服务

使用 `@Host singleton` 声明服务，并在构建前调用
`builder.BindHost("服务限定名", host)`。宿主实现 `ICoflowHost`：
`MemberType` 返回成员的 Coflow 类型，`Read` 提供字段值。
标量可以直接返回；对象、集合和模板等复杂值返回同一运行时的现有包装。

服务可以缺少绑定，实际访问时报告错误。宿主回调异常转换为 `CoflowException`。
同一个运行时并发执行会报告忙错误；空闲后可以换线程使用。
