# C# VM 性能基准

该项目使用 BenchmarkDotNet 在 Release 模式下测量正式 C# runtime。执行组覆盖整数循环、
CFD 调用链、尾递归、生成类型字段读取、`map/filter/fold`、CFD 到 Host、VM 到 Host 再回到
VM，以及返回给宿主的 VM 闭包。生命周期组覆盖完整加载编译和原子 Reload 重新编译。

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

结果写入 `BenchmarkDotNet.Artifacts/results/`。`Mean`、`Median`、`Min` 和 `Max` 表示执行延迟；
`Allocated` 和各代 GC 列表示每次操作的托管内存成本。生命周期基准的 `Allocated` 是本次
加载或 Reload 产生的总分配量，并不表示旧 generation 被永久保留；generation 可回收性由
runtime 单元测试单独验证。

比较提交或运行时版本时，应使用相同机器、相同 .NET runtime、相同电源策略，并保留完整的
BenchmarkDotNet 报告。不要用 Debug 构建或单元测试总耗时推断 VM 吞吐量。
