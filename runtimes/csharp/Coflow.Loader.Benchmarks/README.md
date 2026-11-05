# C# Loader 性能观测

该工具使用共享 CFD 夹具和只读业务视图，输出各场景的 NDJSON 样本。字段访问采用 `reader.Get<T>(string)`。工具使用当前 Loader 构造协议，不属于发布门禁；历史对照应使用对应版本的工具。

## 运行

原位置参数继续有效。默认 500 条记录、1 次预热、7 次正式样本；旧命令的样本参数仍按指定数量运行，输出的 `meetsSevenSampleMinimum` 表示是否达到 7 次。

```powershell
dotnet run --project runtimes/csharp/Coflow.Loader.Benchmarks --configuration Release -- 500 7
dotnet run --project runtimes/csharp/Coflow.Loader.Benchmarks --configuration Release -- --suite all --records 500 --samples 7 --warmups 2
dotnet run --project runtimes/csharp/Coflow.Loader.Benchmarks --configuration Release -- --suite for --records 500 --samples 7 --for-iterations 100
```

`--suite` 接受逗号分隔的名称。`--variant-counts` 默认 `3,12`，用于 `variants` 和 `realistic`。`--smoke` 把记录数限制为 8，使用 1 次预热、1 个样本和 3 次缓存循环，只用于检查输入、输出和断言。

`count-descriptors` 或 `--count-descriptors` 在宽表场景中启用来源集合访问计数。计数运行与时间采样分开；正式采样使用生成代码实际采用的描述符数组。

## 场景与输入

| suite | 输入 | 阶段 |
| --- | --- | --- |
| `wide`（默认） | 原始宽表：32/128/512 个 int 字段，0/3 个变体；每个变体只覆盖 `r0.f0` | construction、full-load |
| `parser` | 32/128/512 字段无变体宽表，以及 3 个变体的稀疏/密集现实夹具 | parser-only |
| `variants` | 128 字段宽表；稀疏时只覆盖 `r0.f0`，密集时每条记录的每个字段均有覆盖 | construction、full-load |
| `realistic` | 基础输入和稀疏/密集变体；9 个业务字段、嵌套 data、Option、列表和字典 | construction、full-load |
| `for` | 现实夹具，3 个变体，稀疏/密集覆盖 | first-for、cached-for |
| `tables` | 总记录数固定，均匀分配到 1/16/64 个不同 CLR 业务类型，每条记录 1 个 int 字段 | construction、full-load、table-registration |

现实夹具包含中文名称、转义字符串、跨文件循环引用、跨记录插值、内联对象插值和列表中的插值。记录按相邻两条配对引用，最后一条奇数记录引用自身，引用深度不随总记录数增长。基础记录分到两个文件，维度文件置于基础文件之前。稀疏变体只覆盖 `r0.name`，密集变体覆盖全部记录的 `name`；未覆盖的模板仍需在所选上下文重新求值。

多类型夹具在准备阶段通过反射创建封闭泛型类型和描述符，计时中使用类型明确的回调。没有记录的类型仍参与表注册。

## 计时与校验

- `parser-only`：从已有文本解析所有文件，结果为语法文档；不包括绑定、业务对象或表注册。
- `construction`：从已解析文档开始，包括输入发现、绑定、基础构造、已声明单维度变体完整构建和表注册。
- `full-load`：包含完整公共加载入口的解析、构造、校验和注册。
- `table-registration`：提前完成 `LoadContext.Build()`，只计量 `CfdData` 的类型表注册。
- `first-for`：每个样本先加载新数据，计时内对每条记录调用一次 `For("language", "v0")`。For 只查询加载时已完成的对象；计时不包含业务构建或求值。
- `cached-for`：提前加载完整数据并对全部记录查询目标上下文，计时内重复访问。`operationsPerSample = recordCount × forIterations`，耗时和分配均为这些操作的总量。

文本生成、schema 描述符创建、准备阶段、输出数组、值断言和 JSON 序列化均在计时外。解析阶段校验文档、记录、字段及覆盖数量；业务加载校验全部基础值，现实夹具额外校验引用身份、插值与嵌套值；视图阶段校验覆盖、基础对象隔离及重复读取身份。

每次采样在准备状态保持存活时请求 GC，然后使用 `Stopwatch.GetTimestamp()` 和 `GC.GetAllocatedBytesForCurrentThread()` 记录操作。`allocatedBytes` 是当前线程累计分配量，不是峰值内存或常驻内存。强制 GC 统一采样起点，不代表实际游戏或应用的 GC 时序。每次样本的状态和结果在独立方法返回后释放引用，避免上一样本延长存活期。

正式比较至少使用 7 个样本，以原始样本报告中位数和范围；不要把少量样本解释为稳定 P95。固定硬件、运行时和编译选项，记录后台负载，串行运行对照；输入 `sourceSha256`、参数、操作数和工具源码应匹配。

## 版本对照

`LoaderSourceRoot` 可指定采用同一描述符协议的 Loader 源码副本。跨构造协议比较使用各自版本的业务适配，保持 CFD 输入、记录数、覆盖数量和操作数相同，并注明字段读取与视图物化时机差异。

稀疏视图与完整物化视图的对照程序、原始样本及限制见 [稀疏视图测量](../../../docs/archive/benchmarks/csharp-sparse-2026-11-05.md)。

覆盖预绑定与固定值读取优化的配对样本、读取矩阵和源码快照见 [稀疏读取优化测量](../../../docs/archive/benchmarks/csharp-sparse-read-2026-11-05.md)。

当前运行时为完整预构建模型，历史稀疏视图报告只描述其标注的历史代码快照。当前 first-for 与 cached-for 均为已完成结果的查询。

0.16.0 的当前测量见 [完整预构建数据测量](../../../docs/archive/benchmarks/csharp-prebuilt-2026-11-06.md)。
