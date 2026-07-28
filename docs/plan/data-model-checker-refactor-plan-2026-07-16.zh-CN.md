# DataModel 与 Checker 重构最终计划

- 日期：2026-07-16
- 状态：最终实施计划
- 范围：`coflow-cft`、`coflow-data-model`、`coflow-checker`、`coflow-runtime` 中的维度编排，以及直接依赖相关 Rust API 的模块
- 分支：`codex/data-model-checker-refactor-plan`

## 1. 目标

本计划用于消除 DataModel 与 Checker 流程中重复的类型和值语义，明确 source draft 与成功 canonical model 的边界，并使现有增量检查真正做到目标局部化且可以被观测和验证。

计划同时定义一条分阶段实现非结构性 DataModel 增量重建的路径，但不引入以下设计：

- 跨 generation 永久有效的数字 record ID；
- 可变 generation；
- synthetic dimension type、record 或独立长期 dimension store；
- 第二套 schema 类型声明模型；
- 静默发生但无法解释的全量 fallback。

整个工作首先是行为保持型重构，其次才是经过测量的性能优化。除非后续有单独批准的产品变更，否则任何阶段都不得改变用户数据语法、项目配置、诊断码、导出数据形状、生成的 C# 形状、编辑器 wire JSON 或 mutation transaction 语义。

## 2. 当前基线

以下现有设计是正确基础，必须保留：

1. `CftSchema` 持有唯一的成功 type、field、enum、const 和 dimension declarations。
2. 继承字段共享同一个 `Arc<CftField>`，不复制字段声明。
3. `TypedCheckPlan` 与 `ValueDependencyPlan` 在 schema 发布前完成编译。
4. dimension variant value 作为 overlay 附着在 owner `CfdRecord` 上，不存在 synthetic dimension record。
5. direct ref、spread 和 checker read dependency 使用不同图模型，因为三者的失效规则不同。
6. runtime generation 不可变，只有 load、model build、check 和 transaction 条件全部满足后才发布。
7. scalar mutation rebuild 复用已编译 schema，并只 reload affected source batches。
8. checker diagnostics 与 read dependencies 通过 record 业务身份跨 generation 稳定化。

当前需要解决的问题包括：

1. `CftSchemaTypeRef` 实际表达递归字段值类型树，命名会误导为 schema declaration reference。
2. `CheckedType` 除了 compiler-only 状态外，还重复定义了大部分成功值类型。
3. DataModel build validation 与 mutation value validation 分别实现了重叠的语义规则。
4. `CfdInputValue`、`CfdValue`、`CheckValue` 形成三棵范围过于接近的值树。
5. DataModel 通过 `CfdDomainIndex` 复制 schema 的 type/domain/ancestry 信息。
6. Checker 公开 full/subset、dimension/default、deps/no-deps、options/default-options 的组合入口。
7. checker 输出与 runtime 自己维护的 stable diagnostic/check state 分裂。
8. subset dimension check 仍会为每个 variant 扫描完整模型构建 `DimensionRoundView`。
9. spread path lookup 已有相关索引，却仍扫描全部 spread edges。
10. source reload 已增量，但 `CfdDataModel::builder(...).build()` 仍从全部缓存输入重建完整模型。

## 3. 固定架构决策

以下决策作为本次重构的最终约束，不在实施中反复选择其他表示。

### 3.1 Canonical schema declarations

`CftSchema` 继续作为唯一成功 schema 语义所有者：

- `CftType` 是唯一 named type declaration；
- `CftField` 是唯一 field declaration；
- `CftEnum` 是唯一 enum declaration；
- `CftDimension` 是唯一 dimension declaration；
- declaration identity 使用经过验证的 typed names；
- DataModel 和 Checker 不发布或长期保存第二套 type declaration graph。

### 3.2 字段值类型

将 `CftSchemaTypeRef` 重命名为 `CftValueType`，将 `CftField.ty_ref` 重命名为 `CftField.value_type`。

`CftValueType` 是递归值类型表达式树，不是 declaration object，也不是复制出来的 schema type handle：

```rust
pub enum CftValueType {
    Int,
    Float,
    Bool,
    String,
    Object(TypeName),
    Enum(EnumName),
    RecordRef(TypeName),
    Array(Box<CftValueType>),
    Dict(Box<CftValueType>, Box<CftValueType>),
    Nullable(Box<CftValueType>),
}
```

模块从 `schema/type_ref.rs` 改为 `schema/value_type.rs`。不保留 `CftSchemaTypeRef` deprecated alias。本仓库中的 crate 当前不发布，保留旧名只会延续歧义。

### 3.3 Compiler inference

将 `CheckedType` 替换为 `InferredType`。成功值类型只通过 `CftValueType` 表示，compiler-only 状态单独保留：

```rust
enum InferredType {
    Value(CftValueType),
    Null,
    EmptyArray,
    EmptyObject,
    EnumNamespace(EnumName),
    Entry(Box<InferredType>, Box<InferredType>),
    Unknown,
}
```

compiler 在完成名称验证前可以保留 unresolved syntax name，但成功发布的 schema 与 typed check plan 只能包含 validated typed names。

成功类型的 assignability、comparability 和 container 规则只能有一套实现。`InferredType` 只添加临时状态与错误恢复所需规则。

### 3.4 Source draft 不是第二套 canonical model

source input 必须能表示缺失字段、spread directive、尚未解析的语法和非法数据，因此需要独立的 ingest 表示。但它是可失败的编译 IR，不是另一套成功 DataModel。

Provider contract 使用明确名称：

```rust
LoadedRecordDraft
LoadedValueDraft
DimensionValueDraft
```

这些名称替换容易与成功模型混淆的 `CfdInput*`。draft 可以包含成功 `CfdValue` 不允许存在的 syntax/build directives，但不得复制 schema assignability、nullable、enum、dict key、object 或 ref target 规则。

Provider 内部 parser AST、cell token 可以继续存在。它们属于 adapter 状态，必须先经过共享 ingest/semantic 边界，才能进入成功 model。

### 3.5 Record identity

将 `RecordCoordinate` 从 `coflow-runtime` 移到 `coflow-data-model`：

```rust
pub struct RecordCoordinate {
    pub actual_type: TypeName,
    pub key: RecordKey,
}
```

它是跨 generation 和 wire boundary 使用的稳定业务身份。序列化形状继续保持现有 `{ actual_type, key }` 字符串对象。

`CfdRecordId` 继续是 generation-local dense index。任何需要跨 generation 保存的状态都必须先转换为 `RecordCoordinate`。

本计划明确不引入永久 numeric record ID、tombstone、record arena 或 record generation token。

### 3.6 增量范围

第一版增量 DataModel 只支持 record membership、coordinate 和顺序不变的非结构性值修改。

结构变化在所有相关索引和生命周期规则具备经过验证的 delta 实现前，使用明确的 full fallback。fallback 是正确性边界，必须报告 machine-readable reason。

## 4. DataModel 目标结构

目标目录：

```text
coflow-data-model/src/
  lib.rs
  model/
    mod.rs
    ids.rs
    record.rs
    value.rs
    dimensions.rs
  ingest/
    mod.rs
    record.rs
    value.rs
    directives.rs
  semantics/
    mod.rs
    validation.rs
    assignability.rs
    references.rs
  build/
    mod.rs
    context.rs
    defaults.rs
    resolve.rs
    materialize.rs
  indexes/
    mod.rs
    records.rs
    references.rs
    spreads.rs
  dependencies/
    mod.rs
    materialization.rs
  diagnostics/
    mod.rs
    codes.rs
    paths.rs
    mapping.rs
```

只有在语义所有权明确后才移动目录。禁止先做大范围文件搬迁，再立即做第二次大范围内容修改。

### 4.1 成功模型的 typed identity

成功 DataModel 内部统一使用 typed identity：

```rust
CfdRecord.actual_type: TypeName
CfdRecord.key: RecordKey
CfdObject.actual_type: TypeName
CfdObject.fields: BTreeMap<FieldName, CfdValue>
CfdValue::Ref(RecordKey)
CfdEnumValue.enum_name: EnumName
CfdEnumValue.variant: Option<EnumVariantName>
CfdRecord.dimension_fields: BTreeMap<FieldName, CfdDimensionOverlay>
```

普通 string value、dict string key、source display path、diagnostic text，以及 draft 中尚未验证的字段名继续使用 `String`，因为它们不是 schema declaration identity。

### 4.2 唯一值语义校验

DataModel build 与 runtime mutation preflight 调用同一个递归 semantic validator。概念接口为：

```rust
pub struct ValueValidationRequest<'a> {
    pub expected: &'a CftValueType,
    pub value: ValueView<'a>,
    pub mode: ValidationMode,
    pub pending_insert: Option<PendingInsertRef<'a>>,
}
```

`ValidationMode` 区分：

- source fragment：允许 object 省略字段；
- complete value：required field 必须完整；
- mutation：允许 pending records，并附加 mutation context。

以下规则只能实现一次：

- nullable；
- primitive compatibility 与 finite float；
- object actual type、abstract/singleton 限制和 assignability；
- required field 与 default；
- enum name、variant、numeric value 一致性；
- array/dict 递归；
- dict-key compatibility；
- record-ref shape 与 target validation。

source spread syntax、source location、default application 和 mutation expected-state 不属于该核心。

### 4.3 Schema relationship 与数据索引

删除作为 DataModel 公共概念存在的 `CfdDomainIndex`、`CfdTypeId` 和 `CfdDomainId`。它们当前复制了 `CftSchema` 已拥有的 type name、domain、member 和 ancestor 信息。

数据索引改用 canonical typed names：

```rust
record_by_type_key: BTreeMap<(TypeName, RecordKey), CfdRecordId>
record_by_domain_key: BTreeMap<(TypeName, RecordKey), CfdRecordId>
```

第二个索引中的 `TypeName` 是 inheritance root。`CftSchema` 提供 inheritance root、assignability、ancestors 和 descendants 查询。

删除 numeric cache 前必须加入 lookup benchmark。如果出现实质回退，应在 `CftSchema` 内优化查询，而不是恢复公开的第二套 DataModel 类型系统。

### 4.4 Record/ref/spread indexes

保留 exact-type 与 inheritance-domain record indexes，因为它们回答不同查询。

`inheritance_index` 只有在其生产 consumer 迁移完成且 benchmark 通过后才删除。

成功模型新增 `spread_by_host`。`spread_edge_at_path` 只能搜索目标 host 的 edge，不再扫描项目全部 spread edges。`spread_by_site` 继续服务 exact-site 与 source rewrite。

以下三张图必须独立：

- direct ref graph：描述存储的 record-ref 关系；
- spread graph：描述物化来源和写回 provenance；
- check read graph：描述运行期 check 实际读取。

### 4.5 非结构性增量模型重建

在行为保持型重构和统计完成后，成功 records 可以改为：

```rust
records: Vec<Arc<CfdRecord>>
```

非结构性 mutation 顺序：

1. 只 reload affected source batches。
2. 按 `RecordCoordinate` 比较 draft fingerprint。
3. 找到直接变化 records。
4. 沿 materialization dependencies 扩张，初期只包含 spread provenance。
5. 只重新 validate/materialize affected records。
6. 复用未变化的 `Arc<CfdRecord>`。
7. 替换 affected host 的 relation edges，并修复 reverse buckets。
8. 输出包含 changed coordinates、dimension coordinates 和 relation changes 的 `ModelDelta`。

只依赖 schema 和本 record 的 default 不产生跨 record dependency。普通 ref target 的字段内容变化不需要重建引用方，checker read dependency 负责重新检查。spread source 内容变化必须重建继承相关字段的 hosts。

insert/delete/rename 在本项目范围内继续 full model fallback。它们需要同时处理 ordering、key uniqueness、singleton、refs、spreads、dimension rows、idAsEnum、diagnostics 和 checker snapshot membership，不在第一版增量模型中实现。

## 5. Checker 目标结构

```text
coflow-checker/src/
  lib.rs
  request.rs
  output.rs
  snapshot.rs
  dependencies.rs
  dimensions.rs
  engine/
    mod.rs
    runner.rs
    evaluator.rs
    statements.rs
    expressions.rs
  eval/
    mod.rs
    value.rs
    location.rs
    collections.rs
  operations/
    mod.rs
    access.rs
    comparison.rs
    predicates.rs
    builtins.rs
    quantifiers.rs
  diagnostics/
    mod.rs
    explanations.rs
    trace.rs
```

### 5.1 单一执行入口

用一个入口替换组合 API：

```rust
pub fn run_checks(
    schema: &CftSchema,
    model: &CfdDataModel,
    request: CheckRequest<'_>,
) -> CheckOutput;
```

`CheckRequest` 包含 targets、rounds、structural limits 和 dependency collection policy。

`CheckOutput` 始终返回 rooted diagnostics、requested snapshot delta 或 complete snapshot、dependencies 和 execution statistics。empty targets 不做任何工作。

consumer 全部迁移后，删除 `CfdCheckExt` 以及 full/subset/dimension/dependency/options wrapper matrix。

### 5.2 借用式求值值

`CheckValue` 不再复制完整成功 `CfdValue` algebra，改为 model borrow 与 evaluator-only states：

```rust
enum EvalValue<'a> {
    Model {
        value: &'a CfdValue,
        location: ValueLocation,
    },
    Temporary(TemporaryValue),
    EnumNamespace(EnumName),
    Entry(EvalEntry<'a>),
    UnresolvedRef,
}
```

Temporary value 只表示模型中不存在的表达式运算结果。collection iteration 继续 lazy 且携带 location，以保持 structural budget 和 precise diagnostics。

### 5.3 Checker snapshot 所有权

将增量 checker state 从 runtime 移入 `coflow-checker`：

```rust
pub struct CheckRoot {
    pub record: RecordCoordinate,
    pub round: CheckRound,
}

pub struct RootCheckState {
    pub diagnostics: Vec<LogicalCheckDiagnostic>,
    pub reads_from: BTreeSet<RecordCoordinate>,
}

pub struct CheckSnapshot {
    pub roots: BTreeMap<CheckRoot, RootCheckState>,
}
```

runtime 继续拥有 physical source-location mapping 和 session publication，但不再维护平行的 stable checker diagnostic model。

dependency 按 root 和 round 保存。当前把全部 dimension rounds 合并为一张图虽然正确，但会过度失效；新 snapshot 明确保存 round。

### 5.4 增量 checker 失效

给定 `ModelDelta`：

1. 将每个 changed record 加入 roots。
2. 加入旧 snapshot 中 `reads_from` 与 changed records 相交的 roots。
3. 在新 generation 解析 affected coordinates。
4. 只执行 affected roots 和 required rounds。
5. 替换它们的旧 snapshot entries。
6. 保留全部 unaffected root states。

第一版保持 record-level dependency。只有 measurement 证明有需要后才考虑 field-level dependency，避免过早引入 path、dimension 和 collection 级失效复杂度。

## 6. 维度架构与优化

### 6.1 不可改变的维度语义

以下行为属于兼容契约：

1. project dimension names 与 variants 参与 schema compilation。
2. `@localized` 和 `@dimension` 将普通 owner field 绑定到 dimension。
3. `CftDimension` 保存 typed variants 与共享 canonical fields。
4. 普通 source field 是唯一 semantic default value。
5. dimension 文件中的 `default` 只是 managed physical mirror。
6. variant values 直接附着 owner records。
7. missing variant 与 explicit null 必须保持不同状态。
8. dimension refs 参与 direct ref index 与 rename rewrite。
9. singleton dimension source 保持 CFD grouping 行为。
10. non-singleton dimension source 保持 CSV path/table 行为。
11. mutation、managed-file changes、compensation 和 generation publication 保持同一 transaction lifecycle。
12. JSON/MessagePack export 与 C# codegen 保持现有 dimension table 名称和形状。

### 6.2 Generation-bound runtime plan

当前 runtime 会重复从 canonical fields 派生 `Vec<DimensionField>` 并进行目录匹配。改为每个 generation 编译一次：

```rust
pub(crate) struct DimensionRuntimePlan {
    fields_by_dimension: BTreeMap<DimensionName, Vec<DimensionFieldPlan>>,
    source_by_path: BTreeMap<PathIdentity, DimensionSourcePlan>,
}
```

`DimensionFieldPlan` 只引用 `TypeName`、`FieldName` 和 canonical field semantics，不复制 `CftField` 或 `CftValueType`。

`DimensionSourcePlan` 保存 runtime-only storage policy：bucket、managed path、provider choice、singleton grouping 和 decoded source options。

只有 schema、dimension config、provider availability 或 managed source topology 变化时才重建该 plan。文件命名和 CSV/CFD policy 属于 runtime，不进入 canonical schema。

### 6.3 Target-local dimension checking

删除 `DimensionRoundView::compile` 的 whole-model projection。

每个 requested root/round 的执行顺序：

1. 从 `TypedCheckPlan` 获取 dimension-relevant statements。
2. 根据 canonical actual type 与 runtime/check request plan 确定相关 dimension fields。
3. 只读取当前 root record 在 requested variant 下的 overlay。
4. 保持 missing 与 explicit null 语义。
5. 只对确实 materialized 且需要 nested checks 的值递归。
6. dimension ref read 使用与 default round 相同的 dependency collector。

full check 仍由 request 遍历全部 roots；subset check 不得 project unrelated records。

### 6.4 Impact-scoped dimension regeneration

维度 regenerate/reload 使用明确 impact plan：

- 普通非 dimension field 变化：不做 dimension source work；
- dimensional field default 变化：只更新对应 managed default mirror；
- 单个 variant 变化：只 write/reload owning dimension source；
- insert/delete/rename：更新适用于 owner type 的全部 dimension fields；
- dimension ref rewrite：只更新 affected dimension sources；
- schema、variants、out dir、bucket、provider 或 source topology 变化：full dimension reconcile。

第一版可以保守地 regenerate 一个 affected dimension 的全部 fields，但不得 regenerate unrelated dimensions，并且必须通过统计暴露该扩张。

## 7. Runtime 职责

`coflow-runtime` 继续作为 generation orchestrator，负责：

- compiled schema/module snapshots；
- source resolution 与 source batch cache；
- generation-bound dimension runtime plan；
- DataModel build/rebuild 调用；
- checker full/incremental 调用；
- source provenance 与 physical diagnostics mapping；
- mutation transaction、compensation 和 publication；
- explicit fallback decision。

runtime 不得重新实现 value typing、DataModel materialization、check evaluation 或 checker snapshot merge semantics。

fallback reason 使用稳定 internal enum：

```rust
enum IncrementalFallbackReason {
    SchemaChanged,
    RecordInserted,
    RecordDeleted,
    RecordRenamed,
    SourceTopologyChanged,
    DimensionConfigurationChanged,
    ProviderConfigurationChanged,
    UnstableCoordinateMapping,
    IncompleteDependencyState,
}
```

## 8. 兼容性要求

必须保持以下契约：

- CFT/CFD source syntax；
- `coflow.yaml` shape 和 dimension config；
- diagnostic code、stage、primary/related spans 和代表性 message；
- CLI human/JSON output；
- editor request/response JSON 与 TypeScript bindings；
- record coordinate wire shape；
- JSON/MessagePack table names、ordering 与 value representation；
- C# public types、fields、dimension tables、loaders 和 idAsEnum；
- CSV/Excel/CFD loader/writer behavior；
- mutation expected-state、no-op、batch、transaction、compensation 与 affected files；
- source-origin mapping 与 spread write routing；
- missing/explicit-null dimension semantics。

重命名的 unpublished internal Rust type 不提供 source compatibility alias。迁移必须在仓库内一次完成并删除旧名。

## 9. 实施阶段与提交边界

### Phase 0：基线可观测性

在不改变行为的前提下增加 internal/test-only statistics：

```text
sources_resolved
sources_reloaded
draft_records_collected
records_validated
records_materialized
records_reused
ref_edges_rebuilt
spread_edges_rebuilt
check_roots_executed
dimension_records_projected
dimension_sources_planned
dimension_sources_written
full_fallback
fallback_reason
```

增加 full/incremental output equivalence 和代表性 work-count tests。

提交：`test(runtime): expose model and checker execution statistics`

### Phase 1：值类型命名

1. `type_ref.rs -> value_type.rs`。
2. `CftSchemaTypeRef -> CftValueType`。
3. `CftField.ty_ref -> value_type`。
4. 更新所有 loader、writer、model、checker、exporter、codegen、runtime、LSP 和 tests。
5. 不改变任何匹配逻辑和输出。

提交：`refactor(cft): rename schema type refs to value types`

### Phase 2：Compiler inference 统一

1. 引入 `InferredType`。
2. 成功类型统一为 `InferredType::Value(CftValueType)`。
3. 集中 successful-type assignability/comparability。
4. 保持所有 CFT diagnostic code 和 span。
5. 完成迁移后删除 `CheckedType`。

提交：`refactor(cft): unify canonical and inferred value types`

### Phase 3：Typed record identity

1. 将 `RecordCoordinate` 移入 `coflow-data-model`。
2. schema identity 对应的 actual type、record key、field name、enum name 和 dimension field map key 改为 typed names。
3. wire serialization 保持 string。
4. unknown/invalid name 只能存在于 draft/diagnostic 层。

提交：`refactor(data-model): use canonical typed record identity`

### Phase 4：共享值语义

1. 提取唯一 semantic validator。
2. build validation 与 mutation validation 适配到同一实现。
3. 建立 draft/build 与 mutation conformance matrix。
4. 删除重复 recursive rules。
5. source spread/default application 保持独立。

提交：`refactor(data-model): centralize value semantics`

### Phase 5：Ingest 与 model 边界

1. 将 `CfdInput*` 重命名并移动为明确 loaded draft IR。
2. 按 model/build/indexes/dependencies/diagnostics 所有权拆分目录。
3. schema adapter 与 build state 分离后删除 `compiler_context.rs`。
4. 增加 `spread_by_host` 并迁移 path lookup。

提交：`refactor(data-model): separate ingest build and model state`

### Phase 6：删除重复 schema relationship model

1. 在 canonical schema 增加 inheritance-root 等必要查询。
2. record indexes 与 edge metadata 迁移为 typed names。
3. benchmark lookup/build cost。
4. 删除 `CfdTypeId`、`CfdDomainId`、`CfdDomainIndex`。
5. consumer 与性能验证完成后迁移或删除 `inheritance_index`。

提交：`refactor(data-model): use canonical schema relationships`

### Phase 7：Checker value/API 收敛

1. 引入 borrowed `EvalValue` 和 temporary evaluator values。
2. 合并 operations modules。
3. 引入 `CheckRequest/CheckOutput`。
4. 迁移全部 consumers。
5. 删除 wrapper APIs 和 `CfdCheckExt`。

提交：`refactor(checker): consolidate evaluation and execution APIs`

### Phase 8：Checker snapshot 所有权

1. 引入 per-root/per-round `CheckSnapshot`。
2. 将 stable diagnostic/dependency merge 从 runtime 移到 checker。
3. runtime 只保留 physical-location mapping。
4. 增量结果与 fresh full output 做差分验证。

提交：`refactor(checker): own incremental check snapshots`

### Phase 9：维度 runtime/checker 优化

1. 每个 generation 编译一次 `DimensionRuntimePlan`。
2. whole-model dimension projection 改为 target-local projection。
3. regeneration/reload 按 dimension impact 缩小。
4. 保持 source generation、transaction、export 和 codegen contract。

提交：`perf(runtime): make dimension work generation and target scoped`

### Phase 10：非结构性增量 DataModel

1. 引入共享成功 record nodes。
2. 增加 draft fingerprint 和 materialization dependencies。
3. 实现 affected-record rematerialization 与 relation-edge replacement。
4. 输出 `ModelDelta`。
5. checker invalidation 改由 `ModelDelta` 驱动。
6. structural fallback 保持明确。

只有 Phase 0 数据证明，在 Checker 和 dimension 优化后 full model materialization 仍是实质成本，才实施本阶段。如果不是主要成本，保留 full immutable model rebuild，并将其记录为有意的简单性选择，不引入无人受益的复杂度。

如实施，提交：`perf(data-model): rebuild non-structural model deltas`

## 10. 验证矩阵

### 10.1 Schema/compiler

- primitive/container/object/enum/ref/nullable 所有组合；
- unknown/ambiguous names；
- invalid dict key；
- null、empty array、empty object inference；
- enum namespace 与 entry inference；
- inherited check schedule；
- diagnostic code、span、related label 稳定性。

### 10.2 DataModel semantics

- draft/build 与 mutation validation conformance；
- required/default/nullable；
- abstract/sealed/struct/singleton/polymorphic object；
- enum/flag enum；
- dict key 与 duplicate；
- direct ref、missing/wrong-domain target；
- object/dict spread；
- spread provenance 与 effective ref lookup；
- structural budget。

### 10.3 Checker

- 全部 expression、statement、builtin、predicate、quantifier；
- nested object/array/dict checks；
- borrowed evaluation 与当前 diagnostics 等价；
- 每次 dereference 都进入 read dependency；
- per-round dimension dependencies；
- budget exhaustion；
- empty subset 不执行工作；
- incremental snapshot 与 fresh full run 等价。

### 10.4 Dimensions

- `@localized` 和 generic `@dimension`；
- multiple dimensions/variants；
- missing 与 explicit null；
- nested dimension values/checks；
- dimension ref/rename rewrite；
- singleton CFD grouping；
- non-singleton CSV/bucket；
- generated source create/update/remove/rename；
- mutation set/clear/insert/delete/rename；
- compensation/affected files；
- target-local checker work counts；
- impact-scoped regeneration work counts；
- JSON/MessagePack golden；
- C# generation golden。

### 10.5 Runtime/editor

- affected/unaffected provider source reload count；
- no-op mutation 不推进 generation；
- structural mutation 报告 full fallback；
- stale expected-state；
- diagnostics index 属于当前 generation；
- editor wire compatibility；
- watcher attribution 与 generated dimension paths；
- incremental/fresh session diagnostics equality。

## 11. 性能验收

性能工作必须同时证明结果等价和实际工作量下降。

单条、无 reader、无 spread dependent 的 scalar 修改必须满足：

- 只 reload owning source；
- 不 plan/write/reload unrelated dimension source；
- 如果启用增量 DataModel，只 materialize 该 record；
- 只执行该 record 的 check roots；
- 不 project unrelated dimension records；
- 不发生 full fallback。

被其他 check 读取的 record 变化时：

- changed root 与 direct reader roots 执行；
- unrelated roots 保留旧 snapshot；
- output 与 fresh full check 相同。

spread source 变化时：

- 继承 affected fields 的 hosts rematerialize；
- unrelated spread hosts 不处理；
- effective ref 与 write provenance 保持正确。

benchmark 必须覆盖 deep inheritance、many refs、many spreads、multiple dimensions 和 multiple variants。wall-clock 作为辅助证据，deterministic work counters 是主要 regression guard。

## 12. 风险控制

1. type rename 不与 semantic behavior change 合并。
2. directory movement 不与 algorithm replacement 合并。
3. execution statistics 完成前不实现 incremental model delta。
4. structural operation 没有 differential test 前不删除 full fallback。
5. runtime storage policy 不进入 canonical schema。
6. source draft recovery state 不得进入成功 `CfdValue`。
7. `CfdRecordId` 不跨 generation 持久化。
8. ref、spread、checker dependency graphs 不合并。
9. dimension performance work 不改变 export/codegen shape。
10. model/check/transaction/mapping 任一 required step 失败时不得发布 generation。

每个 phase 必须可独立回退。如果某阶段无法保持既有行为和 required checks，应停在上一个稳定边界，而不是通过兼容 shim 掩盖未完成迁移。

## 13. 仓库检查

每个普通开发提交只运行仓库根目录规定的两项检查：

```powershell
cargo check --workspace
cargo test --workspace
```

普通开发提交不要求 `cargo fmt` 或 `cargo clippy`。release/packaging 不属于本计划，必须遵守 `AGENTS.md` 中单独定义的完整 release gate。

## 14. 完成条件

全部满足以下条件后，重构才算完成：

1. `CftSchemaTypeRef`、`CheckedType` 和 `CfdCheckExt` 不再存在。
2. 成功 schema value type 只使用 `CftValueType` 与 validated typed names。
3. DataModel build 与 mutation 共用一套 value semantics。
4. source draft 有明确 ingest IR 命名，不会与成功 model value 混淆。
5. DataModel 不再发布重复 schema type/domain declaration model。
6. typed `RecordCoordinate` 负责跨 generation identity，numeric ID 保持 generation-local。
7. Checker 借用 model values，并只有一个 public request/output pair。
8. Checker 拥有 per-root/per-round snapshot 和 dependency merge。
9. subset dimension check 不执行 whole-model projection。
10. dimension runtime plan generation-bound，regeneration impact-scoped。
11. incremental work 与 fallback reason 可观测。
12. 每个支持的增量操作都与 fresh full output 等价。
13. 全部 compatibility requirements 与 golden outputs 保持不变。
14. `cargo check --workspace` 和 `cargo test --workspace` 通过。

本计划有意允许 structural mutation 使用完整 immutable model rebuild。如果 Checker 和 dimension 优化完成后，测量表明 model build 不是主要成本，也可以继续对全部 model mutation 使用 full rebuild。只要它被明确记录且不伪装成增量，这属于经过验证的简单性取舍，而不是隐藏技术债。
