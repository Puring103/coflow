# C# VM 性能基准

该项目使用 BenchmarkDotNet 在 Release 模式下测量 C# runtime，包括函数调用、集合操作、
Host 调用、配置加载和 ModuleSet 替换。

在仓库根目录运行。脚本会先从复杂示例的 CFT 重新生成强类型 C#，再启动基准：

```bash
PATH=/home/wtl/.dotnet:$PATH \
DOTNET_ROOT=/home/wtl/.dotnet \
DOTNET_ROLL_FORWARD=LatestMajor \
runtimes/csharp/Coflow.Cfd.Runtime.Benchmarks/run.sh
```

只运行其中一组：

```bash
runtimes/csharp/Coflow.Cfd.Runtime.Benchmarks/run.sh \
  --filter '*VmExecutionBenchmarks*'
```

结果写入 `BenchmarkDotNet.Artifacts/results/`。`Mean`、`Median`、`Min` 和 `Max` 表示执行延迟，
`Allocated` 和各代 GC 列表示每次操作的内存分配。

比较提交或运行时版本时，应使用相同机器、相同 .NET runtime、相同电源策略，并保留完整的
BenchmarkDotNet 报告。不要用 Debug 构建或单元测试总耗时推断 VM 吞吐量。
