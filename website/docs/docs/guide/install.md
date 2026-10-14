# 安装与快速开始

```powershell
cargo install --git https://github.com/Puring103/coflow.git coflow
coflow --help
```

```powershell
coflow check --rules --project examples/showcase
coflow codegen --project examples/showcase
```

项目至少包含一个 `.cft` schema、一个 `.cfd` 数据路径和一个 `codegen` target。生成 C# 后，将 `runtimes/csharp/Coflow.Loader` 和生成文件引入目标项目，通过 `CoflowData.Load` 提供所选 CFD 文件的逻辑路径与文本，检查加载结果后读取业务数据。详见 [C# 接入](../reference/07-codegen/01-csharp.md)。
