# API 与 Runtime 设计

> 状态：内部设计契约
>
> 范围：生成代码、契约加载、Rust Runtime、C# 包装和 Host 边界。

基础语言语义见 `01-language-design.zh-CN.md`，VM 见 `03-vm-design.zh-CN.md`。对外 C# 用法只记录在
`website/docs/docs/reference/07-codegen/01-csharp.md`。

## 1. 所有权边界

Rust 是唯一权威 Runtime。`coflow-core` 持有不可变契约、数据模型、运行时值、函数程序、VM、check
和 Host 抽象；`coflow-runtime` 负责项目配置、文件发现、CFT 编译、CFD 加载、诊断、写回和产物发布；
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
生成的 `.cs` 文件只包含业务类型和访问包装，不嵌入另一份契约对象。契约文件与生成代码必须来自同一次
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
C# 不自行发现项目文件。项目级路径解析属于 `coflow-runtime` 和 CLI/Editor，嵌入式 C# 调用方显式
提供输入。

Builder 累积的 CFD 来源不创建语言命名空间。Rust 在 `Build()` 时统一解析记录、引用、默认值和函数，
成功后返回一个不可变 Runtime；构建失败抛出带结构化诊断的 `BuildException`，不返回部分 Runtime。
成功构建会消耗 builder；更新数据或 Host 绑定需要创建新的 builder 和 Runtime。

## 4. 值与查询

Rust 值由 Runtime 句柄和 `ValueId` 标识。生成类型保存访问所需的轻量包装，不复制整张数据图。
字段、表、singleton、集合、函数和模板读取都通过固定 FFI 操作回到所属 Runtime。不同 Runtime 的值
不能混用。

生成绑定提供强类型表、记录、data、enum、维度值和函数入口。可选值映射为 C# nullable/引用可空语义；
集合暴露只读包装。所有权由 `CoflowRuntime` 控制，释放 Runtime 后其派生句柄不再有效。

## 5. Host

`@Host` 只适用于具体 singleton。生成的 Host 接口描述需要由应用实现的数据读取和函数调用。C# 将实现
注册为 native callback；Rust VM 在实际访问时同步调用该 callback，并按声明类型验证参数和返回值。

未绑定 Host 不阻止加载普通数据，只在实际读取或调用时产生明确 fault。回调期间允许受控的同步重入；
线程占用、递归深度和工作量继续受同一顶层执行预算约束。外部 delegate 不能作为普通 Coflow 函数值
写入数据。

## 6. 诊断与 Fault

加载和发布错误使用结构化诊断，包含稳定代码、阶段、严重级别、来源和 UTF-8 字节范围。C# 只转换该
结构，不解析消息文本。执行 fault 包含来源、调用栈和 fault 类别，通过 FFI 错误结果返回。

## 7. 固定不支持

- C# 侧读取或编译 CFT、解析 CFD、发现项目文件。
- 在生成 `.cs` 中嵌入或重建完整契约。
- C# 侧维护记录图、函数 IR、VM Arena 或 check 执行器。
- 运行时反射发现字段、函数或 Host 方法。
- 在不同 Runtime 或已释放 Runtime 之间复用值句柄。
