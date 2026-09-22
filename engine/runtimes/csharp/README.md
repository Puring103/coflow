# C# Runtime

本目录集中维护 Coflow 的 C# 运行时及其验证工程：

- `src/Coflow.Runtime`：运行时源码、Unity package 元数据与 NuGet 项目。
- `tests/Coflow.Runtime.Tests`：运行时单元测试。
- `tests/integration`：代码生成、原生运行时和 Host 互调的端到端夹具。
- `tests/unity`：Unity Mono 与 IL2CPP smoke 脚本源码。
- `benchmarks/Coflow.Runtime.Benchmarks`：托管运行时基准。
- `smoke/Coflow.Runtime.NetStandardSmoke`：`netstandard2.1` 原生加载验证。

日常仓库检查以根目录 `AGENTS.md` 中的命令为准。
