# Unity 数据加载与访问基准

本宿主使用 Unity 2022.3 LTS Windows x64 非 Development Player，分别支持 Mono 和 IL2CPP。输入、真实生成业务类型、逐字段断言来自 `runtimes/csharp/Coflow.Loader.Benchmarks/`；Loader 使用 Release `netstandard2.1` DLL。所有 Unity 工程、数据与构建产物位于独立目录。

## 准备与构建

PowerShell 7 在仓库根目录执行。预先通过 Unity Hub 安装指定版本及 Windows Build Support (IL2CPP)，并具备可用 Unity 许可。IL2CPP 还需 Visual Studio C++ 工具链与 Windows SDK。

```powershell
dotnet build runtimes/csharp/Coflow.Loader/Coflow.Loader.csproj -c Release -f netstandard2.1
$benchmarkProject = Join-Path $env:TEMP ('coflow-unity-' + [Guid]::NewGuid().ToString('N'))
& scripts/performance/unity/prepare.ps1 -ProjectPath $benchmarkProject
& scripts/performance/unity/build.ps1 -Unity 'C:/Program Files/Unity/Hub/Editor/2022.3.62f3/Editor/Unity.exe' -ProjectPath $benchmarkProject -OutputPath "$benchmarkProject-mono" -Backend Mono
& scripts/performance/unity/build.ps1 -Unity 'C:/Program Files/Unity/Hub/Editor/2022.3.62f3/Editor/Unity.exe' -ProjectPath $benchmarkProject -OutputPath "$benchmarkProject-il2cpp" -Backend IL2CPP
```

`prepare.ps1` 要求工程目录不存在，并记录 Loader、夹具和断言源码 SHA256。它将两份共享夹具源码转换为 C# 9 块命名空间并补齐显式 using，保持数据语义。六种夹具的真实生成代码原样复制。`build.ps1` 只接受带有专用标记的新建基准工程，输出目录必须不存在。构建设置采用空场景、禁用增量 GC、Mono 禁用托管裁剪或 IL2CPP Minimal 裁剪；IL2CPP 编译配置为 Release。完整设置保存在 `build-metadata.json`。

## 运行

构建与其他性能采样错开，所有玩家进程串行执行。先以 100 条、两个文件对六种夹具执行纯语义烟测，然后采集所需规模。

仅验证语义时使用 `-Operations validate`：运行完整夹具预检并输出成功标记，不产生 `measurement` 记录。`-Operations contracts` 另外在 Unity 主线程和未指定栈大小的默认新线程上验证 256 层引用与模板、超限拒绝、循环身份、集合暂停恢复和 DFS 诊断顺序。这两个操作组独立于正式采样路径。

```powershell
& scripts/performance/unity/run-player.ps1 -Player "$benchmarkProject-mono/CoflowBenchmark.exe" -Output "$benchmarkProject-results/mono-scalar20-1000.ndjson" -Case scalar20 -Records 1000 -Files 10 -Samples 5
& scripts/performance/unity/run-player.ps1 -Player "$benchmarkProject-il2cpp/CoflowBenchmark.exe" -Output "$benchmarkProject-results/il2cpp-dimensions-10000-v10.ndjson" -Case dimensions -Records 10000 -Files 100 -Samples 3 -Variants 10 -Density sparse -Topology hub -Templates -Operations load,dimensions,error
```

每个 NDJSON 对应一个独立 Player 进程；配套 `.player.log` 保存 Unity 日志，`.process.json` 保存参数、退出状态、墙钟及整个玩家进程的采样峰值。`--output` 对直接调用玩家也是必填参数。脚本只终止超时的自建基准玩家。

## 串行套件与续跑

`run-suite.ps1` 串行调用上述玩家脚本，覆盖与 .NET 矩阵相同的 16 组主要夹具：scalar20 千条/万条/十万条，万条 8/100 字段，32 表，复杂值万条/十万条，模板、高扇入、有界环，以及两维各 1/10 个变体的稀疏/密集万条和每维 1 个变体的稀疏十万条。每组都保留 .NET 对照场景 ID。一般加载使用 5 个样本，复杂值十万条和维度组使用 3 个样本；查询为 100 个样本、每批 10000 次调用。

```powershell
& scripts/performance/unity/run-suite.ps1 -Player "$benchmarkProject-mono/CoflowBenchmark.exe" -Backend Mono -OutputDirectory "$benchmarkProject-results" -List
& scripts/performance/unity/run-suite.ps1 -Player "$benchmarkProject-mono/CoflowBenchmark.exe" -Backend Mono -OutputDirectory "$benchmarkProject-results"
& scripts/performance/unity/run-suite.ps1 -Player "$benchmarkProject-mono/CoflowBenchmark.exe" -Backend Mono -OutputDirectory "$benchmarkProject-results" -Resume -Only unity-mono-scalar20-10k,unity-mono-dim1-sparse-10k
```

`-Only` 接受完整 ID 数组或逗号分隔的 ID，`-List` 只列出选中 ID，不启动玩家或写结果。IL2CPP 使用 `-Backend IL2CPP` 和相应 Player，场景前缀为 `unity-il2cpp-`。`-TimeoutSeconds` 默认限制每个完整玩家进程 1800 秒，包含引擎启动、夹具、预检、预热、全部操作及校验；它不是单操作时延。

套件保存 `unity-mono-plan.json` 或 `unity-il2cpp-plan.json`，包含完整参数、构建元数据、实际构建载荷 SHA256 和运行脚本 SHA256。每组输出 `<id>.stdout.ndjson`、`<id>.stdout.ndjson.player.log` 和 `<id>.stdout.ndjson.process.json`。同目录可保存两个后端的独立计划和结果。

`-Resume` 仅跳过计划和参数匹配、进程正常退出、语义预检通过且最后输出 `complete=true` 的结果。已有超时、失败、部分输出、参数不匹配或产物改变时直接报错，保留全部证据。重测使用新输出目录；在原目录继续其他未运行场景可用 `-Only` 选择其 ID。套件不会覆盖失败文件，也不会把失败结果视为已完成。

## 测量协议

操作组、夹具与 .NET 基准相同：`startup`、`load`、`io`、`reload`、`queries`、`dimensions`、`error`。维度组增加 `dimension_fallback_first_record` 和 `dimension_fallback_cached_batch`，分别测量未知变体首次回退及重复访问。两个维度各有 `Variants` 个变体，稀疏覆盖为 1%。引用环每 64 条独立成环。

`startup_load_text` 是本玩家首次 Loader 调用，包含 Mono 首次 JIT 或 IL2CPP 首次静态初始化；Unity 进程和引擎启动耗时只在外部墙钟中。其他指标各预热一次。正式预热前，独立加载结果验证全部基础字段、集合、模板、引用和派生表身份，并对所有声明变体的代表记录验证回退、组合顺序、维度引用。

加载与首次维度操作每样本前完整 GC；首次维度准备新加载结果的成本不计时。断言与内存采集位于计时之外。文件 I/O 包含读取、解码和加载，操作系统缓存可复用。重载时业务有意持有旧代，验证新旧对象身份隔离。查询计入统一适配器委托和类型转换开销。

NDJSON 保存完整 `samples_ms`、`samples_detail`、`operations_per_sample`、中位数与范围。少于 20 样本时 P95 为 null，少于 100 样本时 P99 为 null。批次分位数描述整个批次，归一化平均访问时间不能解释为单操作尾延迟。

内存指标采用 Unity 可用的实际口径：

- `allocated_current_thread_bytes`：当前线程在操作区间的分配；启动探测不支持时为 null。它不同于 .NET 8 全进程总分配。
- `collections`：`0..GC.MaxGeneration` 的操作区间集合次数；Unity Mono/IL2CPP 的收集器代数与 .NET 8 不同。
- `managed_live_bytes`、`retained_delta_bytes`：`GC.GetTotalMemory` 及相对操作前增量；重操作在结果存活且校验完成后完整 GC，查询不插入完整 GC。
- `unity_managed_used_bytes`、`unity_managed_reserved_bytes`：Unity Profiler 返回的托管堆已用及保留容量，包含基准宿主和夹具。
- `.process.json` 中的峰值：外部每 100ms 采样整个 Unity 玩家进程，包含引擎、夹具、正确性预检、预热、历史操作与验证。

容量建议覆盖 scalar20 的 1,000/10,000/100,000 条，dimensions 的 1/10 变体、稀疏与密集覆盖以及模板开关，再对照 complex 和各种引用拓扑。只有在对应玩家上完成的样本才能标注为 Mono 或 IL2CPP 实测；.NET 8 和编辑器测试无法替代实际玩家数据。
