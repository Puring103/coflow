# Unity 中使用 C# Runtime

`Coflow.Runtime` 和生成的 C# binding 支持 Unity 2021.3 LTS。生成代码的语言基线为
C# 9，不会使用文件作用域命名空间等 C# 10 及以上语法。

`Coflow.Runtime` 同时面向 `netstandard2.1` 和 `net8.0`。Unity 项目应引用
`netstandard2.1` 版本以及由 Coflow 生成的 C# binding。

## Mono 与 IL2CPP

- Unity Mono 后端支持动态代码时，Runtime 会将内部表达式编译为 delegate，使用正常的
  JIT 快速路径。
- 生成 binding 使用 `CoflowFunction<...>` 表示全部函数值；Host 调用与 VM 闭包跨边界使用固定的
  两 ID 布局，不需要生成 delegate adapter。
- 生成 Schema 会注册声明和签名中的 closed value type 布局，Module 编译器补全函数体使用的组合布局，
  并直接提供强类型字段 binding；执行阶段不再推导布局。
- IL2CPP 等不支持动态代码的环境会对动态插件式 `Delegate` 和内部结构 codec 自动使用
  表达式解释器 fallback。Runtime 不依赖 `DynamicMethod`、`Reflection.Emit` 或运行时生成程序集。
- 两条路径具有相同的调用接口；业务代码和生成的 binding 不需要根据后端切换 API。

Runtime 的冷路径仍会建立泛型 codec 和内部 delegate。因此，IL2CPP 构建必须保留 Runtime 程序集，
以及包含 Coflow 生成 binding 的程序集。

## Unity linker

NuGet 包包含 `Coflow.Runtime/link.xml`，用于保留整个 `Coflow.Runtime` 程序集。
确认包管理工具最终将该文件复制到 Unity 项目的 `Assets` 目录下；部分 NuGet-to-Unity
工具不会自动复制 `contentFiles`，这种情况下需手动将包中的 `link.xml` 放入 `Assets`。

生成 binding 所在程序集也应添加到项目自己的 `link.xml`。将下面的程序集名替换为实际
名称：

```xml
<linker>
  <assembly fullname="Coflow.Runtime" preserve="all" />
  <assembly fullname="Game.Generated.Coflow" preserve="all" />
</linker>
```

保留整个生成程序集是兼容性优先的配置。确认所有目标平台的 IL2CPP 构建和功能测试通过后，
可以根据项目的生成类型逐步收窄规则。

## 发布前验证

仓库中的 .NET 测试会强制执行无动态代码路径，但不能替代 Unity 构建。每个目标平台至少应
在实际 IL2CPP Player 中验证一次：加载 CFD、读取生成字段、直接调用函数、传入和返回
`CoflowFunction<...>`，以及包含 `option`、`result`、array 和 map 的调用。
