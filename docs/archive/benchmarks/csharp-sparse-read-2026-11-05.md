# C# 稀疏视图读取优化测量（2026-11-05）

本次对照使用优化前后的稀疏运行时，基础值、覆盖值和上下文语义相同。视图创建时绑定实际命中的覆盖，固定值通过泛型直接读取，动态结果缓存命中时省去重复来源定位。没有覆盖命中的视图不分配覆盖索引，也不复制完整字段数组。

## 环境与方法

Windows x64，Intel i5-13600KF，.NET SDK 8.0.427，net8.0 / .NET 8.0.33，Release，`DOTNET_TieredCompilation=0`。两组串行运行，各预热 2 次、采样 7 次；测量时未并行运行构建或测试，其他系统后台负载未控制。每次读取计时包含 100 万次操作。报告中数值为中位数，范围及源码 SHA-256 见 [元数据](csharp-sparse-read-2026-11-05-metadata.json)。

## 宽表结果

500 条记录、128 个整数字段、12 个稀疏变体，使用 [宽表探针](csharp-sparse-2026-11-05-probe.cs)。

| 指标 | 优化前 | 优化后 |
| --- | ---: | ---: |
| 加载耗时 | 18.12 ms | 17.67 ms |
| 创建视图耗时 | 8.18 ms | 8.34 ms |
| 加载分配量 | 24,858,008 B | 24,866,008 B |
| 保留内存 | 8,155,224 B | 8,262,392 B |
| 100 万次标量读取 | 11.96 ms | 2.24 ms |

标量读取耗时下降约 81%。本夹具保留内存增加 107,168 B（约 1.3%），来自读取器引用及命中覆盖索引；仍保持基础字段共享。保留内存为探针强制 GC 后的差值，不是峰值内存。

## 读取矩阵

500 条记录，两个维度同时选择。none 只有空变体声明，sparse 仅第一条记录有覆盖，dense 每条记录均覆盖整数和字符串。模板和列表在计时前完成访问；下表衡量热读取。所有样本热读取循环分配量均为 0 B。

| 覆盖密度 | 字段 | 优化前 ms | 优化后 ms |
| --- | --- | ---: | ---: |
| none | int | 11.190 | 2.153 |
| none | string | 14.457 | 2.340 |
| none | template | 16.527 | 14.301 |
| none | list | 12.355 | 3.999 |
| sparse | int | 11.353 | 2.320 |
| sparse | string | 14.710 | 2.336 |
| sparse | template | 16.642 | 14.419 |
| sparse | list | 12.283 | 4.585 |
| dense | int | 21.950 | 5.418 |
| dense | string | 23.958 | 5.205 |
| dense | template | 16.489 | 15.982 |
| dense | list | 12.123 | 6.540 |

模板缓存仍通过会话锁保护；本优化主要减少固定字段读取开销。结果属于合成微基准，不代表端到端应用或 Unity 性能，也不意味着共享读取器与直接 CLR 字段访问成本相同。

## 复现资料

- [优化前宽表样本](csharp-sparse-read-2026-11-05-before.ndjson)、[优化后宽表样本](csharp-sparse-read-2026-11-05-after.ndjson)。
- [优化前矩阵样本](csharp-sparse-read-2026-11-05-before-matrix.ndjson)、[优化后矩阵样本](csharp-sparse-read-2026-11-05-after-matrix.ndjson)、[矩阵探针](csharp-sparse-read-2026-11-05-matrix.cs)。
- [两份运行时源码与探针项目](csharp-sparse-read-2026-11-05-sources.zip)：解压后，将 `after/probe` 和 `after/matrix` 复制至 `before` 下，分别以 Release 执行两个目录下的项目。项目均通过相对路径引用同级 `Coflow.Loader`；运行前设置环境变量 `DOTNET_TieredCompilation=0`。
