# Check 静态依赖与无状态执行重构计划

日期：2026-07-24

## 1. 决策摘要

本次重构一次性替换现有 checker 的动态依赖、snapshot、round 和 target selection 体系，不维护旧新双轨，不保留内部兼容层，不通过 feature flag 延后删除。

最终系统只有三段：

```text
coflow-cft
  编译 check，并建立 Field/RecordSet -> root statement 的静态反向索引

coflow-runtime
  将 mutation 归一化为字段/集合变化，查询静态索引并生成 CheckTask

coflow-checker
  无状态执行明确指定的 CheckTask，返回本次任务产生的诊断和统计
```

核心原则：

1. checker 不拥有增量状态、依赖状态、诊断集合或 host generation。
2. “增量 check”不是 checker 的执行模式，只是 runtime 生成了比完整检查更少的任务。
3. 依赖在 CFT 编译期按所有可能执行分支的并集静态确定；运行期不记录实际读取。
4. 依赖精度止于顶层字段与 record set，不追踪数组元素、字典 key 或任意 `CfdPath`。
5. 可独立执行的最小节点是 check block 的根 statement；`when`、`all`、`any`、`none` 的内部 statement 不独立调度。
6. 直接宿主字段变化只检查变化记录；跨类型字段依赖保守检查 statement owner 对应的全部顶层记录。
7. 项目级 check 使用相同 statement/index/task 模型，target 为 project，而不是虚构记录。
8. 维度是字段变化和执行任务上的可选投影，不存在 dimension round。
9. 诊断聚合、旧诊断替换和 editor generation 由 runtime/host 的诊断子系统负责，不进入 `coflow-checker`。
10. 一次性迁移 workspace 内全部调用方；本分支完成前不合并任何只实现半条路径的提交。

## 2. 非目标

- 不根据实际引用 key 构建记录级依赖图。
- 不根据运行时短路、条件或量词访问结果提高失效精度。
- 不追踪数组 index、dict key 或嵌套 path 的增量依赖。
- 不在 checker 中保存或合并诊断集合。
- 不为 schema 热替换复用旧执行结果；schema 重编译后由 runtime 重新规划完整检查。
- 不在本次重构中增加并行 evaluator；并行调度属于后续独立性能工作。
- 不为未来函数、lambda、反射或动态类型查询预留 trait、占位 enum 或 feature flag。

## 3. 最终所有权

| 能力 | 唯一所有者 | 禁止事项 |
| --- | --- | --- |
| check 语法、类型和 lowering | `coflow-cft` | checker 不重新推导表达式类型 |
| 根 statement 身份 | `coflow-cft` | runtime 不使用 AST 地址或 span 拼装 ID |
| 字段/record-set 静态依赖 | `coflow-cft` | evaluator 不记录实际读取 |
| mutation 字段影响 | `coflow-runtime` | checker 不理解 writer、spread 或 materialization |
| full/incremental 任务规划 | `coflow-runtime` | checker 不接收 `All/Records/Incremental` 模式 |
| check 任务执行 | `coflow-checker` | checker 不选择目标或维度 |
| 维度值投影 | `coflow-checker` | projection 不枚举 variant 或任务 |
| 诊断聚合与 generation | `coflow-runtime`/host | `coflow-checker` 不提供 snapshot/delta store |
| CLI/LSP/editor 展示 | 各 host | host 不反解析 checker message |

## 4. CFT 编译产物

### 4.1 根 statement ID

每个 type-local check block 和项目级 check block 的根 statement 获得 schema 内稳定 ID：

```rust
pub struct CheckStatementId(u32);

pub enum CheckOwner {
    Type(TypeName),
    Project(CheckName),
}
```

ID 由 lowering 按 canonical module/declaration/root-statement 顺序分配，仅在一个 `CftSchema` 生命周期内有效。runtime 和 checker 不持久化 ID，不要求跨 schema rebuild 稳定。

嵌套 statement 不分配独立执行 ID。一个根 `when` 或量词连同 condition、collection、body 和 message 构成一个执行节点。

### 4.2 依赖源

```rust
pub enum CheckDependency {
    Field(CheckField),
    RecordSet(TypeName),
}

pub struct CheckField {
    pub owner: TypeName,
    pub field: FieldName,
}
```

语义：

- `Field(A.f)`：任意可赋值给 `A` 的实际记录的顶层字段 `f` 变化，可能影响 statement。
- `RecordSet(A)`：可赋值给 `A` 的记录成员集合变化，可能影响 statement。
- `id` 作为记录身份依赖，不伪装成普通字段；引用 id 的 statement 额外依赖 `RecordSet(owner root)`，rename 触发该 token。
- const、enum 和纯 literal 不产生数据依赖；schema 变化由完整 rebuild 处理。

### 4.3 Statement 元数据

```rust
pub struct CheckStatementInfo {
    pub id: CheckStatementId,
    pub owner: CheckOwner,
    pub root_index: usize,
    pub dependencies: BTreeSet<CheckDependency>,
    pub dimensions: BTreeSet<DimensionName>,
}

pub(crate) struct CheckIndex {
    statements: Vec<CheckStatementInfo>,
    by_dependency: BTreeMap<CheckDependency, BTreeSet<CheckStatementId>>,
    owners_by_actual_type: BTreeMap<TypeName, Vec<TypeName>>,
    nested_hosts_by_type: BTreeMap<TypeName, BTreeSet<TypeName>>,
}
```

`CheckIndex` 是 `CftSchema` 私有实现，不导出 `TypedCheckPlan`、`TypedCheckSchedule` 或 `ScheduledCheckBlock`。schema 提供窄查询：

```rust
impl CftSchema {
    pub fn check_statement(&self, id: CheckStatementId) -> Option<CheckStatementRef<'_>>;
    pub fn check_statements_for_dependency(
        &self,
        dependency: &CheckDependency,
    ) -> impl Iterator<Item = CheckStatementId> + '_;
    pub fn check_statements_for_actual_type(
        &self,
        actual_type: &TypeName,
    ) -> impl Iterator<Item = CheckStatementId> + '_;
}
```

### 4.4 静态依赖收集规则

依赖收集复用 schema check visitor，不增加第二套 AST walker：

- 裸字段 `price`：收集当前静态 record type 的 `Field(Type.price)`。
- 引用链 `owner.level`：收集宿主的 `Field(Item.owner)` 和目标的 `Field(Character.level)`。
- 多级引用：收集每个跨记录引用字段和最终目标字段。
- nullable/safe access、coalesce、短路：收集所有可能分支的并集。
- `when`：condition、body 和 message 归入同一根 statement。
- 量词：collection、body、message 和 binding 上的字段访问归入同一根 statement。
- `records(T)`：收集 `RecordSet(T)`；通过 binding 访问 `item.price` 时再收集 `Field(T.price)`。
- 数组、dict、嵌套非记录对象：依赖归一化到其所属顶层存储字段，例如 `stats.level`、`rewards[i].count` 均依赖 `Field(Item.stats)` 或 `Field(Item.rewards)`。
- 项目级 check 没有隐式 `self`；数据入口必须能静态归属 `records(T)` 或其 binding。
- formatted message 中的表达式也参与依赖，因为失败状态不变时字段变化仍可能改变消息。
- dimension dependency 继续由 typed expression analysis 得到，但直接写入 statement 的 `dimensions`，不构建另一套 dimension plan。

### 4.5 继承与嵌套对象

- `Field(Base.f)` 对所有可赋值实际类型生效；runtime 查询变化 token 时使用 schema assignability 扩展，不复制字段定义。
- base type statement 对派生记录执行；full planner 按 actual record type 展开 owner chain。
- 嵌套对象 check 仍在顶层记录上下文中执行。`CheckIndex` 预计算包含某 nested type 的顶层 host actual types；跨类型失效时任务 target 始终是顶层 `CfdRecordId`。
- nested field 的内部 path 变化被 runtime 归一化为顶层 host field。checker 不接收 nested path。
- recursive object containment 必须受现有 schema/data structural budget 限制，并有 cycle fixture。

## 5. Runtime 变化模型与任务规划

### 5.1 Mutation 影响

`MutationImpact` 保留 writer、文件和 materialization 所需信息，但 checker 规划只消费以下内部模型：

```rust
pub(crate) struct CheckImpact {
    pub records: BTreeMap<RecordCoordinate, ChangedRecordFields>,
    pub record_sets: BTreeSet<TypeName>,
}

pub(crate) enum ChangedRecordFields {
    All,
    Fields(BTreeSet<ChangedField>),
}

pub(crate) struct ChangedField {
    pub field: FieldName,
    pub projection: ChangedProjection,
}

pub(crate) enum ChangedProjection {
    Base,
    Dimension(DimensionVariant),
}
```

规则：

- 普通字段写入将任意 path 折叠为第一个顶层 `FieldName`。
- dimension value write 保留 `dimension + variant`，不再丢失为普通 path。
- insert/delete/rename/type transfer 产生 `ChangedRecordFields::All` 和对应 actual type/ancestor 的 `record_sets` token。
- 无法提供字段详情的 provider reload 明确产生 `All`。
- spread、materialization 和 folded write 在 runtime 内先展开到实际受影响宿主记录，再形成 `CheckImpact`。
- reorder 只有改变 check 可观察数据或 record set 时才产生影响。

该模型是 runtime 内部类型，不移动到 `coflow-checker`。

### 5.2 最终任务

```rust
pub struct CheckTask {
    pub statement: CheckStatementId,
    pub target: CheckTarget,
    pub projection: CheckProjection,
}

pub enum CheckTarget {
    Record(CfdRecordId),
    Project,
}

pub enum CheckProjection {
    Base,
    Dimension(DimensionVariant),
}
```

`CheckTask` 具有全序，用 `BTreeSet` 去重并确定稳定执行顺序。最终排序键固定为：target kind、record model order、statement ID、base/dimension、dimension/variant schema order。

### 5.3 直接与跨类型失效

对每个 `ChangedField(record, field)`：

1. 根据 actual type 和 ancestors 生成 `Field(Type, field)` token。
2. 查询 `CheckIndex` 得到 statement IDs。
3. statement 的 type owner 能在当前变化记录内执行时，只生成该 record 的任务。
4. statement owner 与变化记录不属于同一顶层宿主时，枚举该 owner 的所有 assignable 顶层记录并生成任务。
5. nested statement 使用 `nested_hosts_by_type` 映射到顶层 host records。
6. project owner 只生成一个 `Project` task。
7. 多个变化命中同一任务时去重。

本方案有意不查询具体引用目标。`Character.level` 变化会为依赖它的 `Item` statement 生成全部 Item record 任务。

### 5.4 RecordSet 失效

- `RecordSet(Base)` 由 Base 或任意派生实际类型的 insert/delete/rename/type transfer 触发。
- 命中的 type-local statement 按 owner 全表执行。
- 命中的 project statement 执行一次。
- 内容字段变化不触发纯 `RecordSet(T)`；若 statement 遍历集合元素字段，静态索引中同时存在相应 `Field(T.f)`。

### 5.5 完整任务规划

`plan_full_checks(schema, model)`：

- 为每个顶层 record 枚举适用的所有 type-local 根 statements。
- 为每个 statement 生成 Base 任务。
- 为 statement 的每个相关 dimension 生成 schema 中所有 variants 的任务。
- 为每个 project 根 statement生成 Base 和相关 dimension tasks。
- 空模型仍执行 project statements。

完整与增量 planner 产生相同 `CheckTask` 类型，并调用同一个 checker API。

### 5.6 维度变化规划

- variant 字段变化只生成该 `dimension.variant` 的相关 statement 任务。
- dimension 字段基础值变化生成 Base 任务，并保守生成该 statement 所有相关 dimensions 的所有 variants，因为 variant 可能 fallback 到基础值。
- 非维度字段变化若命中 dimension-aware statement，生成 Base 及该 statement 全部 dimensions/variants；例如 `name.len() <= price` 中 `price` 变化影响所有 language variants。
- 跨类型维度字段依赖保留相同 projection；`Character.title@language.zh` 触发依赖 statement 的 `language.zh` tasks。
- 不相关 dimension 不生成任务。
- `DimensionRoundView` 重命名为 `CheckProjectionView`，职责仅是按 task projection 读取值和附加 origin。

## 6. 无状态 Checker

### 6.1 公共 API

```rust
pub fn execute_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    tasks: impl IntoIterator<Item = CheckTask>,
    limits: CheckLimits,
) -> CheckOutput;

pub struct CheckOutput {
    pub results: Vec<CheckTaskResult>,
    pub request_diagnostics: Vec<CheckDiagnostic>,
    pub statistics: CheckExecutionStats,
}

pub struct CheckTaskResult {
    pub task: CheckTask,
    pub diagnostics: Vec<CheckDiagnostic>,
}
```

按 task 分组返回诊断只用于保留执行归属，不表示 checker 管理诊断集合。无法归属到单个 task 的预算或请求错误进入 `request_diagnostics`。checker 不接收 previous output，不返回 invalidation、snapshot 或 merged diagnostics。

### 6.2 内部模块

```text
coflow-checker/src/
  lib.rs
  task.rs
  output.rs
  diagnostics/
  engine/
    mod.rs              批量任务循环、请求总预算
    context.rs          单任务可变状态
    evaluator.rs        表达式语义
    statements.rs       指定根 statement 执行
    expressions.rs
    record_walker.rs    在 target record 中定位 nested owner 实例
  projection.rs         Base/Dimension 值投影
  eval/
  operations/
```

`CheckRunner` 删除。批量循环在 `engine::execute_tasks`，记录内嵌套定位在 `RecordCheckWalker`，表达式状态在 `ExecutionContext/CheckEvaluator`。regex cache 和 request budget 由批量执行器共享。

### 6.3 预算

```rust
pub struct CheckLimits {
    pub structure: StructuralLimits,
    pub max_tasks: usize,
    pub max_request_work: u64,
}
```

- runtime planner 在生成任务时受 `max_tasks` 限制。
- checker 为每个 task 使用结构预算，同时对整个请求累计 work。
- 预算失败产生对应 task 的明确诊断；未执行的后续任务不得伪装为成功。
- planner 和 evaluator 的隐藏全表扫描必须计入工作量或在模型索引中完成。

## 7. 诊断边界

checker 只产生本次 task 的 `CheckDiagnostic`。以下能力明确不在 checker：

- 保存上一代诊断；
- 删除已恢复 statement 的旧诊断；
- 合并 partial run 与历史结果；
- project generation 和过期任务取消；
- CLI/LSP/editor 的最终排序与展示。

runtime 的诊断所有者可以使用 `CheckTask` 作为 replacement scope，但该存储与替换 API 不得放进 `coflow-checker`。CLI/full check 直接扁平化所有 `CheckTaskResult`；editor 可按 generation 和 scope 应用结果。该集成需要独立测试，但不改变 checker 的无状态契约。

## 8. 必须先删除的旧概念

开始实现时先在工作树中删除旧模型，让编译错误成为完整迁移清单；在新模型恢复 workspace 编译前不提交过渡状态。

### 8.1 删除类型和模块

从 `coflow-checker` 删除：

- `CheckRequest<'a>`
- `CheckTargets<'a>`
- `CheckChangeSet`
- `ChangedPaths`
- `DependencyCollection`
- `CheckSnapshot`
- `CheckRound`
- `CheckRoot`
- `StableExecutionId`
- `RootCheckState`
- `StableRecordReadDependency`
- `DependencyGraph`
- `DependencyGraphBuilder`
- `DependencyCollector`
- `RecordReadDependency`
- `DimensionCheckRound`
- `DimensionCheckRoundError`
- `DimensionCheckContext`
- `RoundExecution`
- `TargetSelection`
- `RootedCheckDiagnostic`
- 仅服务旧依赖系统的 `CheckExecutionId`

删除文件：

- `crates/coflow-checker/src/request.rs`
- `crates/coflow-checker/src/snapshot.rs`
- `crates/coflow-checker/src/dependencies.rs`

从 `coflow-cft` 删除：

- `TypedCheckPlan`
- `TypedCheckSchedule`
- `ScheduledCheckBlock`

以最终私有 `CheckIndex` 原位替换 `schema/plans/typed_checks.rs`；文件重命名为 `schema/plans/check_index.rs`，不保留 re-export alias。

从 runtime 删除：

- `CheckState` alias
- checker-owned change set 转换
- incremental snapshot fallback 分支
- `dimension_check_rounds`

### 8.2 删除函数和分支

- `select_targets`
- `full_selection`
- `selection_for_targets`
- `select_incremental_targets`
- `run_default_round`
- `run_dimension_round`
- `run_top_level_checks`
- `record_execution`
- `record_top_level_execution`
- `CheckSnapshot::insert_execution`
- `CheckSnapshot::affected_roots`
- `CheckSnapshot::merge_replacement`
- `changed_paths_overlap`
- `path_is_prefix`
- `expand_materialization_changes`（能力移到 runtime mutation impact，不保留 checker 版本）
- evaluator 的 `note_read_from`、`note_value_read` 及所有调用
- `CheckRunner::run_rooted` / `run_top_level` 的 dependency flags 和 graph output

### 8.3 删除完成审计

最终 `rg` 必须无正式代码命中：

```powershell
rg -n "CheckSnapshot|CheckRequest|CheckTargets|CheckChangeSet|ChangedPaths|DependencyCollection|DependencyGraph|DependencyCollector|CheckRound|DimensionCheckRound|TypedCheckSchedule|ScheduledCheckBlock|note_read_from|affected_roots|merge_replacement" crates src editors
```

文档历史可以提及旧名称，但架构参考和公开 API 文档必须只描述新模型。

## 9. 一次性实施顺序

这些是同一工作分支中的施工顺序，不是允许独立合并的兼容阶段。禁止建立旧 API 到新 API 的长期 adapter。

1. **建立删除清单基线**：保存 `rg` 结果、现有 full/incremental 等价 fixture 和 benchmark 基线。
2. **删除旧系统**：删除第 8 节全部类型、模块、函数和导出；不先写包装器。
3. **最终命名落位**：创建 `CheckStatementId`、`CheckDependency`、`CheckIndex`、`ChangedField`、`CheckTask`、`CheckProjection`、`CheckTaskResult`；立即使用最终名称，不引入 `New*`、`V2*`、`Plan2`。
4. **CFT 一次性实现**：在现有 typed analyzer/lowering visitor 中收集根 statement 依赖与 dimensions，构建反向索引、inheritance/nested host 索引和查询 API。
5. **Checker 一次性重构**：删除 Runner 多模式分支，按最终目录拆分 executor/context/walker/projection，实现指定根 statement 的无状态执行。
6. **Runtime 一次性迁移**：MutationImpact 产生最终字段/record-set 变化，full/incremental planner 都生成统一 tasks；迁移 load、session build、write transaction 和 host 调用。
7. **诊断消费迁移**：runtime/host 使用 task scope 管理自己的诊断，不向 checker 增加 state/delta API。
8. **删除所有临时施工代码**：不得留下旧名 alias、deprecated wrapper、双写字段、feature flag、TODO 或“fallback to old engine”。
9. **完整测试与 benchmark**：只有新路径通过全部正确性、等价性、预算和性能门槛后才提交/合并。

施工期间允许工作树暂时不编译，但每个实际提交必须满足仓库 `AGENTS.md`：`cargo check --workspace` 和 `cargo test --workspace` 都通过。推荐在恢复完整垂直路径后形成第一个提交，避免把不可构建的删除阶段写入历史。

## 10. 测试方案

### 10.1 CFT 静态图单元测试

新增 `crates/coflow-cft/tests/check_dependencies.rs`，覆盖：

- 同类型裸字段、多语句独立依赖；
- custom message 字段；
- `when` condition/body；
- all/any/none collection 与 binding 字段；
- nullable/safe/coalesce/短路两侧依赖并集；
- 一层和多层 record-ref field chain；
- `records(Type)` 的 RecordSet 与 element Field 双依赖；
- project check 多 statement 独立索引；
- base/derived assignability；
- nested object/array/dict 归一化到顶层 host field；
- dimension 字段和非维度字段共同参与 dimension-aware statement；
- formatted message；
- const/enum/literal 不产生伪数据依赖；
- visitor 对新增 AST variant 的 exhaustive coverage；
- schema dependency budget 超限。

测试直接断言 canonical `CheckStatementInfo` 和反向索引，不通过 evaluator 行为间接猜图。

### 10.2 Runtime planner 单元测试

新增 `crates/coflow-runtime/tests/check_planning.rs`，覆盖：

- direct owner field 只生成变化记录任务；
- cross-type field 生成 owner 全表任务；
- 多级静态依赖由字段到 statement 直接命中，不依赖运行时引用 key；
- 多条记录、多字段和重复依赖任务去重；
- base field 与 derived actual record；
- nested owner 定位到顶层 host record；
- project statement 恰好执行一次；
- insert/delete/rename 触发 RecordSet；
- pure RecordSet statement 不被普通内容变化触发；
- dimension variant 只生成对应 variant；
- base fallback 生成全部相关 variants；
- 非维度字段触发 dimension-aware statement 的全部 variants；
- 不相关 dimension 不生成；
- empty model/project-only checks；
- max task budget；
- deterministic task order。

### 10.3 Checker 执行测试

重写现有 checker tests，使其只构造明确 tasks：

- 单根 expression/when/quantifier statement；
- record 与 project target；
- nested record walker；
- Base 与 Dimension projection；
- custom message/context/source location；
- task diagnostics 相互隔离；
- 重复 task 由 API 拒绝或稳定去重（最终只选一种行为）；
- task/statement/target 不匹配返回明确内部诊断，不静默跳过；
- 单 task structural budget 与 request total budget。

### 10.4 Full 与增量等价集成测试

对每个 mutation fixture：

1. 在旧模型上执行 full tasks，建立 host-owned scoped diagnostic map。
2. 应用 mutation。
3. runtime 生成 incremental tasks 并执行。
4. 测试 harness 按 task scope 替换结果。
5. 在新模型上独立执行 full tasks。
6. 断言两者 canonical diagnostics 完全相等。

覆盖：

- 同记录字段失败/恢复；
- 跨类型引用目标变化，但实际引用 key 不同；
- 依赖类型无记录、单记录、大量记录；
- insert/delete/rename/type transfer；
- nested object、array、dict；
- spread、materialization 和 folded write；
- project check 与 RecordSet；
- inheritance；
- dimension base/variant/fallback；
- nullable 引用、引用替换和引用环；
- batch mutation 同时命中同一 task；
- schema reload 后 full planning。

等价测试是正确性门槛，不接受“诊断数量相同”；必须比较 code、severity、message、logical record/path、contexts、schema location 和 dimension origin。

### 10.5 静态债务测试

- 迁移完成时用第 8.3 节的 `rg` 删除清单做一次性审计；不在 `repo_hygiene` 长期维护历史 symbol 黑名单。
- 检查 `coflow-checker` 不依赖 runtime mutation 类型。
- 检查 evaluator 不包含 dependency collection 字段。
- 检查 checker public API 不出现 previous/snapshot/incremental/round。
- 检查 schema dependency 和 dimension 分析只使用统一 visitor。

## 11. Benchmark 方案

### 11.1 基准位置与执行方式

新增：

```text
crates/coflow-runtime/benches/check_planning.rs
crates/coflow-runtime/benches/check_execution.rs
```

在 `crates/coflow-runtime/Cargo.toml` 注册 `harness = false` 的可重复 release benchmark，沿用仓库现有手工 `Instant + black_box` 风格，不引入 Criterion 依赖。每个场景 2 次预热、至少 7 次采样，输出 min/median/max、tasks、records、variants、diagnostics 和 records/s。

命令：

```powershell
cargo bench -p coflow-runtime --features internal-check-bench --bench check_planning
cargo bench -p coflow-runtime --features internal-check-bench --bench check_execution
```

`internal-check-bench` 只公开 benchmark 调用 production planner 所需的内部 adapter，不属于产品兼容路径或迁移 feature flag。

### 11.2 必测场景

| 场景 | 数据规模 | 增量变化 | 比较项 |
| --- | --- | --- | --- |
| direct field | 1k/5k/20k Item | 1 个 Item.price | 1 record tasks vs full |
| cross type fanout | 100 Character + 1k/5k Item | 1 Character.level | all Item dependent statement vs full all statements |
| unrelated field | 5k Item，多独立 statements | 1 个无关字段 | affected statement only vs full |
| project RecordSet | 1k/5k Item | insert/delete 1 Item | project statement + new record vs full |
| nested objects | 1k hosts × nested arrays | 1 host root field | one host walker vs full |
| dimensions | 1k/5k records，2/5/10 variants | one variant/base/non-dimension field | selected projection tasks vs all variants |
| batch edits | 1k/5k records | 1/10/100 changed records | dedup planner + execution vs full |
| worst-case fanout | 5k records，所有 statements 依赖同一跨类型字段 | one source field | incremental approaching full |

复杂 check fixture至少包括：18 个根 statements、两个 32-item 量词、string/dict/set/sort/numeric builtins、跨记录字段、project check 和 dimension-aware statement。数据模型构建与 schema 编译不计入 execution benchmark，但 planning benchmark单独计时。

### 11.3 对比输出

每个场景必须同时测：

```text
full_plan_time
full_execute_time
incremental_plan_time
incremental_execute_time
full_task_count
incremental_task_count
speedup = full_total / incremental_total
```

在计时区外断言 full diagnostics 与应用 incremental scoped results 后的 diagnostics 等价，防止用漏执行换取性能。

### 11.4 基线与门槛

实施前用当前 `v0.8` 记录无增量完整检查基线。已观测的复杂 fixture 参考值（本机 release）：

| records | full median | throughput |
| ---: | ---: | ---: |
| 1,000 | 168.6 ms | 5,932 records/s |
| 5,000 | 845.7 ms | 5,912 records/s |
| 20,000 | 3.418 s | 5,851 records/s |

最终验收：

- 新 full execution 在相同 fixture 上不得比上述基线回退超过 15%；超过时必须 profile 并解释。
- direct-field 5k 场景 incremental total 应至少比 full 快 20 倍。
- planning 在 20k records/100 changes 场景不得占 incremental total 的 25% 以上。
- worst-case fanout 允许接近 full，但不得明显慢于 full；目标上限为 1.15 倍。
- 性能数字不作为易抖动的普通单测断言；CI 运行 correctness smoke，release 前人工记录完整 benchmark 表。

之前的 `full_check_benchmark.rs` 原型可作为 fixture seed，但正式基准必须落在 runtime，以覆盖静态索引查询、任务规划和 checker 执行的完整新边界。

## 12. 文档迁移

同步更新：

- `website/docs/docs/reference/02-project-pipeline.md`
- `website/docs/docs/reference/03-language/04-check.md`
- `website/docs/docs/reference/09-diagnostics/01-diagnostics.md`
- `website/docs/docs/reference/10-localization.md`
- `website/docs/docs/reference/11-schema-api.md`
- `website/docs/docs/reference/12-architecture.md`
- `AGENTS.md` crate ownership说明（若最终模块边界变化）

公开文档描述语义，不暴露 runtime 内部任务规划细节；架构文档明确 checker 无状态、依赖静态、诊断集合由 runtime/host 管理。

## 13. 完成门槛

实现完成必须同时满足：

1. 第 8 节旧类型、函数、文件和导出全部删除。
2. workspace 内没有旧新双轨、adapter、deprecated alias、feature flag 或 TODO cleanup。
3. CFT 静态依赖图测试完整覆盖字段、RecordSet、项目级、继承、嵌套和维度。
4. runtime planner 测试覆盖 direct/cross/project/dimension/membership 和去重。
5. 所有 mutation 等价测试证明 incremental 结果与独立 full 一致。
6. benchmark 同时报告 full 与 incremental，并达到第 11.4 节门槛或有审查通过的 profile 解释。
7. `cargo check --workspace` 通过。
8. `cargo test --workspace` 通过。
9. `git diff --check` 通过。
10. 最终架构文档和 public reference 已同步。

本计划完成后的 checker 概念只剩：

```text
CheckTask
CheckTarget
CheckProjection
CheckLimits
CheckTaskResult
CheckDiagnostic
CheckExecutionStats
```

依赖、增量选择和诊断集合都不属于 checker。
