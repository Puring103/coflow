# 生成代码与应用加载

运行 `coflow build --project .`，在声明、CFD 数据和全部业务规则通过后生成 C# 源码。也可以先用 `coflow check --rules --project .` 独立验证，再用 `coflow codegen --project .` 生成代码。

C# 应用引用生成源码和 `runtimes/csharp/Coflow.Loader`，通过 `CoflowData.Load` 提供所选 CFD 的逻辑路径与文本。检查 Success 和 Diagnostics 后读取 Data，使用记录表、`Option<T>` 和按需选择的记录视图 访问业务数据。

加载器完成默认值、跨文件引用、维度覆盖与字符串模板求值。业务规则在 CLI 或编辑器中执行，应用加载时检查数据的类型与引用等有效性。应用可以重新加载一组选定文件以更新数据。完整示例见 [C# 接入](../reference/07-codegen/01-csharp.md)。
