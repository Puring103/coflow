# Aggressive Coflow refactor plan

本文基于当前 crate 架构审计结果，给出一个不考虑向后兼容、以代码质量和长期演进为目标的激进重构方案。这里的“不兼容”包括：允许改公开 Rust API、CLI 内部 JSON 字段、provider trait、project session API、模块路径、测试 fixture、生成代码结构；但不建议无谓改变用户可见的 CFT/CFD 语言语义，除非能明显消除历史设计债务。

## 1. Refactor goals

核心目标：

1. 让 runtime pipeline 显式化：high-level `build` 可以是复合命令，但内部必须拆成可单独测试的 load/generate/reload/check/export/codegen steps。
2. 让 engine 真正 provider-neutral：engine 不直接依赖 CSV/Excel/CFD/Lark 具体实现。
3. 让 schema/type/value 语义只有一个权威实现：直接替换 `coflow-cft::CftSchemaView`，checker、data model、codegen 复用同一套 query/type service。
4. 拆掉千行级中心文件：把大模块拆成可测试、可替换、职责明确的小模块。
5. 统一 diagnostics/source mapping：成功记录、失败记录、重复记录、provider parse error 都能稳定映射回 source。
6. 明确 read/write/generate/export/codegen 的 pipeline：每一步输入输出可测试、可回滚、可缓存、可并发。
7. provider 操作能力显式化：load、write field、insert/delete/rename、create table、sync header、remote retry 都有清晰 contract。

非目标：

- 不以最小 diff 为目标。
- 不保留旧 provider trait 形状。
- 不保留当前 `ProjectSession` 的所有字段和方法名。
- 不迁就当前单文件结构。
- 不把兼容层作为长期代码保留。

## 2. Overall optimization plan

整体优化不是简单拆文件，而是重建 Coflow 的主干边界。目标是把“语言解析、schema 语义、数据模型、provider 契约、runtime pipeline、host 编排”变成单向依赖的层级，并让每一层都有明确输入输出。

### 2.1 Make the runtime pipeline explicit

现状问题：

- session build 同时做 schema build、source load、model build、check、dimension source regeneration、reload。
- build 行为确实可以包含维度生成，但当前实现把写入副作用隐藏在 session build 中，难以单测、回滚和复用。

激进优化：

- high-level `build` 继续是复合命令，可以包含维度变体生成。
- 底层 runtime 拆出显式步骤：base load、dimension plan、dimension commit、reload with generated sources、check、export/codegen。
- dimension generation 不再隐藏在 `build_project_session` 内部，而是由 build pipeline 显式编排。
- artifact commit 从 export/codegen 中移出，变成独立 commit step。
- pipeline 每一步产出结构化 result：`SchemaOutput`, `LoadOutput`, `ModelOutput`, `CheckOutput`, `DataSession`。
- dimension commit 必须支持事务回滚。

收益：

- 构建可缓存、可并发、可 dry-run。
- LSP/editor 可以调用只读 session API；CLI `build` 可以调用复合 build API。
- diagnostics 更容易定位到具体阶段。

### 2.2 Make engine provider-neutral

现状问题：

- engine 直接依赖 CFD/CSV/table-core 等 provider implementation。
- `data_files.rs` 硬编码 `DataFileProvider::{Cfd,Csv,Excel}`。
- create/sync header 等行为没有统一进入 provider operation。

激进优化：

- engine 改为 runtime，只依赖 provider API。
- provider API 拆出 `SourceProvider`, `SourceWriter`, `TableManager`。
- create table、sync header、field write、insert/delete/rename、rewrite refs 都是 provider operation。
- runtime 不允许 import CSV/Excel/CFD/Lark provider crate。

收益：

- 新增 provider 不改 runtime。
- provider 能力以 operation set 表达，不再靠 scattered bool 和 engine 特判。
- 格式细节回到 provider 内部。

### 2.3 Create one authoritative schema/type/value semantic layer

现状问题：

- CFT schema view、data-model schema view、codegen schema view、checker runtime type/value、LSP type display 各自解释 schema。
- 语义变更容易出现 CLI、LSP、checker、codegen 不一致。

激进优化：

- 在 `coflow-cft` 内直接替换现有 `CftSchemaView`，提供唯一 `CftSchemaQuery` / `CftTypeSystem`。
- checker、data model、codegen 不再自建 schema view；LSP 后续阶段再迁移。
- value validation、assignability、polymorphic domain、dict key、nullable、ref target 规则集中。

收益：

- 语义规则只改一处。
- 可以建立跨 crate conformance tests。
- checker、data model、codegen 与真实 compile 行为保持一致；LSP 后续再接入同一 query。

### 2.4 Consolidate duplicated data structures

现状问题：

- 多个 crate 有相似但不完全相同的数据结构：schema view、field/type meta、value enum、diagnostic/location、record/source identity、table source/sheet/location。
- 这些结构不是纯 DTO 重复，而是夹带语义规则；重复定义会导致行为漂移。

激进优化：

- 统一“权威语义结构”和“公共 identity/location 结构”。
- provider-specific config 和格式 DTO 可以保留，但不能重复实现 schema/type/value 语义。
- 对外暴露 query/facade，避免把所有结构合并成一个巨大上帝对象。

需要统合的结构族：

| Structure family | Current duplicated forms | Target owner |
|---|---|---|
| Schema/type view | `CftSchemaView`, data-model `SchemaView`, C# `SchemaView`, LSP type lookup helpers | `coflow-cft::CftSchemaView` rebuilt as the single query/type-system layer |
| Field/type meta | `CftTypeMeta`, data-model `TypeMeta/FieldMeta/CfdType`, C# `TypeMeta/FieldMeta/FieldType` | `coflow-cft` shared metadata/query |
| Value semantics | `CfdValue`, `CfdInputValue`, checker `CheckValue`, table `ParsedCell`, exporter/codegen local type handling | `coflow-data-model` values + `coflow-cft/coflow-data-model` value semantics services |
| Diagnostics/location | API diagnostic, CFT diagnostic, CFD diagnostic, Table/CSV/Excel/Lark diagnostics, LSP adapters | `coflow-api::Diagnostic` as the single host-facing format, with unified conversion paths |
| Record/source identity | `RecordCoordinate`, `CfdRecordId`, `RecordOrigin`, `SourceId`, provider row/cell/span references | `coflow-model` + `coflow-runtime` identity layer |
| Table source model | `CsvSource/CsvSheet`, `ExcelSource/ExcelSheet`, `LarkSheetSource`, `TableSource/TableSheet` | `coflow-provider-table` shared table model + provider adapters |

收益：

- schema/type/value 语义只改一处。
- codegen/export/check 能共享一致 query；LSP 后续迁移。
- diagnostics 和 UI source mapping 不再在各层丢失信息。
- provider 保留格式差异，但不重复公共表格语义。

边界原则：

- 统一语义，不强行统一所有存储形态。
- 统一 query，不暴露可变全局结构。
- 保留 provider-specific DTO，例如 Lark API response、Excel workbook adapter、CSV parser rows。
- 保留 check runtime value 也可以，但其构造、比较、类型判断必须调用共享 value semantics。

### 2.5 Make diagnostics and source mapping first-class

现状问题：

- 成功 records 有 index；rejected/duplicate records 的 source metadata 不完整。
- diagnostics 在 provider/model/engine 间多次转译。

激进优化：

- 引入 `SourceEntryId`, `SourceRecordId`, `RejectedRecordIndex`。
- loader 输出的每个 record，无论是否进入 model，都保留 source identity。
- diagnostic label 不只指向 model record，也可指向 source record、table row、cell、text span。

收益：

- UI/editor 可以展示失败记录。
- 自动修复和 write planning 可以处理更多错误状态。
- duplicate/rejected source 不再从 runtime index 消失。

### 2.6 Split high-risk modules into testable units

现状问题：

- checker evaluator、project、API、engine 都有千行级文件；LSP 和 Lark 也很大，但第一阶段先不做。

激进优化：

- 所有核心非测试文件目标小于 800 行；复杂算法文件超过 500 行必须有明确理由。
- checker 拆 env/eval/ops/builtins/diagnostics/deps。
- LSP 拆 protocol/state/features 和 Lark 拆 HTTP/auth/metadata/load/write/retry/dto/diagnostics 放到后续阶段。
- project 拆 config/path/schema discovery/diagnostics。

收益：

- 单元测试粒度更小。
- review 风险可控。
- 后续功能不会继续堆到单个热点文件。

### 2.7 Keep export/codegen behavior stable while cleaning structure

现状问题：

- exporter/core、JSON、MessagePack、C# codegen 都有自己的 schema/type 解释逻辑或局部 view。
- C# codegen 的 emit 逻辑集中，维护成本高。

激进优化：

- 不新增任何导出元数据、格式版本字段、schema 摘要或额外导出包裹结构。
- JSON/MessagePack 保持现有输出语义。
- exporter 和 codegen 只改内部结构：复用 `coflow-cft` 统一 query，拆模块，补 golden tests。

收益：

- 不扩大导出格式范围。
- 降低重复语义和大文件维护成本。
- 保持现有导出/生成行为可对比。

## 3. Local crate optimization plan

本节先给局部 crate 优化总览；后面的“Concrete crate-by-crate refactor actions”给更具体的动作和验收。

| Current crate | Local aggressive optimization |
|---|---|
| `coflow-api` | 保留 crate 名字，但重做为窄 API crate；按 diagnostics/artifacts/provider/exporter/codegen/registry/operations modules 拆开，承载 loader/writer/exporter/codegen/host contract。 |
| `coflow-builtins` | 保持内置 provider 注册职责；只随 API 重构调整注册接口，不新增 provider profile 功能。 |
| `coflow-cft` | 拆 syntax/schema 子模块；直接重建并替换 `CftSchemaView`，成为唯一 schema/type query。 |
| `coflow-cfd` | 变成纯 syntax/recovery parser crate；format/write 下沉到 CFD provider。 |
| `coflow-data-model` | 改为 `coflow-model`；引入 source/rejected indexes；依赖 `coflow-cft` 的统一 `CftSchemaView`/query。 |
| `coflow-checker` | 移除 project 依赖；拆 evaluator；统一 numeric/value operation。 |
| `coflow-project` | 拆 config/path/schema discovery；删除环境变量展开功能，让 config parse 成为纯函数。 |
| `coflow-engine` | 改为或替换为 `coflow-runtime`；删除 provider implementation 依赖；high-level build 显式编排维度事务。 |
| `coflow-loader-table-core` | 改为 `coflow-provider-table`；拆 cell/load/write-plan/diagnostics。 |
| `coflow-loader-cfd` | 改为 `coflow-provider-cfd`；拆 loader/writer/formatter/patch planning。 |
| `coflow-loader-csv` | 改为 `coflow-provider-csv`；实现 table manager；明确 CSV 容错策略。 |
| `coflow-loader-excel` | 改为 `coflow-provider-excel`；所有 workbook mutation 留在 provider。 |
| `coflow-loader-lark` | 后续阶段改为 `coflow-provider-lark` 并拆 HTTP/auth/load/write/retry/cache/DTO；第一阶段只做必要适配。 |
| `coflow-exporter-core` | 保持现有输出语义；内部改用统一 schema query，拆清遍历/编码边界。 |
| `coflow-exporter-json` | 保持现有 JSON 输出；增加 golden fixture。 |
| `coflow-exporter-messagepack` | 保持现有 MessagePack 输出；增加 decode roundtrip/golden fixture。 |
| `coflow-codegen-csharp` | 删除自有 schema view；拆 emit；保持现有生成语义。 |
| `coflow-lsp` | 后续阶段拆 server/protocol/state/features；第一阶段只做必要适配。 |

## 4. Target crate architecture

建议把当前 18 个 crate 重组为清晰的层级。允许新增 crate，但不为了命名洁癖而新增；第一阶段重点是重做 `coflow-api`、替换 `coflow-engine` 为 runtime、抽出/重命名 table provider core。

### 4.1 Language layer

目标 crate：

- `coflow-cft` 内部 `syntax` 子模块。
- `coflow-cfd` 可保留为 syntax crate。

职责：

- 只负责 lex/parse/AST/span/syntax diagnostics。
- 提供 recovery parser 给 LSP/editor。
- 不包含 schema compile、data model、provider 逻辑。

迁移：

- 在 `coflow-cft` 内整理 `lexer.rs`, `parser.rs`, `ast.rs`, `span.rs`, syntax-only error；不强制新增 `coflow-syntax-cft`。
- 从 `coflow-cfd` 保留 AST/parser，并把 writer roundtrip 相关测试拉到 syntax 层。

### 4.2 Schema semantic layer

目标位置：

- `coflow-cft`。

职责：

- CFT AST 到 compiled schema。
- schema query/type service。
- default/check type rules 的唯一权威实现。
- dimension schema synthesis 的纯函数部分。

核心目标类型：

- `CftSchemaView`：直接替换现有实现，升级为唯一 schema/type query 层。
- `TypeId`, `EnumId`, `FieldId`, `ModuleId`：稳定 typed IDs。
- `TypeRef`, `FieldDef`, `EnumDef`, `CheckAst`, `DefaultValue`。
- `CftSchemaQuery<'db>`：只读查询 facade。
- `CftTypeSystem`：assignability、nullable、ref target、dict key、polymorphic domain。
- `CheckTypeChecker`：check expression type rules。

迁移：

- 合并当前 `coflow-cft::CftSchemaView`、`coflow-data-model::schema_view`、`coflow-codegen-csharp::schema_view` 中重复的 schema projection。
- 删除 codegen/data-model 自有 schema view；它们只依赖 `coflow-cft` 的统一 query。

### 4.3 Data semantic layer

目标 crate：

- `coflow-model`

职责：

- 将 provider records 编译为 validated data model。
- 管理 record/table/domain/ref/spread/index。
- 管理 source mapping 和 diagnostics 映射。
- 提供 write value validation。

核心目标类型：

- `InputRecord`, `InputValue`, `InputDictKey`。
- `DataModel`, `RecordId`, `TableId`, `RecordKey`, `RecordCoord`。
- `RecordOrigin`, `SourceSpan`, `SourceRecordId`, `SourceEntryId`。
- `RefGraph`, `SpreadGraph`。
- `RejectedRecordIndex`：保存 build 失败、重复、被过滤记录的完整来源。
- `ValueSemantics`：写入/插入时复用的值校验。

关键改变：

- `RecordId` 不再直接等于 input order。引入 `SourceRecordId` 表示 loader 输入顺序，`RecordId` 表示 model 内成功记录。
- duplicate/rejected records 不再从 engine index 消失，而是进入 `RejectedRecordIndex`。
- data model 编译只接收 `coflow-cft` 的统一 query/type-system，不再自建 schema view。

### 4.4 API and provider contract layer

目标 crate：

- `coflow-api`，保留名字但重做内容。

职责：

- 定义 loader/writer/exporter/codegen trait。
- 定义 provider registry、provider diagnostics adapter、operation capabilities。
- 不依赖具体 provider crate。
- 尽量只依赖 `coflow-cft` 统一 query 和 `coflow-data-model`/`coflow-model` 的窄 facade。

核心目标 trait：

```rust
trait SourceProvider {
    fn descriptor(&self) -> ProviderDescriptor;
    fn probe(&self, ctx: ProbeCtx, source: &ConfiguredSource) -> ProbeResult;
    fn resolve(&self, ctx: ResolveCtx, source: &ConfiguredSource) -> Result<Vec<ResolvedSource>, Diagnostics>;
    fn load(&self, ctx: LoadCtx, source: &ResolvedSource) -> Result<LoadedSource, Diagnostics>;
}

trait SourceWriter {
    fn capabilities(&self, source: &ResolvedSource) -> WriteCapabilities;
    fn plan(&self, ctx: WriteCtx, op: WriteOp) -> Result<WritePlan, Diagnostics>;
    fn commit(&self, ctx: WriteCtx, plan: WritePlan) -> Result<WriteResult, Diagnostics>;
}

trait TableManager {
    fn create_table(&self, ctx: TableCtx, request: CreateTable) -> Result<TableResult, Diagnostics>;
    fn sync_header(&self, ctx: TableCtx, request: SyncHeader) -> Result<TableResult, Diagnostics>;
}

trait DataExporter {
    fn descriptor(&self) -> ExporterDescriptor;
    fn export(&self, ctx: ExportCtx, output: OutputSpec) -> Result<ArtifactSet, Diagnostics>;
}

trait CodeGenerator {
    fn descriptor(&self) -> CodegenDescriptor;
    fn generate(&self, ctx: CodegenCtx, output: OutputSpec) -> Result<ArtifactSet, Diagnostics>;
}
```

关键改变：

- `DataLoader` 和 `DataWriter` 拆成 source provider、writer、table manager。
- `DataExporter` 和 `CodeGenerator` 仍属于 `coflow-api`，但放在独立 exporter/codegen 模块中。
- create table 和 sync header 都是 provider operation，不允许 engine 直接写 CSV/Excel/CFD。
- `WriteCapabilities` 从一组 bool 改为 operation set：`EditField`, `RenameKey`, `InsertRecord`, `DeleteRecord`, `RewriteRefs`, `CreateTable`, `SyncHeader`。
- provider options 增加 typed decode contract，所有 provider 共享 option diagnostic 格式。

### 4.5 Runtime/pipeline layer

目标 crate：

- `coflow-runtime`

职责：

- project schema build。
- source resolve/load。
- model build。
- check run。
- diagnostics/index aggregation。
- read-only session。
- write transaction orchestration。
- generation/export/codegen orchestration hooks。

核心目标类型：

- `Runtime`.
- `SchemaSession`.
- `DataSession`.
- `SessionOptions`.
- `BuildPlan`.
- `LoadPlan`.
- `DiagnosticIndex`.
- `SourceIndex`, `RecordIndex`, `RejectedRecordIndex`, `FileIndex`, `DependencyIndex`。
- `WriteTransaction`, `WritePlan`, `WriteCommitResult`。

关键改变：

- `build_data_session(options)` 默认只读。
- dimension generation 是 high-level build 内部的显式 transaction step，不在 low-level session build 里隐式执行。
- write 成功后重建 session 仍可以作为第一阶段策略，但通过 `WriteTransaction` 封装；之后再优化为 incremental reload。
- runtime 不依赖任何 provider implementation，只依赖重做后的 `coflow-api`。

### 4.6 Provider implementations

目标 crate：

- `coflow-provider-cfd`
- `coflow-provider-table`
- `coflow-provider-csv`
- `coflow-provider-excel`
- `coflow-provider-lark`
- `coflow-export-json`
- `coflow-export-messagepack`
- `coflow-codegen-csharp`

职责：

- 具体格式和远程 API 实现。
- 共享表格逻辑全部放 `coflow-provider-table`。

关键改变：

- `coflow-provider-cfd` 同时依赖 syntax CFD 和 model facade，包含 parse/load/write/format。
- CSV/Excel/Lark 都使用同一个 table adapter，但能力差异通过 provider feature set 表达。
- Lark 拆文件放到后续阶段：`http`, `auth`, `metadata`, `load`, `write`, `dto`, `diagnostics`, `retry`。
- export/codegen 只依赖 schema/model query，不直接解释重复语义。

### 4.7 Host layer

目标 crate：

- root CLI crate。
- `coflow-lsp`。
- editor backend。

职责：

- 参数解析、命令编排、JSON/human 输出、LSP protocol。
- 不直接改数据源。
- 不直接解释 schema 语义；只调用 runtime/schema services。

关键改变：

- LSP 后续阶段使用 recovery parser 和 `coflow-cft` 统一 query。
- LSP 的 state/protocol/features 拆模块放后续阶段。
- CLI command 只调用 runtime command API。

## 5. New pipeline design

### 5.1 Low-level read-only session pipeline

目标流程：

1. `ProjectConfigLoader::load`.
2. `SchemaPipeline::parse`.
3. `SchemaPipeline::compile`.
4. `SourceResolver::resolve_all`.
5. `SourceLoader::load_all`.
6. `ModelCompiler::compile`.
7. `CheckRunner::run`.
8. `SessionAssembler::assemble`.

这是底层只读 session pipeline，供 LSP/editor/dry-run/测试使用。所有阶段返回结构化 output：

- `SchemaOutput { schema_db, source_map, diagnostics }`
- `LoadOutput { loaded_sources, source_index, diagnostics }`
- `ModelOutput { model, rejected_records, diagnostics }`
- `CheckOutput { diagnostics, dependencies }`
- `DataSession { schema, model, indexes, diagnostics }`

硬约束：

- 不写文件。
- 不生成 dimension sources。
- 不 commit artifacts。
- 不 mutate provider cache，除了 provider 内部只读 token/cache。

### 5.2 Write pipeline

目标流程：

1. host 提交 `WriteCommand`。
2. runtime 用 session indexes 定位 source/record/path。
3. runtime 调 provider `plan`。
4. provider 返回 `WritePlan`，包含 precise source edit 或 remote operation。
5. runtime 调 provider `commit`。
6. runtime rebuild read-only session。
7. runtime 返回 `WriteCommitResult`。

硬约束：

- CLI/editor 不直接写文件。
- engine/runtime 不知道 CSV quote、Excel workbook、CFD span rewrite、Lark API endpoint。
- 失败时 provider 返回可映射 diagnostics，不 panic。

### 5.3 High-level build and dimension generation pipeline

high-level `build` 是复合命令，允许包含维度变体生成。目标流程：

1. `build_data_session(include_generated_sources=false)` 加载基础数据。
2. `DimensionPlanner::plan(schema, model, config)` 生成维度写入计划。
3. 通过 provider operation commit dimension plan，不能在 runtime 里直接写 CSV/CFD/Excel。
4. `build_data_session(include_generated_sources=true)` 重新加载普通 sources + 已生成维度 sources。
5. run checks。
6. export/codegen。
7. commit artifacts。

关键改变：

- dimension generation 是 high-level build 的显式 pipeline step，不是 session build 的隐式副作用。
- dry-run 返回将创建/修改/删除的文件、记录和列。
- 维度 source 参与普通 build。
- 支持并发保护：写入前校验文件仍是 plan 时看到的版本。

维度生成规则：

- 可以更新 `default`。
- 可以新增记录。
- 可以删除记录。
- 可以新增列。
- 可以删除列。
- 绝对不能覆盖玩家翻译：已有 variant 单元格内容必须原样保留，除非对应记录或列被删除。
- 删除记录或列允许删除对应翻译内容，这是显式接受的不兼容行为。

事务和回滚：

- dimension commit 前记录所有目标文件的旧内容或 hash。
- commit 后如果 reload/check/export/codegen/artifact commit 任一阶段失败，必须回滚维度生成造成的所有文件变更。
- 新建文件回滚时删除文件。
- 修改文件回滚时恢复旧内容。
- 删除记录/列回滚时恢复旧内容。
- commit 前 hash 不匹配时中止，避免覆盖并发修改。

### 5.4 Export/codegen pipeline

目标流程：

1. build read-only session。
2. export/codegen 只读取 `CftSchemaView`/query + `DataModelQuery`。
3. 输出现有 `ArtifactSet`。
4. artifact commit 是独立步骤。

关键改变：

- 不改变 JSON/MessagePack/C# 生成结果的外部格式。
- exporter/codegen 只共享 `coflow-cft` 统一 query，避免重复 schema view。

## 6. Data structure consolidation plan

这部分专门处理“多处重复定义的数据结构”。重构时应先统合这些结构，再大规模改 pipeline，否则新 pipeline 仍会复制旧语义。

### 6.1 Consolidate schema views first

当前重复：

- `coflow-cft::schema_view::CftSchemaView`
- `coflow-data-model::schema_view::SchemaView`
- `coflow-codegen-csharp::schema_view::SchemaView`
- `coflow-lsp` 中的 type lookup、field lookup、type formatting helpers
- checker evaluator 对 `CftSchemaTypeRef` 的直接解释

目标：

- 直接替换 `coflow-cft::CftSchemaView`，让它成为唯一 schema/type query 层。
- 建立 `CftSchemaQuery`，覆盖 type/enum/field 查询、继承、多态、维度字段、check block、enum value。
- 建立 `CftTypeSystem`，覆盖 assignability、nullable unwrap、dict key validity、ref target、object actual/declared type rules。

迁移顺序：

1. 在 `coflow-cft` 中重建 `CftSchemaView`，先覆盖当前 CFT schema view 能力。
2. data-model compiler 改用新的 `CftSchemaView`/query。
3. checker runner/evaluator 改用新的 query + type-system。
4. C# codegen 删除自有 schema view。
5. 删除旧 `CftSchemaView` 或只保留薄 wrapper。
6. LSP completion/hover/definition 的局部 type lookup 后续阶段再迁移。

验收：

- workspace 中只剩一处 schema/type 权威 query。
- `rg "struct SchemaView|CftSchemaView|FieldType|CfdType"` 不再出现多个语义等价类型。

### 6.2 Consolidate value semantics without forcing one value enum

当前重复：

- `CfdInputValue`：loader 输入值。
- `CfdValue`：validated model value。
- `CheckValue`：checker runtime value。
- `ParsedCell`：table cell parse result。
- exporter/codegen 对 `CftSchemaTypeRef` 的本地 value/type switch。

目标：

- 保留不同阶段的 value enum，但统一转换和规则：
  - `InputValue -> ModelValue`
  - `ModelValue -> CheckValue`
  - `CellText -> InputValue`
  - `ModelValue -> ExportValue`
- 建立共享 `ValueSemantics`：
  - `validate_input_value(schema, expected_type, value)`
  - `validate_model_value(schema, expected_type, value)`
  - `coerce_default(schema, expected_type, default)`
  - `compare_values`
  - `dict_key_from_value`
  - `render_value_for_source`

原则：

- 不要把 `CheckValue` 强行并入 `CfdValue`；checker 需要 `EnumNamespace`, `Entry`, lazy record refs 等 runtime-only 形态。
- 不要让 table cell parser 直接返回 model value；它应该返回 input value 或 typed parsed token。
- 所有类型判断必须经 `TypeSystem`，不能在各 crate 自己 match `CftSchemaTypeRef`。

验收：

- float division/pow/NaN/inf 有唯一策略和测试。
- enum/dict/ref/null/default 在 table、CFD、checker、export 中结果一致。

### 6.3 Consolidate diagnostics and locations

当前重复：

- `coflow-api::Diagnostic`
- `coflow-cft::CftDiagnostic`
- `coflow-data-model::CfdDiagnostic`
- `CfdTextDiagnostic`
- `TableDiagnostic`
- `CsvDiagnostic`
- `ExcelDiagnostic`
- `LarkDiagnostic`
- LSP JSON diagnostic helper

目标：

目标：

- 不优先新增 `coflow-diagnostics`。
- 保留当前 `coflow-api::Diagnostic` / `DiagnosticSet` 作为 host-facing 统一格式。
- 各领域保留自己的 error code enum，但统一到 `coflow-api::Diagnostic` 的转换路径。
- LSP/CLI 只消费统一 diagnostic，不认识 provider-specific diagnostic struct。

位置模型：

- `Location::FileSpan`
- `Location::TableCell`
- `Location::TableRow`
- `Location::RemoteDocument`
- `Location::RecordPath`
- `Location::ConfigPath`

验收：

- provider diagnostic 不需要先转 table diagnostic 再转 API diagnostic 再转 LSP diagnostic。
- 每个 diagnostic 至少有 stage/code/message，能稳定 JSON 序列化。

### 6.4 Consolidate record and source identity

当前重复：

- `CfdRecordId`
- `RecordCoordinate`
- `RecordRef`
- `SourceId`
- `RecordOrigin`
- table row/cell origin
- provider-specific document/sheet/cell identity

目标：

- `coflow-model` 拥有 `RecordId`, `RecordCoord`, `SourceRecordId`, `SourceEntryId`, `RecordOrigin`, `SourceSpan`。
- runtime/engine 拥有 `SourceIndex`, `RecordIndex`, `RejectedRecordIndex`, `FileIndex`。
- provider 只能创建 source identity，不能创建 model identity。

关键改变：

- loader 输出每条 record 都带 `SourceRecordId`。
- model build 成功后才分配 `RecordId`。
- duplicate/rejected records 进入 `RejectedRecordIndex`，不丢失 source identity。

验收：

- duplicate key diagnostics 能列出所有冲突 source locations。
- UI 能查询“这个 source row 为什么没有进入 model”。

### 6.5 Consolidate table model while preserving provider config

当前重复：

- `CsvSource/CsvSheet/CsvLocation`
- `ExcelSource/ExcelSheet/ExcelLocation`
- `LarkSheetSource/LarkSheetLocator`
- `TableSource/TableSheet/TableLocation`

目标：

- `coflow-provider-table` 提供公共 `TableDocument`, `TableSheet`, `TableRange`, `TableCell`, `TableHeader`, `TableMapping`, `TableOrigin`, `TableWritePlan`。
- CSV/Excel/Lark 保留 provider config：CSV file/encoding、Excel workbook/sheet/format policy、Lark credentials/spreadsheet token/API locator。

原则：

- 公共 table model 表达二维表格语义。
- provider config 表达来源和能力差异。
- table write plan 是 provider-neutral，commit 是 provider-specific。

验收：

- table cell parse/render 测试只写一套。
- CSV/Excel/Lark 的 header mapping 共享同一实现。
- provider-specific tests 只关注文件/API 差异。

## 7. Concrete crate-by-crate refactor actions

### 7.1 `coflow-api`

激进处理：

- 保留 `coflow-api` 名字，但重做为窄 API crate。
- 删除当前大一统 `lib.rs` 结构，拆成 `diagnostics`, `artifacts`, `provider`, `writer`, `exporter`, `codegen`, `registry`, `operations` 模块。
- 不新增 `coflow-provider-api`；loader/writer/exporter/codegen trait、registry、operation contract 仍放在 `coflow-api`。
- provider trait 不再直接暴露完整 `CfdDataModel` 和 `CftContainer`；改用 `CftSchemaView`/query、model query、`WriteCtx`。

验收：

- `coflow-api` 没有单文件千行聚合。
- diagnostics/artifacts 模块不依赖 schema/model。
- provider registry 不依赖具体 provider。

### 7.2 `coflow-cft`

激进处理：

- 整理 syntax 和 schema semantic 子模块。
- 直接替换 `CftSchemaView`，让它成为唯一 schema/type query 层。
- 删除 data-model/codegen/checker 自有 schema view 后，`coflow-cft` 成为唯一 query 来源；LSP 后续迁移。

验收：

- parser 不依赖 schema compiler。
- schema compile 不依赖 provider/model/runtime。
- type rules 只在一处实现。

### 7.3 `coflow-cfd`

激进处理：

- 成为纯 syntax crate。
- writer serializer/formatter 移到 `coflow-provider-cfd`，但 syntax crate 保留 roundtrip test fixtures。
- 增加 recovery parser API。

验收：

- LSP 可以用 recovery parse 获取 partial AST。
- provider CFD writer 使用 syntax span，不复制 parser。

### 7.4 `coflow-data-model`

激进处理：

- 改名为 `coflow-model`。
- 编译器输入从直接解释 `CftContainer` 改为使用新的 `CftSchemaView`/query。
- 引入 `SourceRecordId` 与 `RejectedRecordIndex`。
- 把 origin/source mapping 做成一等公民。

验收：

- duplicate/rejected records 可被 UI 精确定位。
- model compile 不依赖 provider implementation。
- `RecordId` contract 明确为 model-local stable ID。

### 7.5 `coflow-checker`

激进处理：

- 删除对 `coflow-project` 的依赖。
- evaluator 拆成 `env`, `eval`, `ops`, `builtins`, `diagnostics`, `deps`。
- numeric semantics 由 `TypeSystem`/`ValueOps` 明确。

验收：

- `evaluator.rs` 不存在或小于 500 行。
- float division/pow/NaN/inf 有明确测试。
- dependency graph 可独立测试。

### 7.6 `coflow-project`

激进处理：

- 拆为 `config`, `paths`, `schema_sources`, `diagnostics`。
- 删除 `${VAR}` 环境变量展开功能；配置解析读到什么字符串就保留什么字符串。
- source/output options 走 provider-declared typed option schema。

验收：

- config parse 是纯函数，不读 env。
- project crate 不依赖 provider API，只依赖 diagnostics/schema syntax。

### 7.7 `coflow-engine`

激进处理：

- 改名为 `coflow-runtime`。
- 删除具体 provider dependencies。
- 删除 `data_files.rs` 的硬编码 provider enum。
- `build_project_session` 拆成显式 pipeline。
- high-level build 继续包含 dimension generation，但该步骤必须经过 provider operation，并支持事务回滚。

验收：

- runtime `Cargo.toml` 不出现 CSV/Excel/CFD/Lark provider implementation。
- low-level read-only session build 不调用 `fs::write/create_dir_all`；high-level build 的写入只发生在 dimension transaction/provider commit step。
- 所有 write/create/sync 通过 provider operation。

### 7.8 `coflow-loader-table-core`

激进处理：

- 改名为 `coflow-provider-table`。
- 分拆 `schema_mapping`, `cell`, `load`, `write_plan`, `diagnostics`。
- 表格能力从“统一模型压平差异”改为 feature-based。

验收：

- CSV/Excel/Lark 可声明不同 table features。
- cell parser/render 和 value semantics 有 conformance tests。

### 7.9 `coflow-loader-cfd`

激进处理：

- 改名为 `coflow-provider-cfd`。
- loader/writer/formatter/span patch 分模块。
- provider writer 只输出 plan/commit，不让 runtime理解 CFD 结构。

验收：

- CFD writer patch planning 可单测。
- rename/rewrite refs 有 roundtrip golden tests。

### 7.10 `coflow-loader-csv`

激进处理：

- 改名为 `coflow-provider-csv`。
- 只保留 CSV parse/write 和 provider adapter。
- header sync/create file 实现 `TableManager`。

验收：

- runtime 不再调用 `coflow_loader_csv::write`。
- CSV BOM/empty row/duplicate header 策略明确。

### 7.11 `coflow-loader-excel`

激进处理：

- 改名为 `coflow-provider-excel`。
- Excel workbook mutation 全部放 provider。
- header sync/create sheet 实现 `TableManager`。

验收：

- runtime 不再引用 `calamine`。
- 样式/公式保护策略有测试。

### 7.12 `coflow-loader-lark`

激进处理：

- 第一阶段只做 provider API 必要适配，避免同时重构远程 API 细节。
- 后续阶段改名为 `coflow-provider-lark`。
- 后续阶段拆成 `http`, `auth`, `metadata`, `values`, `loader`, `writer`, `retry`, `dto`, `diagnostics`。
- 后续阶段禁止 `put_json` 默认 fallback 到 POST，token cache key 包含 app_id、tenant identity、secret hash。

验收：

- 第一阶段现有行为不回退。
- 后续阶段 HTTP method contract tests 覆盖 GET/POST/PUT/DELETE。
- 后续阶段 rate limit/token expired/sheet missing 有分类错误。

### 7.13 exporters

激进处理：

- 保持 JSON/MessagePack 现有输出格式和文件组织。
- `coflow-exporter-core` 只整理内部职责：schema traversal、value encoding、错误上下文。
- JSON/MessagePack exporter 改用统一 `CftSchemaView`/query。

验收：

- JSON/MessagePack golden tests 证明输出没有非预期变化。
- MessagePack 有 decode roundtrip test。

### 7.14 `coflow-codegen-csharp`

激进处理：

- 删除自有 schema view，使用 `coflow-cft` 的统一 `CftSchemaView`/query。
- `emit.rs` 拆 declarations/properties/loaders/database/serialization。
- 保持现有生成语义，不改变现有导出/生成结果。

验收：

- 生成 C# runtime 能消费现有导出 fixture。
- 命名冲突、保留字、idAsEnum、多态都有 golden tests。

### 7.15 `coflow-lsp`

激进处理：

- 第一阶段只做必要适配，避免扩大重构面。
- 后续阶段拆模块：`server`, `protocol`, `state`, `diagnostics`, `completion`, `hover`, `definition`, `formatting`, `semantic_tokens`, `cfd_features`。
- 后续阶段使用 recovery parser 和 `coflow-cft` 统一 query。
- 后续阶段再考虑 incremental cache。

验收：

- 第一阶段 LSP 测试不回退。
- 后续阶段 `lib.rs` 小于 300 行。
- 后续阶段 CFT 和 CFD feature tests 能直接测 feature module。

## 8. Migration strategy

### Phase 0: Lock behavior before breaking APIs

目标：

- 建立现有行为基线，避免重构时无意改变语义。

动作：

1. 增加 workspace-level golden tests：
   - CFT parse/compile fixtures。
   - CFD load/write roundtrip。
   - CSV/Excel/table cell fixtures。
   - checker numeric/ref/spread fixtures。
   - JSON/MessagePack export fixtures。
   - C# codegen fixtures。
2. 增加 session purity regression test：low-level read-only session build 不写文件。
3. 增加 high-level build transaction test：维度生成后任一后续阶段失败必须回滚。
4. 增加 provider boundary regression test：runtime crate 不依赖 provider implementation。
5. 增加 dimension generation golden tests：更新 default、增删记录、增删列、保留玩家翻译、回滚恢复旧内容。

### Phase 1: Rebuild API and schema query foundations

目标：

- 保留并重做 `coflow-api`，不新增 `coflow-provider-api`。
- 在 `coflow-cft` 内直接替换 `CftSchemaView`，不新增 `coflow-schema`。
- 允许新增或重命名 `coflow-provider-table`；可选新增 `coflow-artifacts` / `coflow-diagnostics`，但第一阶段不强制。

动作：

1. 将 `coflow-api/src/lib.rs` 拆为 diagnostics/artifacts/provider/writer/exporter/codegen/registry/operations 模块。
2. 重建 `coflow-cft::CftSchemaView`，提供统一 query/type-system。
3. 让 data-model/checker/codegen 依赖新的 `CftSchemaView`。
4. 删除 `coflow-project` 的环境变量展开。
5. 用最少桥接保持 workspace 可编译；不长期保留旧路径。

删除条件：

- 所有自有 schema view 替换完成。
- 编译期和运行期 type semantics 测试通过。

### Phase 2: Rebuild runtime pipeline

目标：

- 新建或替换为 `coflow-runtime` pipeline。
- high-level build 是复合命令，包含维度生成；low-level session build 仍可只读。

动作：

1. 实现 `SchemaPipeline`, `LoadPipeline`, `ModelPipeline`, `CheckPipeline`, `SessionAssembler`。
2. 实现 `DimensionPlanner` 和 transaction-backed `DimensionCommit`。
3. 重写 `build_project_session` 为 wrapper，内部走显式 pipeline。
4. high-level build 编排 base load -> dimension plan/commit -> reload with dimensions -> check/export/codegen。
5. 任一后续阶段失败时回滚 dimension commit。

删除条件：

- 旧 `build_project_session` 不再直接含 load/check/regenerate/reload 复杂逻辑。
- dimension source 写入只发生在显式 transaction/provider commit step。

### Phase 3: Replace provider contracts

目标：

- provider operations 全部显式化。

动作：

1. 在 `coflow-api` 中新建 `SourceProvider`, `SourceWriter`, `TableManager`。
2. 迁移 CFD/CSV/Excel/Lark provider。
3. 删除 runtime 中的 concrete provider imports。
4. 删除 `DataFileProvider` enum。

删除条件：

- `coflow-runtime/Cargo.toml` 不依赖 provider implementation。
- create table/sync header 都走 provider API。

### Phase 4: Split high-priority large modules

目标：

- 先消除会阻碍 provider/runtime/schema 重构的大文件中心化。LSP 和 Lark 后置。

动作：

1. checker evaluator 拆，补 numeric semantics。
2. project config/path/schema discovery 拆。
3. engine/runtime pipeline 拆。
4. API provider/writer/exporter/codegen/registry/operations 拆。
5. LSP 和 Lark 在后续阶段拆。

验收：

- 没有单个非测试 `.rs` 文件超过 800 行；核心算法文件超过 500 行必须有明确理由。

### Phase 5: Export/codegen structure cleanup

目标：

- 不改变导出格式和生成语义，只清理内部结构并补测试。

动作：

1. exporter/codegen 改用统一 `CftSchemaView`/query。
2. 拆 exporter traversal/value encoding/error context。
3. 拆 C# `emit.rs`。
4. 补 JSON/MessagePack/C# golden fixtures 和 MessagePack decode roundtrip。

## 9. Proposed target dependency graph

目标依赖方向：

```text
coflow-cft ────────────────────────────┐
coflow-cfd ────────────────────────────┤
                                       ├─> coflow-data-model / coflow-model ─┐
coflow-api ────────────────────────────┤                                      │
coflow-provider-table ─────────────────┘                                      │
                                                                              │
providers ─────────────────────────────────────────────────────────────────────┤
                                                                              │
coflow-runtime ────────────────────────────────────────────────────────────────┘
       │
       ├─> exporters
       ├─> codegen
       └─> hosts: CLI / LSP / editor backend
```

禁止依赖：

- runtime -> concrete provider implementation。
- checker -> project config。
- provider/host API -> concrete provider implementation。
- config parse -> environment access or `${VAR}` expansion。
- low-level read-only session build -> filesystem writes。
- high-level build -> direct filesystem writes outside provider transaction。

## 10. API shape after refactor

### 10.1 Runtime public API

建议形状：

```rust
pub struct Runtime {
    registry: ProviderRegistry,
}

impl Runtime {
    pub fn open_project(&self, path: Option<&Path>) -> Result<ProjectHandle, Diagnostics>;
    pub fn build_schema(&self, project: &ProjectHandle) -> Result<SchemaSession, Diagnostics>;
    pub fn build_data_session(&self, project: &ProjectHandle, options: DataBuildOptions) -> Result<DataSession, Diagnostics>;
    pub fn build_project(&self, project: &ProjectHandle, options: BuildOptions) -> Result<BuildResult, Diagnostics>;
    pub fn plan_write(&self, session: &DataSession, command: WriteCommand) -> Result<WritePlan, Diagnostics>;
    pub fn commit_write(&self, plan: WritePlan) -> Result<WriteCommitResult, Diagnostics>;
    pub fn plan_dimension_generation(&self, session: &DataSession) -> Result<DimensionPlan, Diagnostics>;
    pub fn commit_dimension_generation(&self, plan: DimensionPlan) -> Result<DimensionTransaction, Diagnostics>;
    pub fn rollback_dimension_generation(&self, tx: DimensionTransaction) -> Result<(), Diagnostics>;
}
```

### 10.2 Provider capabilities

建议形状：

```rust
bitflags::bitflags! {
    pub struct SourceOps: u32 {
        const LOAD = 1 << 0;
        const EDIT_FIELD = 1 << 1;
        const RENAME_KEY = 1 << 2;
        const INSERT_RECORD = 1 << 3;
        const DELETE_RECORD = 1 << 4;
        const REWRITE_REFS = 1 << 5;
        const CREATE_TABLE = 1 << 6;
        const SYNC_HEADER = 1 << 7;
    }
}
```

要求：

- capability 是 per source 的，不是只看 provider id。
- provider 可以返回“不支持”的结构化原因。
- host 不根据 bool 猜测复杂约束，最终仍以 plan/preflight diagnostics 为准。

## 11. Testing strategy

### 11.1 Conformance tests

必须新增：

- Schema conformance：同一 schema 在 checker/data-model/codegen query 中结果一致；LSP 后续阶段纳入。
- Value conformance：CFD value、table cell value、write value validation 使用同一语义。
- Export/codegen conformance：保持现有 JSON/MessagePack/C# 输出语义，生成 C# 能消费现有导出 fixture。
- Provider write conformance：field edit/insert/delete/rename/rewrite refs 在 CFD/CSV/Excel 的行为矩阵；Lark 后续阶段纳入。
- Dimension transaction conformance：更新 default、增删记录、增删列、保留玩家翻译、失败回滚。

### 11.2 Boundary tests

必须新增：

- runtime crate dependency test：`cargo metadata` 检查 runtime 不依赖 concrete provider。
- low-level read-only session purity test：`build_data_session` 不创建/修改文件。
- high-level build rollback test：维度写入后任一后续阶段失败会恢复旧文件。
- config purity test：parse config 不读取 env，且不支持 `${VAR}` 展开。
- rejected source mapping test：duplicate/rejected record 能映射回 source。

### 11.3 Golden tests

必须覆盖：

- CFT parser/compiler。
- CFD parser/writer roundtrip。
- table cell parse/render。
- Lark API fake sequences 后续阶段补齐。
- exporter golden outputs。
- C# codegen output。

## 12. Risk and mitigation

主要风险：

1. 重构面大，容易改变用户语义。
2. provider trait 改动会触发所有 provider 重写。
3. `CftSchemaView`/query 设计不好会成为新的上帝对象。
4. build transaction 设计不好会导致回滚不完整。
5. 误改导出格式会影响消费端。

缓解：

- 先写 conformance/golden tests，再迁移。
- 每个阶段结束删除旧路径，不长期双轨。
- `CftSchemaView`/query 只读、窄接口、按 use case 分 trait。
- dimension transaction commit 前校验 hash，失败时恢复旧内容。
- 用 golden tests 锁住现有导出格式，任何格式变化必须显式评审。

## 13. Stop conditions

如果出现以下情况，应暂停继续扩大重构：

- 任何阶段无法通过 workspace `cargo test`。
- 新旧 pipeline 输出的 diagnostics 数量或位置出现未解释差异。
- provider write golden tests 出现格式破坏。
- runtime 重新引入 concrete provider 依赖。
- `CftSchemaView`/query 开始暴露过多 mutable 或 provider-specific API。

## 14. Final target state

重构完成后的理想状态：

- low-level session build 是只读纯 pipeline。
- high-level build 是显式复合 pipeline，包含 dimension transaction、reload、check、export/codegen、artifact commit。
- dimension generation 是 high-level build 的显式事务步骤，并支持失败回滚。
- runtime 不知道 CSV/Excel/CFD/Lark 的格式细节。
- schema/type/value 语义只有一套权威实现。
- 所有 provider 能力通过 operation contract 暴露。
- rejected/duplicate/invalid source records 都可诊断定位。
- LSP 后续阶段使用同一 schema query，不复制语义。
- exporter/codegen 保持现有外部输出语义，只清理内部结构并补 golden tests。
- 没有千行级核心模块，复杂逻辑都能单独测试。
