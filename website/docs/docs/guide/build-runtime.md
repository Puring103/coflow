# 生成代码与应用加载

先运行 `coflow check --project .` 验证声明与 CFD，再运行 `coflow codegen --project .` 生成目标源码。构建失败时保留既有生成目录。

C# 应用引用生成源码和 `runtimes/csharp/Coflow.Loader`，通过 `CoflowData.Load` 提供所选 CFD 的逻辑路径与文本。检查 Success 和 Diagnostics 后读取 Data，使用记录表、`Option<T>` 和 `DimensionValue<T>` 访问业务数据。

加载器完成默认值、跨文件引用、维度覆盖与 fstring 求值。应用可以重新加载一组选定文件以更新数据。完整示例见 [C# 接入](../reference/07-codegen/01-csharp.md)。
