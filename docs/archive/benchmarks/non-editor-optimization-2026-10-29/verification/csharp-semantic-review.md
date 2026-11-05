# C# R6 语义验证与证据边界

R6 的 .NET 8 Release 测试共 **86 项通过、0 失败、0 跳过**，数量包含参数化用例。netstandard2.1 烟测与生成代码／共享运行时语料集成通过。Unity Mono 的六类夹具及主线程、默认新线程共 **30 项契约检查通过**。这些验证支持下列已覆盖能力保持，性能目标另按正式样本验收。

产物、源码 SHA256、宿主设置及逐夹具结果见 [R6 构建与烟测收据](csharp-r6-build-smoke.json)。原始证据为 [.NET 测试日志](csharp-r6-tests.log)、[netstandard2.1 日志](csharp-r6-netstandard-smoke.log)、[生成集成日志](csharp-r6-integration.log)和 [Mono 烟测汇总](csharp-r6-unity-smoke.log)；收据保留各 Player 原始 NDJSON 与进程回执路径。烟测共产生 **0 条正式性能 measurement**。

## .NET 关键覆盖

以下方法位于 [LoadingTests.cs](../../../../../runtimes/csharp/Coflow.Loader.Tests/LoadingTests.cs) 与 [PerformanceContractTests.cs](../../../../../runtimes/csharp/Coflow.Loader.Tests/PerformanceContractTests.cs)，均包含在上述 86 项结果中。

| 能力 | 关键测试名 | 实际断言 |
| --- | --- | --- |
| 加载时完整验证已声明单变体 | `VariantOnlyInterpolationCyclesAreReportedAtLoadTime`；`VariantOnlyLongReferenceChainsStillEnforceTheConstructionBudget`；`ValidationOnlyViewsAreCollectibleAndRequestedViewsKeepReferenceIdentity` | 仅变体中的环和 300 层引用在 Load 时拒绝；两个已声明变体均生成校验对象，临时对象可回收，业务首次 For 仍正确 |
| 默认值、嵌套值与引用身份 | `ConstructsGeneratedTypesAndLinksCyclicReferences`；`IndependentViewsCarryContextsThroughTemplatesCollectionsAndReferences`；`AnonymousObjectsInsideCollectionsReevaluateTheirDefaultTemplates`；`ScheduledReferenceDependenciesRespectEveryOptionAndConstantLayer` | 默认值、Option 层次、嵌套模板、数组／字典、循环引用、视图上下文传播及逐包装常量解析保持正确 |
| 根包装回收后的首次视图 | `LoadedDataReleasesRootSyntaxWhilePreservingLaterDimensionMaterialization` | 完整 GC 后根 Record／Field／Fields 的弱引用失效，首次组合 For、默认嵌套模板、回退及环引用仍可用；组合顺序不改变身份 |
| DFS 诊断与深度边界 | `DeepReferenceErrorsKeepDepthFirstDiagnosticOrder`；`Exactly256ReferenceLevelsPreserveCyclicIdentityOnTheDefaultThread`；`DeepTemplatesRetainTheExactConstructionLimitOnTheDefaultThread`；`DepthLimitPrecedesTheMissingTargetOfThe257thReference` | 深层引用错误先于祖先后续字段错误；256 层合法，257 层返回 CFD-LIMIT；第 257 层的缺失目标不抢先成为首错 |
| 显式记录栈与复杂读取混合 | `ExplicitReferencesAndSuspendedTemplatesShareOneLogicalDepthBudget`；`CollectionSuspensionPreservesExplicitParentFramesAndCompletedObjects`；`ATemplateReadingAPendingAncestorFieldKeepsTheCycleDiagnostic`；`SuspendedCollectionsConstructEveryCompletedInlineObjectOnlyOnce` | 40 层普通引用接模板时共用 256／257 边界；引用接数组／字典与内联对象后继续恢复，四个内联对象只构造四次；回读待完成父字段保留循环诊断 |
| Table 查询接口 | `DirectTableQueriesPreserveExceptionsReadOnlyViewsAndIdentity`；`TableBucketsPreserveClrAssignableMembershipAndSharedObjectIdentity` | 缺失键、null 键异常及 TryGetValue 结果保持；Keys／Values 拒绝修改；枚举、父子表与索引器返回原记录身份 |
| 失败组合的缓存清理 | `FailedCombinedViewsNeverExposePartiallyBuiltCircularNeighbors` | 从两个环形邻居分别请求失败组合都抛出 CfdLoadException，后一次访问不暴露前一次留下的半构造邻居；基础对象与引用仍正确 |
| 缓存、隔离与来源 | `ConcurrentCachedAndNewSelectionOrdersKeepOneViewAndCyclicIdentity`；`CachedSelectionsAndDimensionMetadataDoNotAllocate`；`IndependentlyLoadedContextsRetainTheirOwnSelectionsAndSources`；`NestedOverlayFailureRetainsTheOverlaySourceAndSpan` | 并发组合身份、预热查询分配、独立加载上下文和覆盖诊断来源得到检查；零分配断言仅为本次 .NET 计数证据 |

这组测试覆盖指定语义与退化边界，不是对所有输入、所有并发时序或全部宿主配置的穷尽证明。C# Loader 不执行 Rust 的 CFT 业务 check；此处也不代替 Rust／CLI 的功能门禁。

## Mono 实际覆盖

使用 Unity **2022.3.62f3、Windows x64、Mono、netstandard2.1、非 Development、关闭增量 GC、关闭托管裁剪**的实际 Player。scalar8、scalar20、scalar100、tables32、complex、dimensions 各使用 100 条记录、2 文件；复杂值和维度夹具包含模板与最长 64 条的独立引用环，维度夹具每维 2 个变体。预检检查全部基础字段及集合、模板、引用，派生表身份，以及代表记录上的各变体、选择顺序和未知变体回退。

[ConstructionContracts.cs](../../../../../scripts/performance/unity/ConstructionContracts.cs) 在 Unity 主线程与未指定栈大小的 `new Thread` 上各执行 15 项：256 层引用环、257 层引用拒绝、仅变体的 300 层拒绝、256／257 层模板、96 层模板环、数组／字典恢复、DFS 首错、普通引用接模板的 256／257 边界、普通引用接数组／字典恢复、待完成父字段循环、257 层优先于缺失目标。30 项均通过。

Mono 没有运行整套 xUnit 测试；.NET 的弱引用回收、Table 异常和分配断言不能自动算作 Mono 同项实测。Mono 的逐操作分配计数不可用，相关值保持 null。真实游戏帧、长期组合缓存增长、多线程争用和其他 GC／裁剪设置未由本次烟测覆盖。

## 性能与未覆盖宿主

R6 单轮校准中，Mono dim1 sparse 10k 两项解析拒绝的原始记录见[基线](../calibration-r6/mono-dim1-sparse-10k-r1-baseline.stdout.ndjson)与[优化版](../calibration-r6/mono-dim1-sparse-10k-r1-optimized.stdout.ndjson)。每项每进程只有 1 个计时样本：

| 操作 | 基线 → R6 耗时 | 操作区间 GC 次数 | 完整 GC 后托管存活量 |
| --- | --- | --- | --- |
| `reject_unknown_type_at_end` | 320.974 → 361.969 ms（+12.77%） | 0 → 1 | 719.09 → 298.21 MB |
| `reject_syntax_at_end` | 317.146 → 357.252 ms（+12.65%） | 0 → 1 | 715.30 → 285.49 MB |

两项的托管堆保留容量均为 912.40 → 409.08 MB。GC 计数在操作后、样本后的强制回收前读取，额外 GC 确实发生在测量区间；没有采集 GC 耗时，不能将约 40 ms 的差额全部归因于它。存活量与保留容量包含宿主、夹具和历史加载对象，不能解释为错误 AST 的独占大小。该关联不消除已观测回退，也不替代三轮正式验证；完整结果、目标状态与原始样本来源统一以 [final-results 比较](../final-results/comparison.md)及 `final-results/target-status.json` 为准。

**IL2CPP 无构建及执行证据。** 已授权安装所需模块的官方组件下载返回 HTTP 404，见[安装失败证据](../../non-editor-2026-10-28/unity-il2cpp-install.txt)。.NET、netstandard2.1 和 Mono 的通过不能代替 IL2CPP 的 AOT、裁剪、默认线程与性能验收。
