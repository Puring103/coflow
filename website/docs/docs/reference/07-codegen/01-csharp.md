# C# 代码生成

C# generator 根据 CFT 生成全局命名空间中的强类型 API 和 Schema 绑定代码。生成目录只包含 `.cs` 文件，不复制 CFD 数据。C# target 不接受额外选项：

```yaml
codegen:
  - language: csharp
    dir: Generated
```

将生成目录和 `Coflow.Runtime` 引入 C# 项目后，先创建运行时实例，再按 Module 加载 CFD，最后编译并发布：

```csharp
var coflow = Schema.Create(new CoflowOptions(
    maxInstructions: 10_000_000,
    maxFrameDepth: 1_024));
var baseModule = coflow.LoadModule(
    new CoflowSource("items.cfd", itemsCfd));
var rulesModule = coflow.LoadModule(
    new CoflowSource("rules.cfd", rulesCfd));

coflow.Bind(new HostServices(environment, log));

var result = coflow.Compile();
if (!result.Success)
    throw new CoflowLoadException(result.Diagnostics);

var item = coflow.Table(Item.Table).Get(ItemId.Sword);
var settings = coflow.Singleton<Settings>();
```

同一 `Coflow` 中的 Module 可以互相引用。`ReplaceModule` 和 `RemoveModule` 修改待编译状态；再次成功调用 `Compile` 后，新状态才会生效。编译失败时继续保留上一次成功发布的状态。

`CoflowOptions` 为每个实例设置执行限制。不传参数时使用默认限制。每次顶层函数调用使用一份新预算；同步 Host 回调再次调用同一实例时与外层调用共享预算。

同一 `Coflow` 中的记录可以跨 Module 引用，并支持前向引用、自引用和引用环。

每个可查询记录类型都会生成 `Table`。字符串键直接传入字符串，使用 `@idAsEnum` 的类型传入对应 enum 值。找不到记录或 singleton 时返回 `Option<T>.None`。

`@Host` 生成可直接构造的类型。一个 `Coflow` 可以绑定多个不同 Host 类型，同一类型再次 `Bind` 表示换绑，并在下一次成功编译后生效。

生成实例函数显式接收要执行的 `Coflow`：

```csharp
var damage = item.Value.Calculate(coflow, input);
```

CFT `int` 生成 C# `long`，`float` 生成 C# `double`。CFT `@struct` 生成真正的 `readonly struct`，并支持普通字段、集合、引用、`Result` 和函数字段。

CFT 函数值生成 `CoflowFunction<T1, ..., TResult>`。调用函数值时同样显式传入运行时实例：

```csharp
var operation = scenario.MakeOperation(coflow, options);
var output = operation.Invoke(coflow, input);
```

函数值可以出现在 class、struct、`Option`、`Result` 和集合中。默认函数值可以被构造和传递，但实际调用会报告函数未绑定。普通 C# delegate 不能作为 Coflow 数据传入；外部函数由 `@Host` 构造参数提供。
