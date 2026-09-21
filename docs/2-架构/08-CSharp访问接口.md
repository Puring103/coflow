# C# 访问接口

> 新 VM 的设计与验收以 [新 VM 实施计划](../plans/new-vm-and-csharp-snapshot.zh-CN.md) 及其配套规格为准；本文冲突内容在同一次交付中同步。

## 职责

| 部分 | 职责 |
| --- | --- |
| Rust 运行时 | 契约、CFD 解析与映像构建、来源编号、诊断、身份、Host 值校验、执行接口及批量快照编码 |
| C# 运行时 | C ABI 调用、批量快照解码、UTF-8 和标量转换、异常转换、Runtime/lease 所有权及只读集合 |
| 生成代码 | 契约加载入口、预期契约身份、静态类型绑定、派生类型工厂、本地属性/查询、函数/模板入口及强类型 Host 适配 |

C# 在构建时物化完整托管只读快照，不实现语言解析、语义查询规则或执行器。
`Runtime.Table<T>().Get(key)` 和 `Runtime.Get<T>()` 使用生成的 `TypeBinding<T>` 与内存数据索引。
类型与工厂由静态代码直接引用，IL2CPP 不通过反射寻找构造函数，也不动态生成托管代码。

## 构建

代码生成在同一输出目录产生 C# 绑定源码和 `coflow.contract`。应用读取契约文件的字节，调用
`Generated.LoadContract(bytes)` 创建契约，再传给 `Coflow.RuntimeBuilder`。契约文件由 Rust 生成并解析；
Rust 在加载时校验格式、版本、摘要、类型化 IR 和生成绑定所携带的预期身份。
`AddSource(text, sourceName)` 总是追加来源，名称仅用于诊断，省略时由 Rust 编号。
`AddSource` 和 `BindHost` 返回 builder。构建成功后 builder 不再接受修改和构建；
失败保留候选输入，可追加来源重试。重新配置已成功的实例必须使用新 builder。

构建失败由 Rust 返回结构化诊断，经 C ABI 传输后映射为 `BuildException.Diagnostics`。
诊断包括代码、来源、消息和可用的 UTF-8 字节偏移；C# 不重新解析错误文本。

## 所有权与身份

契约、builder、Runtime 和临时返回缓冲区使用拥有型原生句柄。
普通对象、记录和集合包装读取托管快照；函数、动态模板及含执行能力的动态返回图同时保存 Runtime、局部值 ID 和内部 lease。局部值 ID 从 1 编码，0 表示查询未找到。
值包装不提供 Dispose，不参与原生全局句柄表的逐值注册。Runtime 显式 Dispose 后普通快照仍可读，执行能力失效；没有显式释放时，SafeHandle 将终结请求转交创建线程回收域。

相同 Runtime 和记录 ID 构成记录包装的 C# 相等性与哈希；不同 Runtime 的记录不相等。
Host 返回已有记录时由 Rust 归一到原记录 ID。语言内容比较仍由 Rust `ValueEquals` 完成。
普通标量、字符串、data、记录和集合的托管投影与 Runtime 生命周期无关；记录传回 VM 时仍校验实例身份。

## 查询与 Host

Rust 在构建和投影解码时验证 table、singleton 与字段类别。table 查询包含派生类型，托管快照建立稳定顺序及本地查询索引。
枚举、数组、字典、维度和记录关系均在快照中类型化保存；普通查询不跨 FFI。
可选标量、枚举和 struct 使用 Nullable；可选引用类型使用 null。

每个 Host 服务生成强类型数据接口与静态适配器，`BindHost(host)` 调用生成的扩展方法。
适配器提供固定的服务名和签名，Rust 校验契约、重复绑定及返回值归属。
函数成员生成强类型 C# 方法，适配器按声明顺序解码参数并编码返回值，不使用反射或 `object[]`。
回调、异常边界和释放要求见 [Unity 原生集成](07-Unity原生集成.md)。

## 函数与检查

一等函数按签名生成 `CoflowFunction<T1, ..., TResult>`，使用 `Invoke(...)` 调用；无返回值使用
`Unit`。函数包装持有所属 Runtime，不重复接受 Runtime 参数。参数、返回值、闭包与 Host 函数
共用同一套静态 codec。C# 目标支持零到八个参数，超出范围时代码生成失败。

`fstring` 字段生成 `CoflowTemplate` 属性，获取属性不执行模板；`Render<字段名>()` 或包装的 `Render()` 显式进入执行边界。
`Runtime.RunChecks(CheckOptions)` 显式执行检查，可选择记录、规则名称、全局规则和执行预算，返回
`CheckResult`、结构化诊断及执行统计；C# 不缓存检查结果。

## 线程回收域

`Runtime.Dispose()` 只允许创建线程在没有活动执行、Host 回调、同步重入、导入或投影时调用。活动边界返回 RuntimeBusy，C# 抛出 `InvalidOperationException`，实例和句柄保持可用。

`RuntimeThread.DrainFinalizers()` 在创建线程处理调用开始时的终结请求快照；`RuntimeThread.Shutdown()` 在空闲时释放该线程域的全部本地资源。Unity 包通过主线程 PlayerLoop 每帧驱动回收，并在正常退出时关闭；其他宿主由创建线程事件循环驱动。重复请求、关闭后的迟到请求和重复关闭均不得双释放。
