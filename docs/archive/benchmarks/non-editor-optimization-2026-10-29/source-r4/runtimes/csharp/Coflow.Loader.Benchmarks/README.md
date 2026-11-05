# C# 数据加载与访问基准

本工具使用真实 `coflow codegen` 输出的业务类型，通过公共 `CoflowData.Load`、表查询和维度扩展访问数据。夹具保存在 `fixtures/`，生成源码随基准保存。工具只在独立临时目录创建数据，不修改用户工程。

## 构建与运行

在仓库根目录生成并构建，编译完成后再串行采样：

```powershell
& runtimes/csharp/Coflow.Loader.Benchmarks/generate.ps1 -Coflow ./target/release/coflow.exe
dotnet build runtimes/csharp/Coflow.Loader.Benchmarks/Coflow.Loader.Benchmarks.csproj -c Release
dotnet runtimes/csharp/Coflow.Loader.Benchmarks/bin/Release/net8.0/Coflow.Loader.Benchmarks.dll --case scalar20 --records 10000 --files 100 --samples 5 --operations load,io,reload,queries,error --output "$env:TEMP/csharp-scalar20-10000.ndjson"
dotnet runtimes/csharp/Coflow.Loader.Benchmarks/bin/Release/net8.0/Coflow.Loader.Benchmarks.dll --case dimensions --records 10000 --files 100 --variants 10 --density sparse --templates true --topology hub --samples 3 --operations load,dimensions,error --output "$env:TEMP/csharp-dimensions-10000.ndjson"
```

参数均使用 `--name value`。`--case` 为 `scalar8`、`scalar20`、`scalar100`、`tables32`、`complex` 或 `dimensions`；`tables32` 含 32 个派生表及共同父表，每条记录拥有 20 个字段。复杂场景含标量、数组、字典、内联对象、Option、引用及字符串。

`--topology none|pairs|hub|ring` 决定复杂场景的引用结构；`ring` 每 64 条构成一个环，以符合构造深度上限，不能解释为十万条单一长环。`--templates true|false` 控制实际插值表达式和等值字面字符串对照。模板包含 self、内联字段和跨记录读取。

`dimensions` 声明 language 和 balance 两个维度，每个维度有 `--variants` 个变体。`--density sparse` 覆盖每 100 条中的一条；`dense` 覆盖全部记录。实际已声明变体总数是参数的两倍。`--query-samples` 默认 100，`--query-batch` 默认 10,000。

## 操作与计时

| 操作组 | 指标 | 计时内容 |
| --- | --- | --- |
| startup | startup_load_text | 本进程首次加载，包含生成描述初始化与首次JIT；每进程一个样本 |
| load | load_text | 已有源字符串到完整加载结果 |
| io | read_files_and_load | 应用读取全部文件、文本解码及完整加载 |
| reload | reload_with_old_result_alive | 业务保留旧结果时重新加载；旧新记录身份隔离 |
| queries | table_lookup_batch、direct_property_batch、table_enumerate | ID 查询、属性读取、全表枚举；前两项批量计时 |
| dimensions | dimension_first_record、dimension_first_all | 独立加载结果的首次单记录及全表语言视图构造 |
| dimensions | dimension_cached_for_batch | 重复取得同一缓存视图，验证对象身份 |
| dimensions | dimension_combined_first_record | 首次取得两维组合，验证组合顺序与字段结果 |
| dimensions | dimension_fallback_first_record、dimension_fallback_cached_batch | 未知变体回退的首次选择与缓存选择，验证基础值及缓存身份 |
| dimensions | dimension_variants_batch、dimension_has_variant_batch | 变体列表与显式覆盖查询 |
| error | reject_unknown_type_at_end、reject_syntax_at_end、reject_missing_reference | 语义、语法与缺失引用错误拒绝；引用错误仅复杂场景 |
| error | load_after_invalid_input | 无效输入后重新加载健康来源，验证完整结果 |

C# 接口为只读加载器，不支持数据修改、文件发布或增量热重载；重新加载是独立上下文。数据文件读取由应用负责。`load_text` 不包含 CFT 编译、业务 check、文件 I/O 或 JIT 进程启动，不能直接与 Rust 完整工程会话耗时比较。

除 `startup_load_text` 外，每个指标先预热一次，再保存全部样本。启动指标每进程一个样本，需通过独立进程重复采集。夹具生成、文件准备、编译、完整结果断言、首次视图的加载准备均位于计时之外。加载的固定泛型转发和结果包装计入计时；查询包含测量适配器的类型转换与委托调用。每次健康结果逐记录验证全部字段、集合、模板与引用身份。

可选启动指标之后、正式预热之前，在独立加载结果上预验证全部基础记录、32 个派生表与父表的对象身份，以及每个声明变体的边界/稀疏覆盖/回退代表记录。维度预验证包含集合、内联值、模板、引用身份、组合顺序和未知变体回退；随后释放验证实例并完整 GC。正式样本中的断言只检查已返回值，不为逆序组合等验证新建额外视图缓存。

重操作在每次样本前完整 GC，目的是固定已释放结果的生命周期；此协议不代表应用自然 GC 下的连续吞吐。查询样本之间不强制 GC。文件系统缓存不清空，I/O 组是已创建夹具的缓存可复用读取，不代表机器冷启动。

输出为 NDJSON：每个 measurement 保存 `metric`、`samples_ms`、`samples`、`samples_detail` 和 `operations_per_sample`。P50 使用算术中位数，少于 20 样本不报告 P95，少于 100 样本不报告 P99。查询归一化需使用每样本操作数，批量分位数不能声称为单次访问尾延迟。

分配字节与 GC 次数只覆盖计时操作。重操作额外记录结果保持存活时完整 GC 后的堆及相对操作前增量；进程工作集与累计峰值包含夹具、验证、历史操作和运行时，不等于数据模型大小。非重操作的 RetainedDeltaBytes 是未强制 GC 的堆变化。`reload_with_old_result_alive` 有意同时保留两代。不同操作的峰值应按独立进程复测后比较。

## 推荐矩阵

先以 100 条、2 文件、1 个正式样本执行各场景作正确性烟测。正式采样依次测 scalar20 的 1,000/10,000/100,000 条以及单文件/百文件；在 10,000 条固定规模对照 scalar8/20/100、tables32、complex 的模板与引用拓扑。维度先对照 1/10 变体、稀疏/密集、模板关闭/开启，再选择 100,000 条作容量测量。

慢加载通常先采 3–5 次并报告中位数与范围，热查询至少 100 个批次；需要验收加载尾延迟时另采足够独立样本。运行环境为 .NET 8；Unity Mono/IL2CPP 需要在相应宿主另建并执行测量。
