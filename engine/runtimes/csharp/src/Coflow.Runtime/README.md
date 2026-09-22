# Coflow Runtime for Unity

支持 Unity 2022+ 的 .NET Standard 2.1 API 级别和 IL2CPP。通过 Unity Package Manager
的本地包入口选择此目录中的 `package.json`，把对应目标平台的 `coflow_ffi` 原生库放入
Unity 工程的 `Assets/Plugins`，并在 Plugin Inspector 中设置平台和 CPU。

把代码生成目录中的 `coflow.contract` 作为运行时资源部署，使用
`Generated.LoadContract(contractBytes)` 加载契约，再传给 `new Coflow.RuntimeBuilder(contract)` 创建构建器。
`AddSource(text, sourceName: "可选诊断标签")` 提交 CFD 文本，`Build()` 获得只读运行时。
通过 `runtime.Table<Item>().Get("key")` 和 `runtime.Get<Settings>()` 读取数据。
释放契约、builder 与 Runtime；对象、集合、函数和 struct 包装无需单独 Dispose。
运行时显式释放后，依赖它的包装失效。Host 实现生成的强类型接口，通过 `builder.BindHost(host)` 绑定。

函数使用生成的 `CoflowFunction<..., TResult>.Invoke(...)` 强类型调用，`fstring` 字段在读取时求值。
Host 接口同时生成数据属性和强类型函数，`Runtime.RunChecks()` 返回结构化检查结果与统计。

原生构建及 IL2CPP 集成说明见仓库 `docs/2-架构/07-Unity原生集成.md`。
