# API 与 Runtime 设计

> 状态：内部设计契约
>
> 范围：生成代码、契约加载、Rust Runtime、C# 包装和 Host 边界。

基础语言语义见 `01-language-design.zh-CN.md`，VM 见 `03-vm-design.zh-CN.md`。对外 C# 用法只记录在
`website/docs/docs/reference/07-codegen/01-csharp.md`。

## 1. 所有权边界

Rust 是唯一权威 Runtime。`coflow-core` 持有不可变契约、数据模型、运行时值、函数程序、VM、check
和 Host 抽象；`coflow-project` 负责项目配置、文件发现、CFT 编译、CFD 加载、诊断、写回和产物发布；
`coflow-ffi` 只暴露句柄式 C ABI 并适配 Host 回调。

C# `Coflow.Runtime` 不解析 CFT/CFD，不保存第二份契约或记录模型，也不编译、链接或执行 Coflow 函数。
它负责 native 库生命周期、句柄封装、值转换、强类型集合与 Host 回调适配。

## 2. 生成产物

C# 代码生成以 Rust 已编译的 `CftSchema` 为输入，在目标目录原子发布：

```text
generated/
├── coflow.contract
├── Coflow.Bindings.cs
├── Coflow.Host.cs
└── <schema types>.cs
```

`coflow.contract` 是 Rust 序列化的版本化二进制契约，包含类型、字段、默认值、函数程序和运行所需元数据。
生成的 `.cs` 文件只包含业务类型、构造读取代码和函数包装，不嵌入另一份契约对象。契约文件与生成代码必须来自同一次
原子发布，Runtime 对格式版本和内容完整性执行校验。

## 3. C# 加载流程

应用以契约字节和一个或多个 CFD 文本来源创建 Runtime：

```csharp
using var contract = Generated.LoadContract(contractBytes);
using var builder = new RuntimeBuilder(contract);
builder.AddSource(File.ReadAllText(dataPath), "gameplay");
using var runtime = builder.Build();
```

`Generated.LoadContract` 校验契约与生成绑定的 identity。`AddSource` 将文本和可选诊断标签传入 Rust；
C# 不自行发现项目文件。项目级路径解析属于 `coflow-project` 和 CLI/Editor，嵌入式 C# 调用方显式
提供输入。

Builder 累积的 CFD 来源不创建语言命名空间。Rust 在 `Build()` 时统一解析记录、引用、默认值和函数，
成功后返回一个不可变 Runtime；构建失败抛出带结构化诊断的 `BuildException`，不返回部分 Runtime。
成功构建会消耗 builder；更新数据或 Host 绑定需要创建新的 builder 和 Runtime。

## 4. 值与查询

Rust 值由 Runtime 句柄和 `ValueId` 标识。表与 singleton 查询先取得记录身份；一条记录首次访问时，Rust
一次性编码该记录及其内联 struct、集合、字典和维度值。对其他记录的引用只传 `ValueId` 和实际类型，C#
按需读取目标记录。Runtime 维护固定记录的 `ValueId -> object` 缓存；动态 data 的对象缓存由各自返回图持有。构造函数进入时先发布实例，因此循环引用、
继承和多态共享同一对象身份。这里不存在整张固定数据快照，也不存在逐字段 FFI 读取。

生成的非 Host 记录是普通 C# 属性和一个内部 `Record` 构造函数。构造函数直接完成字段赋值；构造完成后，
标量、struct、集合、字典和维度读取均为纯托管访问。函数字段只保存轻量执行句柄，调用时才跨 FFI 进入 VM。
动态函数返回值使用独立的值图和 lease；图内动态对象身份保持稳定，图中的固定记录引用仍解析到 Runtime 的固定记录缓存。丢弃返回图后，动态对象及其循环引用可整体回收。

生成绑定提供强类型表、记录、data、enum、维度值和函数入口。可选值映射为 C# nullable/引用可空语义；
集合暴露只读包装。不同 Runtime 的执行能力不能混用。Runtime 释放后，已经构造的普通数据仍可读取；函数、
模板执行和未加载记录查询不再有效。

## 5. Host

`@Host` 只适用于具体 singleton。生成的 Host 接口描述需要由应用实现的数据读取和函数调用。C# 将实现
注册为 native callback；Rust VM 在实际访问时同步调用该 callback，并按声明类型验证参数和返回值。

未绑定 Host 不阻止加载普通数据，只在实际读取或调用时产生明确 fault。回调期间允许受控的同步重入；
线程占用、递归深度和工作量继续受同一顶层执行预算约束。外部 delegate 不能作为普通 Coflow 函数值
写入数据。

## 6. 诊断与 Fault

加载和发布错误使用结构化诊断，包含稳定代码、阶段、严重级别、来源和 UTF-8 字节范围。C# 只转换该
结构，不解析消息文本。执行 fault 包含来源、调用栈和 fault 类别，通过 FFI 错误结果返回。

## 7. 线程与生命周期

Contract、Builder、Runtime、编译器、返回缓冲和动态值 lease 的原生句柄全部归创建线程所有。
FFI 使用线程本地句柄表；全局原子计数仅生成唯一身份，不承担资源同步。同线程 Host 重入不持有句柄表借用。
Host 服务无需实现 `Send`，执行状态、报告和预算使用同线程内部可变性。

C# 使用显式 `IDisposable`，调用方在创建线程通过 `using` 或 `Dispose()` 释放资源。终结线程不调用原生 API，
不设延迟终结队列或 Unity 更新泵。`RuntimeThread.Shutdown()` 在当前线程空闲时释放全部句柄，并使旧托管句柄失效；
活动执行期间释放 Runtime 或关闭线程域会失败且保留原状态。

参数编码器持有全部实参投影直到原生执行和结果接管结束，执行目标在整个调用期间保持存活。编码期间的托管 GC 与同步重入不会提前释放实参 lease。

动态投影通过弱引用登记在 Runtime 中。托管 GC 后，下一次 Runtime 请求在创建线程释放已死亡投影的 lease；
Runtime 释放或线程域关闭时释放其余 lease。已物化的普通业务数据保持可读；Runtime、表查询和执行能力仍受创建线程约束。

## 8. 固定不支持

- C# 侧读取或编译 CFT、解析 CFD、发现项目文件。
- 在生成 `.cs` 中嵌入或重建完整契约。
- C# 侧维护第二份可执行记录模型、函数 IR、VM Arena 或 check 执行器。
- 运行时反射发现字段、函数或 Host 方法。
- 在不同 Runtime 或已释放 Runtime 之间复用值句柄。

## 9. 实现边界

C# 的 `Contract.cs` 管理类型绑定与构建，`Runtime.cs` 管理实例生命周期与对象缓存，`Projection.cs` 管理值投影，`Invocation.cs` 管理调用与编码。
FFI 线程本地表维护 Runtime 身份到句柄的直接索引。参数解码共享内存与深度上限，字段和字典项直接解码单个值；记录图与动态图共用编码遍历，固定记录只传身份引用。
