# 历史资料

本目录保存旧方案、界面草图和历史测量。当前设计入口为 [文档索引](../README.md)。

| 资料 | 类型 | 当前对应文档 |
| --- | --- | --- |
| [核心增量与超大图节点验收](benchmarks/incremental-graph-acceptance-2026-10-26.md) | 不可变批次、字段依赖、完整端口几何、窗口化与生产基准；含未达性能项 | [增量与图优化计划](../development/incremental-graph-optimization-plan.zh-CN.md) |
| [全路径审查微基准](benchmarks/full-performance-audit-2026-10-26.json) | 选区集合索引与不可变值身份比较的局部算法对照 | [核心与前端性能审查](../development/full-performance-audit.zh-CN.md) |
| [表格尺寸预算优化验收](benchmarks/table-size-budget-2026-10-26.md) | 可见高度预算、主动适配、列区间共享与滚动复测 | [性能约束](../editor-design/performance.md) |
| [复杂表格优化验收](benchmarks/complex-table-optimized-2026-10-26.md) | 集中测量、复杂列窗口、共享描述与生产构建复测 | [复杂表格设计](../development/editor-performance-review.zh-CN.md) |
| [复杂表格滚动专项采样](benchmarks/complex-table-performance-2026-10-26.md) | 复杂列数量、集合长度、摘要测量诊断对照与 CPU 热点 | [性能约束](../editor-design/performance.md) |
| [编辑热路径与界面帧间隔](benchmarks/engine-editing-2026-10-26.md) | 批次输入、行对象复用、虚拟列表缓存与生产构建前后对照 | [引擎优化方案](../development/engine-performance-plan.zh-CN.md) |
| [局部更新、检查与加载测量](benchmarks/engine-performance-incremental-2026-10-26.md) | 分页共享、重复刷新、批量更新与完整原始对照 | [引擎优化方案](../development/engine-performance-plan.zh-CN.md) |
| [数据加载器重构](plans/data-loader-redesign.zh-CN.md) | 历史实施方案 | [C# 生成与加载](../language-design/02-api-runtime-design.zh-CN.md) |
| [编辑器值创建](plans/editor-value-creation-design.zh-CN.md) | 历史设计与实施计划 | [值创建](../editor-design/value-creation.md) |
| [编辑器布局草图](previews/editor-layout-draft.html)、[版本一](previews/editor-layout-draft-v1.html) | 历史静态预览 | [视图与工作区](../editor-design/views.md) |
| [编辑器宽表与大工程性能测量](benchmarks/editor-performance-2026-10-25.md) | 生产构建、功能回归与后端大工程测量 | [性能约束](../editor-design/performance.md) |
| [引擎核心性能基线](benchmarks/engine-performance-2026-10-25.md) | 模型、check、真实更新与十万条容量测量 | [引擎优化方案](../development/engine-performance-plan.zh-CN.md) |
| [引擎核心优化验收](benchmarks/engine-performance-optimized-2026-10-25.md) | 十万条连续编辑、批量更新、增量等价与底层微基准 | [引擎优化方案](../development/engine-performance-plan.zh-CN.md) |
| [集中引用拓扑测量](benchmarks/engine-performance-topology-2026-10-25.json) | 环形与集中引用、模型入边更新、检查共同读者的原始样本 | [数据结构与算法复审](../development/engine-performance-review.zh-CN.md) |
| [静态检查依赖性能测量](benchmarks/check-static-dependency-2026-07-27.md) | 指定版本与机器的测量结果 | [系统架构](../architecture/overview.md) |

归档资料保留原始设计和测量语境，其语法、命令、接口及验收状态以对应历史版本为背景。现行开发与发布检查统一见 [AGENTS.md](../../AGENTS.md)。
