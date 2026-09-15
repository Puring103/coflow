# Coflow Runtime for Unity

支持 Unity 2022+ 的 .NET Standard 2.1 API 级别和 IL2CPP。通过 Unity Package Manager
的本地包入口选择此目录中的 `package.json`，把对应目标平台的 `coflow_ffi` 原生库放入
Unity 工程的 `Assets/Plugins`，并在 Plugin Inspector 中设置平台和 CPU。

生成代码提供 `CoflowSchema.Load()`，用于读取随代码生成的契约。创建构建器后用
`AddSource(logicalPath, source)` 提交 CFD 文本，调用 `Build()` 获得只读运行时。
对象、集合与函数包装使用 `Dispose()` 释放句柄；运行时显式释放后，依赖它的包装失效。

函数体和 fstring 模板保留源码。当前版本的函数编译、函数调用、模板求值和 check 执行
返回尚未实现错误；字段、记录、集合和模板源码仍可访问。

原生构建及 IL2CPP 集成说明见仓库 `docs/2-架构/07-Unity原生集成.md`。
