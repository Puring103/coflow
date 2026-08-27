# C# Runtime API 设计

> 状态：当前 API 设计
>
> 范围：固定 C# Runtime 库、CFT 生成类型、CFD 数据加载、函数编译、C# VM、强类型访问、
> `@Host` singleton 注入、函数绑定、generation 与 reload

本文档只描述目标版本。旧 API 和旧命名直接删除，不保留兼容别名、自动转换、fallback 或其他
兜底入口。

## 1. 目标与约束

C# Runtime 是固定发布的库。应用运行时只提供 CFD 文本，不提供 CFT、数据 artifact 或字节码
artifact。CFT 在构建期生成 C# 类型以及这些类型内部使用的加载 metadata；C# Runtime 根据生成
metadata 直接加载 CFD，并可选择是否检查和编译函数。

公共 API 遵循以下约束：

- 顶层入口固定为 `Coflow.LoadData` 和 `Coflow.LoadAndCompile`，不是项目生成类。
- 两个入口只接受一个 CFD 字符串或 CFD 字符串数组。
- `LoadData` 只加载普通数据；其 generation 不能检查函数、绑定函数或调用函数。
- `LoadAndCompile` 加载普通数据，并对 CFD 函数完成名称解析、类型检查和字节码生成。
- C# VM 是 CFD 函数唯一的正式执行实现。
- C# Runtime 不加载 CFT，不计算或比较 CFT hash，也不执行 CFT check。
- codegen 只为 CFT 中实际声明的类型生成公开 C# 类型，不生成项目级数据根类型或 Runtime 包装类型。
- 应用不按字符串读取 type、field 或 function。普通 record key 本身仍可以是 `string`。
- 完整记录注入只允许显式声明为 `@Host @singleton` 的 CFT type。
- 只有 `LoadAndCompile` 返回的 `CoflowData` 可以绑定 `@Host` singleton 或普通函数字段。
- 按 key 查找记录只有 `CoflowTable<T>.Get(...)`：返回 Runtime 的 `Option<T>`，未找到时返回
  `None`。不提供返回 nullable、缺失时抛异常、接受默认值或采用 fallback 的查找入口。

Runtime 内部可以保留 AST、动态 value、layout、slot 和函数索引，但这些都不属于应用公共 API。

## 2. 生成代码边界

以下 CFT：

```cft
@idAsEnum(ItemId)
type Item {
  name: string;
  tags: [string] = [];
  backup: Option<&Item> = None;
}

enum ItemId {}

type Rule {
  evaluate: fn(int) -> Result<int, string>;
}
```

只生成 CFT 中存在的公开类型：

```csharp
public sealed class Item { /* generated fields and internal reader */ }
public enum ItemId { /* generated record keys */ }
public sealed class Rule { /* generated fields, call and bind methods */ }
```

生成器不得自行增加 `Record`、`Action`、`Model`、`Data` 或项目名称后缀。生成类型的名称直接
来自 CFT 声明。

本文中的 `Item`、`Rule`、`Services` 等名称都只是 CFT 业务声明示例，不是 Runtime 入口。
顶层 Runtime API 始终位于固定库的 `Coflow` 与 `CoflowData` 上。

每个生成类型同时包含或关联 Runtime 内部使用的：

- 规范 CFT type identity；
- 字段名称、字段类型和继承 metadata；
- 默认值和注解 metadata；
- 强类型 CFD reader；
- record key 类型信息；
- 函数签名、调用包装和绑定入口。

CFT 中声明在 `type`、`enum`、字段和 enum variant 上的全部内建或自定义注解都进入 metadata。
自定义注解保留原名称以及名称、string、int、float、bool 参数；codegen 不按一份封闭的内建注解
列表过滤或丢弃它们。

这些成员可以通过 internal 接口供 Runtime 使用，但应用不需要注册 binding、传入 schema 对象或
访问 descriptor。生成代码可以使用内部 module initializer 注册同一 CFT 项目的类型 metadata；
注册过程不形成新的公开项目类型。

## 3. CFT 到 C# 的类型映射

| CFT 类型 | C# 类型 |
| --- | --- |
| `()` | `Unit`；可清楚表达时 delegate 也可以返回 `void` |
| `int` | `long` |
| `float` | `double` |
| `bool` | `bool` |
| `string` | `string` |
| enum | 同名生成 enum |
| object type | 同名生成 class；`@struct` 时生成 readonly struct |
| record reference | 目标 CFT type 对应的生成 C# 类型 |
| `[T]` | `IReadOnlyList<T>` |
| `{K: V}` | `IReadOnlyDictionary<K, V>` |
| `Option<T>` | Runtime 的 `Option<T>` |
| `Result<T, E>` | Runtime 的 `Result<T, E>` |
| `fn(A) -> R` | `Func<A, R>`、`Action<A>`，或标准 delegate 无法清楚表达时生成的 delegate |

不为数组、字典、Option、Result 或普通 string key 生成 `XxxList`、`XxxMap`、`XxxOption`、
`XxxResult` 或 `XxxKey` 包装。

### 3.1 Record key

未声明 `@idAsEnum` 的 type 使用 `string` key：

```csharp
Option<Item> item = data.Table<Item>().Get("sword");
```

声明 `@idAsEnum(ItemId)` 的 type 使用生成 enum：

```csharp
Option<Item> item = data.Table<Item>().Get(ItemId.Sword);
```

record 的 `Id` 属性使用同一种 key 类型。普通 string key 是配置数据，不属于动态 schema API。

## 4. 顶层加载 API

Runtime 库提供固定入口：

```csharp
public static class Coflow
{
    public static CoflowData LoadData(string cfd);
    public static CoflowData LoadData(string[] cfdSources);

    public static CoflowData LoadAndCompile(string cfd);
    public static CoflowData LoadAndCompile(string[] cfdSources);
}
```

参数是 CFD 文本内容，不是文件路径。数组顺序参与稳定的 source identity；诊断使用
`source[0]`、`source[1]` 等来源名称。

### 4.1 `LoadData`

`LoadData`：

1. 解析 CFD 顶层记录、普通字段和函数 body 边界。
2. 使用生成 metadata 验证 record type、普通字段、默认值、继承、Option、集合、维度和引用。
3. 构建不可变数据 generation。
4. 不解析函数 body 内部表达式，不执行函数名称解析、类型检查、控制流检查或字节码生成。

返回的普通数据可以完整读取。函数调用和任何函数绑定都抛出
`CoflowFunctionNotCompiledException`。

### 4.2 `LoadAndCompile`

`LoadAndCompile` 先执行 `LoadData` 的全部数据阶段，然后：

1. 解析全部函数 body。
2. 完成项目级名称解析和类型检查。
3. 完成闭包捕获与调用目标分析。
4. 生成 generation-local 函数表和字节码。
5. 返回允许函数调用和绑定的 `CoflowData`。

函数编译失败时不返回部分可用的 `CoflowData`。

## 5. 强类型数据访问

`CoflowData` 和 `CoflowTable<T>` 都属于固定 Runtime 库：

```csharp
public sealed class CoflowData : IDisposable
{
    public bool FunctionsCompiled { get; }

    public CoflowTable<T> Table<T>();
    public T Singleton<T>();

    public Result<ReloadInfo, CoflowReloadError> Reload(string cfd);
    public Result<ReloadInfo, CoflowReloadError> Reload(string[] cfdSources);
}

public sealed class CoflowTable<T> : IReadOnlyList<T>
{
    // 仅当 T 未声明 @idAsEnum 时有效
    public Option<T> Get(string key);

    // 仅当 T 声明 @idAsEnum(TKey) 且参数正是该 TKey 时有效
    public Option<T> Get<TKey>(TKey key)
        where TKey : struct, Enum;
}
```

`Option<T>` 是固定 Runtime 库提供的泛型值类型，不是为每个 record 生成的包装类型。`Get`
命中时构造 `Some(record)`，未命中时构造 `None`；它不会用 `default(T)` 表示缺失。
`CoflowTable<T>` 不提供 `TryGet`、`GetRequired`、按 key 索引器或带默认值参数的另一组按 key
查找方法，也不为旧查找 API 保留别名。继承自 `IReadOnlyList<T>` 的索引器只按记录顺序和整数位置
访问，不能接受 record key，也不改变 `Get` 是唯一按 key 查找入口这一约束。

`Table<T>()` 只接受生成的非 singleton CFT type；`Singleton<T>()` 只接受生成的 singleton type。
传入不符合该类别的生成类型是 API 使用错误。Runtime 根据 `T` 的内部生成 metadata 定位类型，
不接收 type name 字符串。

`Get` 的返回值语义固定如下：key 存在时返回 `Some(record)`，key 不存在时返回 `None`。
缺失记录是正常的 Option 分支，不是加载错误或 VM fault。

```csharp
Option<Item> found = data.Table<Item>().Get(ItemId.Sword);

if (found.TryGetValue(out Item item))
{
    long price = item.Price;
}
```

这里的 `TryGetValue` 是 `Option<T>` 的解包方法，不是 `CoflowTable<T>` 上的另一种记录查找 API。

未声明 `@idAsEnum` 的 type 只接受 `string` key。枚举 key 重载只接受目标 type 通过
`@idAsEnum` 指定的生成 enum；传入其他 enum 属于 key 类型错误。两种重载不会互相转换，也不接受
enum 名称字符串作为替代输入。由于 C# 无法在 `CoflowTable<T>` 的单个泛型参数上表达关联的 enum
key 类型，`Get<TKey>` 的方法签名保证传入值是 enum，Runtime 再根据 `T` 的生成 metadata 验证
`TKey` 正是 `@idAsEnum` 指定的 enum；不匹配时立即报告 API 使用错误，不尝试按名称或底层整数转换。
`IReadOnlyList<T>` 枚举顺序是 generation 中稳定的记录顺序。

普通 singleton 在数据加载阶段必须恰好存在一条 CFD 记录，否则加载失败。`@Host` singleton
由 Runtime 创建未绑定位置，不要求 CFD 记录。

## 6. `@Host` Singleton

CFT 可以声明多个宿主 singleton：

```cft
@Host
@singleton
type Services {
  environment: string;
  limits: Limits;
  log: fn(string) -> ();
}
```

规则：

- `@Host` 必须与 `@singleton` 同时显式出现，且目标必须是具体 type。
- CFD 不得声明 `Services` 记录。
- codegen 生成的 `Services` 类型直接提供强类型 `Bind(...)`。
- `Bind` 参数由 CFT 字段按声明顺序生成，不使用 record key、type name 或 field name 字符串。
- 所有无默认值字段必须提供；有默认值字段可以由生成重载省略。
- 一个 generation 中每个 `@Host` singleton 只能成功绑定一次。
- 只有 `LoadAndCompile` 返回的数据允许绑定。

示例：

```csharp
using CoflowData data = Coflow.LoadAndCompile(cfdSources);

Services services = data.Singleton<Services>();
Result<Unit, HostBindError> bound = services.Bind(
    environment: "development",
    limits: limits,
    log: Log);

if (bound.IsOk)
{
    services.Log("started");
}
```

`"development"` 是 CFT `string` 字段的普通值。API 中没有宿主 record key 或动态字段名。

绑定成功后，普通字段编码进当前 generation 的只读宿主记录，delegate 进入同一 generation 的函数
表。绑定完成前读取该 singleton 或调用其函数产生 `CoflowHostNotBoundException`。

## 7. 普通函数绑定与调用

以下 CFT：

```cft
type Rule {
  priority: int;
  evaluate: fn(int) -> Result<int, string>;
}
```

生成同名 `Rule` 类型，不生成 `RuleRecord`：

```csharp
public sealed class Rule
{
    public string Id { get; }
    public long Priority { get; }

    public Result<long, string> Evaluate(long input);
    public Result<Unit, FunctionBindError> BindEvaluate(
        Func<long, Result<long, string>> implementation);
}
```

应用先以强类型 table 和 key 找到记录：

```csharp
Option<Rule> found = data.Table<Rule>().Get("combat");

if (found.TryGetValue(out Rule rule))
{
    Result<Unit, FunctionBindError> bound =
        rule.BindEvaluate(EvaluateCombat);
}
```

绑定规则：

- 只有已由 `LoadAndCompile` 编译的 generation 可以绑定。
- CFD 已为该字段提供 body 时，再绑定 C# delegate 返回重复实现错误。
- 同一字段已经成功绑定时，再次绑定返回重复绑定错误。
- delegate 的参数和返回类型由生成方法签名在 C# 编译期约束。
- delegate 抛出的未处理 C# 异常转换为 VM fault。
- CFT `Err(E)` 保持语言值，不转换为异常。

函数调用方不区分 CFD body 与 C# delegate。直接调用、函数值间接调用和高阶集合方法都使用同一
签名与同步调用顺序。

## 8. Generation 与 Reload

每次成功加载产生一个 generation。普通 CFD 记录、集合、引用和字节码在发布后不可变；
`@Host` singleton 和缺少 CFD body 的函数字段拥有 bind-once slot。绑定只填充这些预先编译并经过
签名验证的 slot，不改变普通 CFD 数据、记录身份或字节码。

`CoflowData` 持有当前 generation；从 table 或 singleton 取得的生成对象固定属于取得它们时的
generation。同一 generation 内的生成对象可以观察其 bind-once slot 从未绑定变为已绑定。

`Reload`：

1. 使用新的 CFD 字符串加载候选数据。
2. 如果当前数据由 `LoadAndCompile` 创建，则重新检查和编译全部候选函数。
3. 按生成 type、record key 和函数字段重新解析已有 `@Host` 输入及普通函数 delegate。
4. 全部成功后原子发布候选 generation。

reload 不加载 CFT、不比较 CFT hash，也不改变生成 C# 类型。失败时继续保留旧 generation。
旧生成对象继续观察旧 generation；reload 后重新执行 `Table<T>()`、`Singleton<T>()` 或 `Get`
才能取得新 generation 对象。

普通函数 delegate 的稳定绑定身份由生成 type metadata、record key 和函数字段共同确定。
`@idAsEnum` key 在 reload 时按对应 enum 值重新解析。

## 9. Fault 与错误

错误通道固定如下：

- `LoadData` 和 `LoadAndCompile` 返回 `CoflowData`；失败时抛出包含全部诊断的
  `CoflowLoadException`，函数编译诊断也属于该异常。
- `Bind` 返回 `Result<Unit, HostBindError>` 或 `Result<Unit, FunctionBindError>`。
- `Reload` 返回 `Result<ReloadInfo, CoflowReloadError>`，失败时保留旧 generation。
- record 查找只通过 `CoflowTable<T>.Get(...)` 完成，并返回 `Option<T>`；缺失记录是 `None`，
  Runtime 不提供 nullable、异常、带默认值或 fallback 的替代版本。

函数声明返回 `Result<T, E>` 时，生成方法直接返回该语言结果。VM fault 不是语言业务错误：

- 整数溢出、除零、预算超限和 VM 不变量破坏产生 `CoflowFaultException`。
- 未处理的 C# delegate 异常包装为 `CoflowFaultException`，并保留原异常作为 inner exception。
- fault 包含 CFD source 位置和简短 Coflow 调用栈。
- Runtime 不把 `Err(E)` 转换为异常。

## 10. 执行模型

Coflow 函数和 C# delegate 全部同步执行。生成方法返回 CFT 声明的结果，不返回 `Task<T>`。

应用可以自行把一次完整调用调度到线程池，但 delegate 不能暂停正在执行的 VM。当前 API 不提供
continuation、pending call、resume token、scheduler 或异步宿主函数。

每次调用固定使用目标生成对象所属的 generation。Runtime 对每次调用应用指令、调用深度和值栈
预算，这些是 Runtime 内部执行保护，不增加 `LoadData` 或 `LoadAndCompile` 参数。墙钟取消、外部
调度和可配置执行策略留给后续独立 API，不属于当前固定加载入口。C# VM 是正式执行实现；Rust
项目工具不执行这套 CFD 函数字节码。

## 11. CFT Check 边界

CFT check 暂不迁移到 C# Runtime：

- codegen 不生成 check body 或 check IR。
- `LoadData`、`LoadAndCompile` 和 `Reload` 都不执行 CFT check。
- C# Runtime 不公开 `Check()`。
- CFT check 继续由 Rust 项目工具按现有语义处理，且不能调用 CFD 函数或 C# delegate。

以后迁移 check 时需要单独设计，不预留当前公共 API。

## 12. 明确不公开的 API

C# Runtime 不公开：

- 按 type name、field name 或 function name 字符串访问；普通 string record key 除外；
- 动态 `CoflowValue` union；
- CFT parser 或 CFT schema 文件加载；
- schema hash 或 contract hash；
- 数据、schema 或字节码 artifact loader；
- generation-local ID、slot、layout、field offset 或 row pointer；
- 宿主 record key、动态 Host descriptor 或按字符串逐字段绑定；
- `XxxRecord`、`XxxKey`、`XxxList`、`XxxMap` 等非 CFT 声明包装类型；
- CFT check 执行；
- 异步函数或 `Task<T>` 映射。

这些限制保证固定 Runtime 库只提供通用加载、generation 和 VM 能力，项目公开类型严格来自 CFT
声明，应用通过泛型生成类型、普通 key 或 `@idAsEnum` enum 完成全部数据访问。
