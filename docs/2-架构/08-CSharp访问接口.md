# C# 访问接口

## 职责

| 部分 | 职责 |
| --- | --- |
| Rust 运行时 | 契约、CFD 解析与构建、来源编号、诊断、记录索引、字典查找、身份、Host 值校验和执行接口 |
| C# 运行时 | C ABI 调用、UTF-8 和标量转换、异常转换、Runtime 所有权及只读访问包装 |
| 生成代码 | 契约加载入口、预期契约身份、静态类型绑定、派生类型工厂、属性读取、可选值转换及强类型 Host 适配 |

C# 不维护第二份记录数据，不实现语言解析、查询规则或执行器。
`Runtime.Table<T>()` 和 `Runtime.Singleton<T>()` 使用生成的 `TypeBinding<T>`。
类型与工厂由静态代码直接引用，IL2CPP 不通过反射寻找构造函数，也不动态生成托管代码。

## 构建

代码生成在同一输出目录产生 C# 绑定源码和 `coflow.contract`。应用读取契约文件的字节，调用
`Generated.LoadContract(bytes)` 创建契约，再传给 `Coflow.RuntimeBuilder`。契约文件由 Rust 生成并解析；
Rust 在加载时校验格式、版本、摘要、字节码和生成绑定所携带的预期身份。
`AddSource(text, sourceName)` 总是追加来源，名称仅用于诊断，省略时由 Rust 编号。
`AddSource` 和 `BindHost` 返回 builder。构建成功后 builder 不再接受修改和构建；
失败保留候选输入，可追加来源重试。重新配置已成功的实例必须使用新 builder。

构建失败由 Rust 返回结构化诊断，经 C ABI 传输后映射为 `BuildException.Diagnostics`。
诊断包括代码、来源、消息和可用的 UTF-8 字节偏移；C# 不重新解析错误文本。

## 所有权与身份

契约、builder、Runtime 和临时返回缓冲区使用拥有型原生句柄。
对象、集合、函数和模板只保存 Runtime 与局部值 ID。局部值 ID 从 1 编码，0 表示查询未找到。
这些包装不分配独立原生句柄，不提供 Dispose，不参与原生全局句柄表的逐值注册。
Runtime 显式 Dispose 后全部依赖访问失效；没有显式释放时，包装保活 Runtime，SafeHandle 负责终结回收。

相同 Runtime 和记录 ID 构成记录包装的 C# 相等性与哈希；不同 Runtime 的记录不相等。
Host 返回已有记录时由 Rust 归一到原记录 ID。语言内容比较仍由 Rust `ValueEquals` 完成。
普通标量和字符串复制后与 Runtime 生命周期无关。

## 查询与 Host

Rust 验证 table 和 singleton 查询类别。table 查询包含派生类型，构建阶段建立查询索引。
枚举只逐项返回值 ID，C# 不缓存第二份表；字典查找将生成器选定的键编码提交 Rust。
可选标量、枚举和 struct 使用 Nullable；可选引用类型使用 null。

每个 Host 服务生成强类型数据接口与静态适配器，`BindHost(host)` 调用生成的扩展方法。
适配器提供固定的服务名和签名，Rust 校验契约、重复绑定及返回值归属。
函数成员生成强类型 C# 方法，适配器按声明顺序解码参数并编码返回值，不使用反射或 `object[]`。
回调、异常边界和释放要求见 [Unity 原生集成](07-Unity原生集成.md)。

## 函数与检查

一等函数按签名生成 `RuntimeFunction<T1, ..., TResult>`，使用 `Invoke(...)` 调用；无返回值使用
`Unit`。函数包装持有所属 Runtime，不重复接受 Runtime 参数。参数、返回值、闭包与 Host 函数
共用同一套静态 codec。C# 目标支持零到八个参数，超出范围时代码生成失败。

`fstring` 字段的普通属性读取执行模板并返回字符串，`Get_<字段名>_Template()` 返回模板句柄以读取源码。
`Runtime.RunChecks(CheckOptions)` 显式执行检查，可选择记录、规则名称、全局规则和执行预算，返回
`CheckResult`、结构化诊断及执行统计；C# 不缓存检查结果。
