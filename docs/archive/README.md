# 性能测量资料

本目录保存带日期、环境和代码基线的性能报告及原始数据。当前设计入口为 [文档索引](../README.md)。

| 资料 | 类型 | 当前对应文档 |
| --- | --- | --- |
| [非编辑器优化验证](benchmarks/non-editor-optimization-2026-10-29/README.md) | 基于 0.14.0 的 Rust、CLI 与 C# 实现、功能回归和成对实测 | [优化方案](../development/non-editor-performance-plan.md) |
| [0.14.0 非编辑器基线](benchmarks/non-editor-2026-10-28/README.md) | Rust 加载/修改/拒绝路径、独立 CLI、.NET 8 与 Unity Player；含未达项与未测边界 | [优化方案](../development/non-editor-performance-plan.md) |
| [审查性能复测](benchmarks/review-performance-2026-10-28.md) | 标准图、热编辑与明确未达项 | [性能验证](../development/performance-validation.md) |
| [核心增量与超大图节点验收](benchmarks/incremental-graph-acceptance-2026-10-26.md) | 不可变批次、字段依赖、完整端口几何、窗口化与生产基准；含未达性能项 | [增量代次契约](../runtime-design/incremental-generations.md) |
| [全路径审查微基准](benchmarks/full-performance-audit-2026-10-26.json) | 选区集合索引与不可变值身份比较的局部算法对照 | [性能验证](../development/performance-validation.md) |
| [表格尺寸预算优化验收](benchmarks/table-size-budget-2026-10-26.md) | 可见高度预算、主动适配、列区间共享与滚动复测 | [性能约束](../editor-design/performance.md) |
| [复杂表格优化验收](benchmarks/complex-table-optimized-2026-10-26.md) | 集中测量、复杂列窗口、共享描述与生产构建复测 | [复杂表格设计](../editor-design/table-projection.md) |
| [复杂表格滚动专项采样](benchmarks/complex-table-performance-2026-10-26.md) | 复杂列数量、集合长度、摘要测量诊断对照与 CPU 热点 | [性能约束](../editor-design/performance.md) |
| [编辑热路径与界面帧间隔](benchmarks/engine-editing-2026-10-26.md) | 批次输入、行对象复用、虚拟列表缓存与生产构建前后对照 | [增量代次契约](../runtime-design/incremental-generations.md) |
| [局部更新、检查与加载测量](benchmarks/engine-performance-incremental-2026-10-26.md) | 分页共享、重复刷新、批量更新与完整原始对照 | [增量代次契约](../runtime-design/incremental-generations.md) |
| [编辑器宽表与大工程性能测量](benchmarks/editor-performance-2026-10-25.md) | 生产构建、功能回归与后端大工程测量 | [性能约束](../editor-design/performance.md) |
| [引擎核心性能基线](benchmarks/engine-performance-2026-10-25.md) | 模型、check、真实更新与十万条容量测量 | [增量代次契约](../runtime-design/incremental-generations.md) |
| [引擎核心优化验收](benchmarks/engine-performance-optimized-2026-10-25.md) | 十万条连续编辑、批量更新、增量等价与底层微基准 | [增量代次契约](../runtime-design/incremental-generations.md) |
| [集中引用拓扑测量](benchmarks/engine-performance-topology-2026-10-25.json) | 环形与集中引用、模型入边更新、检查共同读者的原始样本 | [性能验证](../development/performance-validation.md) |

测量结果对应报告中的代码版本、机器与夹具；当前设计以维护文档为准。现行开发与发布检查统一见 [AGENTS.md](../../AGENTS.md)。
