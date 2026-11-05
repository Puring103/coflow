# C# 完整预构建数据测量（2026-11-06）

版本 0.16.0 的合并前快照 `830e4dcddcce051339923d08d2007ae6290bf059`，具体源码由 metadata 中的哈希绑定。这组数值不代表合并远端性能改动后的最终发布构建。加载阶段完成全部请求上下文，普通字段直接读取最终数据，Dimension 查询已完成的对象。运行环境为 Windows x64、Intel i5-13600KF、.NET SDK 8.0.427、.NET 8.0.33、Release，关闭分层编译。每项预热 2 次、正式采样 7 次，串行执行；测量期间没有并发构建或测试，其他系统后台负载未控制。

## 已完成数据读取

500 条记录，基础与日语单维上下文，仅第一条记录有名称覆盖；模板字段、模板列表元素和字典值均在加载时生成。每项每次 100 万次读取，计时之外校验全部结果与校验和。

| 操作 | 中位数 ms | 最小 ms | 最大 ms | 热循环分配 |
| --- | ---: | ---: | ---: | ---: |
| scalar | 0.4087 | 0.4073 | 0.4308 | 0 B |
| template | 0.6859 | 0.6818 | 0.6915 | 0 B |
| list-template | 4.2580 | 4.1561 | 4.2961 | 0 B |
| dictionary-template | 10.5746 | 10.3966 | 10.6805 | 0 B |
| for | 43.4335 | 42.8633 | 43.9589 | 0 B |

For 包含目录查询及取得字段值，业务字段读取无需再次查询目录。列表与字典保留标准只读集合的访问成本。结果是本机合成微基准，不代表 Unity 或端到端应用性能。

## 加载与选择

[加载原始样本](csharp-prebuilt-2026-11-06-load.ndjson) 包含 48 项解析、宽表、稀疏/密集源覆盖、现实引用与模板、选择及多表注册场景。这里的 construction/full-load 包含所有已声明单维上下文的完整构建；first-for/cached-for 均仅查询已完成结果。分配量是操作期间当前线程累计分配，不是保留内存。没有用历史稀疏视图样本作同轮性能对照，也不据此宣称加载耗时或内存优于历史模型。

## 复现与证据

- [读取探针](csharp-prebuilt-2026-11-06-probe.cs)：使用 net8.0 控制台项目，开启 ImplicitUsings，引用 0.16.0 的 Coflow.Loader 项目，以 Release 构建并执行。
- [读取原始样本](csharp-prebuilt-2026-11-06-reads.ndjson)、[环境、源码哈希与全部统计](csharp-prebuilt-2026-11-06-metadata.json)。
- 加载基准：设置 `DOTNET_TieredCompilation=0` 后执行 `dotnet run --project runtimes/csharp/Coflow.Loader.Benchmarks/Coflow.Loader.Benchmarks.csproj --configuration Release -- --suite all --records 500 --samples 7 --warmups 2`。
- 功能测试另外验证构建器可回收、最终对象图没有读取器/会话/委托、组合在加载时失败和并发读取身份一致。性能采样不替代功能验证。
