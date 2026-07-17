# Coflow crates architecture analysis

本报告只依据当前仓库源码、`Cargo.toml` / `cargo metadata`、`crates/` 下的模块结构和实现细节整理；没有使用站点参考文档作为事实来源。目标是重新核对现状，确认上一轮指出的问题哪些已经修复、哪些仍成立，以及当前代码里是否出现新的结构性风险。

## 1. Source audit scope

当前 `crates/` 下共有 18 个 crate：

| Crate | 源码文件数 | 源码行数 | 当前角色概述 |
|---|---:|---:|---|
| `coflow-api` | 13 | 1261 | provider trait、诊断、artifact、source/write/table/dimension/export/codegen 契约、registry |
| `coflow-builtins` | 1 | 54 | 注册内置 source provider、writer、table/dimension manager、exporter、codegen |
| `coflow-cfd` | 4 | 609 | CFD 文本 AST 与 parser |
| `coflow-cft` | 30 | 6448 | CFT lexer/parser/schema compiler/type checker/schema view |
| `coflow-checker` | 20 | 3325 | check 表达式运行、内置函数、依赖收集、错误解释 |
| `coflow-codegen-csharp` | 13 | 3525 | C# IR、命名、渲染、代码生成 provider |
| `coflow-data-model` | 21 | 3916 | 输入记录编译、数据模型、索引、origin、值语义 |
| `coflow-runtime` | 28 | 6087 | project runtime、load/build/check、索引、读写、mutation、维度生成、文件操作 |
| `coflow-exporter-core` | 1 | 355 | 格式无关导出遍历与编码接口 |
| `coflow-exporter-json` | 1 | 128 | JSON exporter provider |
| `coflow-exporter-messagepack` | 1 | 156 | MessagePack exporter provider |
| `coflow-loader-cfd` | 12 | 2836 | CFD loader、parser lowering 与 CFD writer/table/dimension manager |
| `coflow-loader-csv` | 9 | 1542 | CSV source provider、writer、table/dimension manager |
| `coflow-loader-excel` | 6 | 1650 | Excel source provider、writer、table/dimension manager |
| `coflow-loader-lark` | 10 | 2329 | Lark source provider、writer、remote table operations、HTTP/cache/DTO |
| `coflow-loader-table-core` | 18 | 2781 | 表格抽象、列映射、cell 解析、table writer 公共逻辑 |
| `coflow-lsp` | 20 | 5713 | CFT/CFD LSP stdio server、state、diagnostics、completion/hover/definition/tokens |
| `coflow-project` | 6 | 1301 | project config、路径、schema discovery/build helpers、diagnostic bridge |

测试文件未计入源码行数。和上一轮分析相比，`coflow-api`、`coflow-cft`、`coflow-checker`、`coflow-data-model`、`coflow-runtime`、`coflow-loader-lark`、`coflow-loader-cfd`、`coflow-lsp`、`coflow-project` 都已经明显拆分；“大量千行级单文件”这个旧问题已经不再准确。当前仍有若干 400-600 行左右的热点文件，但风险已经从“单文件巨石”转为“语义边界和跨 crate 重复仍未统一”。

## 2. Coarse-grained architecture

### 2.1 Actual dependency layers

`cargo metadata --no-deps` 显示当前内部依赖关系如下：

| Crate | 内部依赖 |
|---|---|
| `coflow-api` | `coflow-cft`, `coflow-data-model` |
| `coflow-cft` | 无 |
| `coflow-data-model` | `coflow-cft` |
| `coflow-cfd` | `coflow-cft` |
| `coflow-project` | `coflow-api`, `coflow-cft` |
| `coflow-checker` | `coflow-cft`, `coflow-data-model`, `coflow-project` |
| `coflow-loader-table-core` | `coflow-cft`, `coflow-data-model` |
| `coflow-loader-cfd` | `coflow-api`, `coflow-cfd`, `coflow-cft`, `coflow-data-model` |
| `coflow-loader-csv` | `coflow-api`, `coflow-cft`, `coflow-data-model`, `coflow-loader-table-core` |
| `coflow-loader-excel` | `coflow-api`, `coflow-cft`, `coflow-data-model`, `coflow-loader-table-core` |
| `coflow-loader-lark` | `coflow-api`, `coflow-data-model`, `coflow-loader-table-core` |
| `coflow-exporter-core` | `coflow-cft`, `coflow-data-model` |
| `coflow-exporter-json` | `coflow-api`, `coflow-exporter-core`, dev `coflow-data-model` |
| `coflow-exporter-messagepack` | `coflow-api`, `coflow-exporter-core`, dev `coflow-data-model` |
| `coflow-codegen-csharp` | `coflow-api`, `coflow-cft` |
| `coflow-builtins` | `coflow-api`, providers, exporters, codegen |
| `coflow-runtime` | `coflow-api`, `coflow-cfd`, `coflow-cft`, `coflow-checker`, `coflow-data-model`, `coflow-project`, dev `coflow-builtins`, dev `coflow-loader-csv` |
| `coflow-lsp` | `coflow-api`, `coflow-cfd`, `coflow-cft`, `coflow-project` |

粗粒度上可以看出五层：

1. 语言层：`coflow-cft`, `coflow-cfd`。
2. 数据语义层：`coflow-data-model`, `coflow-checker`。
3. provider 契约与实现层：`coflow-api`, loader/exporter/codegen crates, `coflow-builtins`。
4. project/runtime 层：`coflow-project`, `coflow-runtime`。
5. host 层：根 CLI、editor backend、`coflow-lsp`。

当前分层比上一轮更清楚：原 `coflow-engine` 实体已经改名为 `coflow-runtime`，删除了只做 re-export 的薄 facade crate。`coflow-runtime` 的生产依赖不直接依赖 CSV/CFD/Excel/Lark 具体 provider crates，文件创建、表头同步、维度源生成已经通过 `TableManager` / `DimensionSourceManager` 走 registry。CLI/editor 直接依赖名字更准确的 runtime 实现 crate。

仍然存在的偏移：

- `coflow-api` 仍直接依赖 `coflow-cft` 和 `coflow-data-model`，并 re-export 大量 schema/model 类型。它是 provider API，但不是轻量 ABI 或纯 DTO 层。
- `coflow-checker` 仍依赖 `coflow-project`。check 语义本身主要需要 schema/data model，当前依赖说明 checker 仍混入 project/维度配置语义。
- `coflow-runtime` 现在是完整实现 crate，不再是 facade；它的 public surface 仍较宽，尚未区分 build/read-only/write 三类更窄入口。
- `coflow-lsp` 仍绕过 `coflow-runtime` 的 session，自己组合 project、CFT、CFD 和诊断能力；这对轻量 schema LSP 合理，但会让未来 data-aware LSP 功能重复 runtime 逻辑。

### 2.2 Actual runtime flow

当前核心路径是：

1. `coflow-project` 读取 `coflow.yaml`，解析 `schema`、`sources`、`outputs`、`dimensions`，解析路径和 schema 文件。
2. `coflow-cft` 对 schema 文件 lex/parse，再 compile 到 `CftContainer`。
3. `coflow-runtime::build_project_session_for_build` 创建 schema session。
4. runtime 通过 registry 选择 source provider，加载 sources 为 `CfdInputRecord`。
5. 如果 schema 有 dimension fields，普通 `build_project_session_for_build` 会在基础数据 load 后调用 `dimensions::regenerate_dimension_sources`，再带隐式维度 sources 重新 load 一次数据；`open_project_session_read_only` 则跳过生成但仍会进行维度相关 reload。
6. `coflow-data-model` 编译输入记录，构建 table/index/ref/spread 图。
7. `coflow-checker` 执行 schema 中的 check 表达式，并返回 diagnostics 与依赖图。
8. runtime 聚合 diagnostics/source/record/file/dependency index，供 CLI/editor 读写。
9. exporter/codegen 根据 project outputs 提交 artifact。

和上一轮相比，维度生成已经有 transaction rollback：如果生成后 pipeline diagnostics 非空，会通过 `DimensionGenerationTransaction::rollback` 回滚已写文件。这个修复降低了半状态风险。但普通 `build_project_session_for_build` 仍是带写副作用的复合构建入口，这在用户确认“build 本身就是复合命令、维度 build 需要参与普通 build”之后不再是 CLI 行为 bug；它仍是 API 语义风险，因为只读 host 必须明确调用 `open_project_session_read_only`。

### 2.3 Fixed or no-longer-valid previous findings

| 旧问题 | 当前结论 | 依据 |
|---|---|---|
| engine 直接依赖具体 local provider 实现 | 已修复为生产依赖层面不成立 | `coflow-runtime` 生产依赖不含 loader provider crates；只保留 dev 依赖用于测试 |
| `data_files.rs` 硬编码 CSV/CFD/Excel writer 操作 | 已修复 | `create_data_file` / `sync_data_header` 走 registry 的 `TableManager`；provider 推断也改为 `TableManagerDescriptor` 的 extension/alias |
| header sync 没有 provider operation | 已修复 | `coflow-api::operations::TableManager::sync_header` |
| 维度生成没有回滚 | 已修复 | `DimensionGenerationTransaction` 和 failed pipeline rollback 已存在 |
| 环境变量展开影响 config 纯度 | 已修复 | `coflow-project/src` 搜索不到 `expand_env`、`std::env`、`${...}` 展开逻辑 |
| `coflow-api` 是 1000 行单文件 | 已修复 | 已拆为 artifacts/codegen/data_output/diagnostics/operations/provider/registry/writer 等模块 |
| `coflow-loader-lark` 单文件包含全部远程逻辑 | 已修复为“不再单文件”，但职责仍集中 | 已拆 load/write/write_http/source/writer_cache/dto/http/diagnostics |
| `coflow-checker::evaluator` 2000+ 行 | 已修复 | evaluator 约 454 行，表达式、字段、ops、builtins、deps、diagnostics 等已拆分 |
| `coflow-project` 单文件 1200+ 行 | 部分修复 | 已拆出 config/diagnostics/path/schema，但 `lib.rs` 仍约 342 行并承担入口编排 |
| host 直接依赖 `coflow-engine` | 已修复 | `coflow-engine` 已移除，根 CLI/editor 直接依赖 `coflow-runtime` |

### 2.4 Current architectural advantages

- crate 轮廓已经比较清晰，语言、数据模型、provider、runtime、host 不再混在一个 crate。
- runtime 生产依赖已经从具体 provider crate 下沉到 provider registry；新增 provider 对 runtime 的侵入性明显降低。
- `TableManager`、`DimensionSourceManager` 把 create/sync/dimension source 这类跨 provider 操作从 runtime 具体实现中抽出。
- 维度生成遵守“只更新 default 与记录/列增删，不能覆盖玩家翻译”的方向，并有 rollback 测试覆盖。
- `coflow-runtime` 名称现在直接对应 runtime 实现，删除了无实际边界价值的 facade crate。
- `coflow-api`、`coflow-checker`、`coflow-cft`、`coflow-data-model`、`coflow-loader-lark` 等热点 crate 已做中等粒度拆分，review 和局部测试成本下降。
- `coflow-loader-table-core` 继续承载 CSV/Excel/Lark 共享表格逻辑，`coflow-exporter-core` 继续承载 JSON/MessagePack 共享导出遍历。
- diagnostics 从 API 到 runtime/LSP 保持统一表达，用户可恢复错误大多进入 diagnostic flow 而不是 panic。

### 2.5 Current architectural disadvantages and risks

#### High severity

1. **普通 session build 仍有写副作用，但 public 入口已经显式。**
   `build_project_session_for_build` 默认 `DimensionBuildMode::Generate`，会写入 generated dimension sources，再 reload；`open_project_session_read_only` 才是无写入口。用户已经确认普通 build 包含维度 build 是合理的，所以这不是 CLI 需求偏差。上一轮“名称没有显式表达”的问题已修复；剩余风险是第三方 host 仍可直接调用较宽的 runtime API，需要继续通过 public surface 收窄和测试约束防止误用。

2. **schema field 查询已进一步统一，但完整语义 facade 仍未完成。**
   `CftSchemaView` 现在提供 `fields` / `fields_slice` / `field_count` / `has_dimension_fields` 等查询，runtime、exporter-core、loader-cfd、loader-table-core、C# codegen 的核心路径已避免直接依赖 `CftTypeMeta::all_fields`。data-model 和 C# codegen 也不再定义本地 `TypeMeta`/`FieldMeta` wrapper。剩余问题更窄：checker runtime、LSP 语义功能、部分 codegen 命名上下文仍有自己的语义组织方式，尚未形成一个跨所有消费者的完整 schema/type/value facade。

3. **`coflow-api` 仍是宽接口层，不是低耦合 provider ABI。**
   API 模块已经拆分，但 provider trait 仍直接暴露 `CftContainer`、`CfdDataModel`、`CfdValue`、`RecordOrigin` 等内部模型。这样 provider 与语言/数据模型版本强绑定。当前作为同仓库 crate 可以接受，但如果要长期支持独立 provider 或稳定扩展点，仍会成为主要变更热点。

4. **维度生成和 load pipeline 强耦合，失败语义比普通 load 复杂。**
   transaction rollback 修复了明显半状态问题，但流程仍是“base load -> generate files -> reload with implicit sources -> possibly rollback”。这使 diagnostics 顺序、file index、record index、增量构建和并发构建都比普通 load 复杂。当前测试覆盖了 rollback，但仍需要持续防止后续 write manager 引入不可回滚 side effect。

5. **LSP 仍然是独立 runtime，未来 data-aware 功能会重复 runtime。**
   LSP 已拆出 completion/definition/hover/semantic_tokens/state/protocol 等模块，但当前依赖仍是 `coflow-api`、`coflow-cfd`、`coflow-cft`、`coflow-project`，没有复用 `coflow-runtime` session。只做 schema/CFD 语法功能时问题不大；一旦要做数据引用跳转、真实 record diagnostics、write preview，就会重新实现 runtime 的 source/record/file index。

#### Medium severity

6. **`coflow-runtime` public surface 仍然偏宽。**
   现在已经删除薄 facade，`coflow-runtime` 是实际实现 crate。但它仍暴露大部分 runtime 内部报告、索引、mutation、write 类型，没有表达 read-only/build/write 三类更窄 session 入口的差异。短期命名已经准确，长期需要继续收窄 public API。

7. **runtime 已 provider-neutral，table/dimension 特性继续下沉到 provider capability。**
   `data_files.rs::resolve_provider_id` 已根据 `registry.table_manager_descriptors()` 读取 `TableManagerDescriptor::file_extensions` / `aliases`，并用 `TableAddressing` 区分 document/sheet table layout，不再用 `provider_id != "cfd"`。维度 source options 也改为 `DimensionSourceManager::source_options`，不再在 runtime 硬编码 CSV `sheets` options。剩余风险是同一 provider 的 source provider、writer、table manager、dimension source manager 四组 descriptor/capability 仍需保持一致。

8. **Table/dimension manager 与 writer 是平行接口，能力模型需要持续校准。**
   `SourceWriter` 处理 record/field 写入，`TableManager` 处理 create/sync header，`DimensionSourceManager` 处理 generated dimension source。拆分比单个巨型 writer 更清晰，但同一 provider 往往要实现三组接口；capability、diagnostic code、source option parsing、cache invalidation 需要保持一致。

9. **rejected/duplicate source rows 已进入 runtime record index。**
   runtime baseline 确认 duplicate record diagnostic 能保留 source file span、logical record location，并进入 diagnostics file/record index。`RecordIndex` 现在还保留 model-build 失败时的 pending source rows，提供 `RejectedRecordRef`、`rejected_in_file`、`rejected_by_coordinate` 查询。旧“duplicate/rejected 输入只能通过 diagnostics 间接观察”的判断已修复。剩余问题是 UI/CLI 是否要把 rejected rows 暴露到普通 data list 视图，这是产品层展示选择。

10. **checker 运行期数值策略已有 baseline，但仍需文档化为语言语义。**
    checker 已有测试固定 empty int sum、float infinity 传播，以及 NaN 比较必须报 `CheckEvalTypeError` 而不是降级成 false comparison。残留风险从“行为未确认”变为“语言参考与跨消费者语义仍需同步”。

11. **远程 provider 鲁棒性仍有集中风险。**
    Lark 已拆模块，但 remote API contract、retry/backoff、分页、权限错误分类、fake client 与真实 client method contract 仍是高维护区。当前拆分降低了文件复杂度，不等于远程协议边界已经稳定。

12. **project config 的 provider options 仍是弱类型 `serde_json::Value`。**
    `SourceConfig` / `OutputConfig` 把 provider options 延迟到 provider 解析。好处是扩展灵活，代价是错误更晚暴露，配置 schema 级别无法统一提供 option autocomplete、preflight、兼容诊断。

#### Low severity / maintainability

13. `coflow-builtins` 注册顺序和 provider profile 仍是隐含策略；现在全部内置 provider 默认注册，没有 local-only/no-network profile。
14. exporter/codegen 没有新增不必要的 version/manifest contract，这是符合当前需求的；但现有 JSON/MessagePack/C# 端到端 golden 仍是回归保障重点。
15. CFD parser 与 CFD writer 分布在不同 crate，roundtrip invariant 需要靠跨 crate 测试维持。
16. 许多 public struct 字段仍直接暴露，后续想增加 invariant 或 lazy cache 会更困难。

## 3. Medium-grained crate analysis

### 3.1 `coflow-api`

文件结构：

- `artifacts.rs`：artifact set/file/content。
- `diagnostics.rs`：diagnostic、label、source location、flat view、origin mapping。
- `provider.rs`：source provider、resolved source、probe/load context。
- `writer.rs`：source writer 与 record/field 写入契约。
- `operations.rs`：table manager、dimension source manager。
- `data_output.rs`：exporter 契约。
- `codegen.rs`：codegen 契约。
- `registry.rs` 及子模块：provider registry、注册/选择错误。
- `lib.rs`：模块导出与 CFT/data-model 类型 re-export。

核心类型：

- 诊断：`DiagnosticSet`, `Diagnostic`, `Severity`, `Label`, `SourceLocation`, `FlatDiagnostic`。
- artifact：`ArtifactSet`, `ArtifactFile`, `ArtifactContent`, `ArtifactContentKind`。
- source/provider：`SourceLocationSpec`, `SourceResolveContext`, `ResolvedSource`, `SourceProvider`, `SourceProviderDescriptor`, `ProbeResult`, `ProbeConfidence`, `LoadedSource`。
- writer：`SourceWriter`, `WriterDescriptor`, `WriterCapabilities`, `WriteCellRequest`, `InsertRecordRequest`, `DeleteRecordRequest`, `RenameRecordRequest`, `RewriteRecordReferencesRequest`, `WriteOutcome`。
- operations：`TableManager`, `TableManagerDescriptor`, `CreateTableRequest`, `SyncHeaderRequest`, `DimensionSourceManager`, `DimensionSourceRequest`, `DimensionSourceEntry`。
- output/codegen：`DataExporter`, `OutputSpec`, `CodeGenerator`。
- registry：`ProviderRegistry`, `ProviderRegistrationError`, `SourceProviderSelectionError`。

核心入口：

- `ProviderRegistry::register_source_provider/register_source_writer/register_table_manager/register_dimension_source_manager/register_exporter/register_codegen`。
- `ProviderRegistry::select_source_provider/source_provider/source_writer/table_manager/dimension_source_manager/exporter/codegen`。
- `SourceProvider::probe/preflight/load`。
- `SourceWriter::write_field/insert_record/rename_record/delete_record/rewrite_record_references`。
- `TableManager::create_table/sync_header`。
- `DimensionSourceManager::sync_dimension_source`。
- `DataExporter::export`, `CodeGenerator::generate`。

当前问题：

- 已从单文件拆分，旧“API 单文件巨石”不成立。
- 仍然 re-export `CftContainer`、`CfdDataModel`、`CfdValue` 等内部模型，契约层和内部模型耦合仍成立。
- `SourceWriter`、`TableManager`、`DimensionSourceManager` 三套接口的 capability/错误码/缓存失效策略需要 provider 自觉保持一致。

### 3.2 `coflow-builtins`

现状：

- `default_provider_registry` 创建 registry。
- `register_default_providers` 注册 Excel、CSV、Lark、CFD source provider/writer/table/dimension manager，JSON/MessagePack exporters，C# codegen。

当前问题：

- 角色清楚，体量很小。
- 注册 profile 仍单一；如果 CLI/editor/LSP 需要 no-network 或 local-only provider set，需要新增显式 profile。
- 注册顺序对 provider selection 的影响需要继续通过 registry 策略和测试约束。

### 3.3 `coflow-cfd`

文件结构：

- `ast.rs`：CFD AST。
- `parser.rs` 与 `parser/tokens.rs`：parser/token 支撑。
- `lib.rs`：公开 parser 和诊断。

核心类型：

- `CfdAst`, `CfdRecord`, `CfdField`, `CfdValue`, `CfdBlock`, `CfdBlockEntry`, `CfdRef`。
- `CfdSyntaxDiagnostic`。

核心入口：

- `parse_cfd` / `parser::parse`。
- `CfdValue::span`。

当前问题：

- parser/AST 边界清楚。
- writer 在 `coflow-loader-cfd`，格式 roundtrip 要靠跨 crate 测试维持。
- LSP/editor 所需的错误恢复能力仍需要随功能增长评估。

### 3.4 `coflow-cft`

文件结构：

- lexer/parser 已拆成 `lexer/tokens.rs`、`parser/annotations.rs`、`parser/check*.rs`、`parser/defaults.rs`、`parser/definitions.rs`、`parser/literals.rs`、`parser/tokens.rs`。
- schema compiler 已拆成 `schema/compiler/annotations.rs`、`build.rs`、`defaults.rs`、`symbols.rs`、`types.rs`。
- type checker 已拆出 `schema/type_checker/functions.rs` 和 `ops.rs`。
- schema view 已拆出 `schema_view/queries.rs`、`dimension_checks.rs`。

核心类型：

- AST：`ModuleAst`, `Item`, `ConstDef`, `EnumDef`, `TypeDef`, `FieldDef`, `TypeRef`, `DefaultExpr`, `CheckBlock`, `CheckStmt`, `CheckExpr`。
- schema：`CftSchemaModule`, `CftSchemaConst`, `CftSchemaType`, `CftSchemaField`, `CftSchemaTypeRef`, `CftSchemaDefaultValue`, `CftSchemaCheckBlock`, `CftSchemaEnum`, `CftAnnotation`。
- container/view：`ModuleId`, `CftContainer`, `CftSchemaView`, `CftTypeMeta`, `CftFieldMeta`, `CftEnumMeta`, `CftDimensionFieldMeta`。
- diagnostics：`CftDiagnostics`, `CftDiagnostic`, `CftErrorCode`, `CftLabel`, `CftStage`。

核心入口：

- `lex`, `parse_module`。
- `CftContainer::add_module`, `compile`, `register_runtime_type`。
- `CftContainer::resolve_type/resolve_enum/resolve_const/all_types/all_enums/is_assignable/range_is_polymorphic`。
- `CftSchemaView::new`, `type_meta`, `field_type`, `is_assignable`, `checks_for_actual`, `dimension_field`。

当前问题：

- 旧“parser/compiler/type checker 都是大单文件”已经明显缓解。
- `CftSchemaView` 比上一轮更核心，data-model/codegen 已改为编译上下文命名并直接复用 CFT meta；runtime/checker/LSP 仍存在局部解释。
- schema compiler/type checker 和 checker runtime 的表达式规则仍需要更强一致性测试。

### 3.5 `coflow-data-model`

文件结构：

- `ingest/` 保存来源无关的 loaded draft IR。
- `build/` 负责 schema 驱动的验证、默认值与 spread materialization。
- `model/` 只保存成功状态、typed identity、values 和 relation edges。
- `indexes/` 保存 record/ref/spread 查询结果，`semantics/` 保存共享值语义。
- `dependencies/` 与 `diagnostics/` 分别保存 materialization 依赖和诊断映射。

核心类型：

- model：`CfdDataModel`, `CfdModelBuilder`, `CfdTable`, `CfdRecord`, `CfdObject`, `CfdRecordId`, `RecordCoordinate`。
- values：`CfdValue`, `CfdDictKey`, `CfdEnumValue`, `LoadedRecordDraft`, `LoadedValueDraft`, `LoadedDictKeyDraft`。
- graph：`RefSite`, `RefEdge`, `RefEdgeId`, `SpreadSite`, `SpreadEdge`, `SpreadEdgeId`。
- origin：`RecordOrigin`, `SourceDocument`, `TextSpan`, `SourceLocation`, `MappedDiagnostic`。
- diagnostics：`CfdDiagnostics`, `CfdDiagnostic`, `CfdErrorCode`, `CfdPath`, `CfdPathSegment`。
- semantics：`CfdValueSemanticContext`, `PendingInsertRef`, `CfdValueSemanticError`。

核心入口：

- `CfdDataModel::builder` and `CfdModelBuilder::add_loaded_record/build`。
- `CfdDataModel::record/table/records/tables/record_by_type_key/record_by_domain_key/lookup_assignable`。
- `CfdDataModel::direct_ref_edges/spread_edges/resolve_direct_ref/resolve_effective_ref/spread_source_at_path`。
- `CfdDataModel::dimension_field_value`。
- `validate_value_for_schema`, `validate_object_type_assignable`。
- `RecordOrigin::location_for_path`, `map_diagnostics`。

当前问题：

- 旧“compiler/model 都过千行”已修复。
- `compiler_context.rs` 是 data-model 编译期上下文，包含 CFT view、dimension storage 索引和 domain 索引；它不再定义本地 type/field wrapper，但仍和 checker/runtime/LSP 的部分 schema 解释没有完全统一。
- `CfdRecordId` 的 session 内稳定性/跨 reload 稳定性仍需要作为 contract 明确。
- rejected/duplicate records 的 runtime 侧 source mapping 仍需要继续改进。

### 3.6 `coflow-checker`

文件结构：

- `check/evaluator.rs` 保留 evaluator 中枢。
- 已拆出 `access.rs`、`builtin_calls.rs`、`builtin_values.rs`、`deps.rs`、`diagnostics.rs`、`dimensions.rs`、`enum_values.rs`、`explanations.rs`、`expressions.rs`、`fields.rs`、`ops.rs`、`quantifiers.rs`、`runner.rs`、`statements.rs`、`type_predicates.rs`、`value.rs`。

核心类型：

- public：`DependencyGraph`, `CfdCheckExt`。
- runtime：`CheckEvaluator`, `CheckValue`, `LocatedCheckValue`, `CheckRecordRef`, `CheckEntry`, `CheckExplanation`, `EvalAbort`。
- builtin：`Builtin`。

核心入口：

- `run_checks`, `run_checks_for`, `run_checks_with_deps`。
- `run_checks_for_dimensions`, `run_checks_for_dimensions_with_deps`。
- `DependencyGraph::affected_by`。
- `CfdCheckExt::check/check_with_deps`。

当前问题：

- 旧“evaluator 过大”已大幅缓解。
- checker 仍依赖 `coflow-project`。
- 运行期 value/type 规则与 CFT type checker 的统一仍不彻底。
- float 非有限值、除零、power 等数值语义需要明确测试基线。

### 3.7 `coflow-project`

文件结构：

- `config.rs`：project config serde。
- `diagnostics.rs`：project diagnostic。
- `path.rs`：路径转换/规范化。
- `schema.rs`：schema discovery/build helpers。
- `lib.rs`：project open/init/validation 入口。

核心类型：

- config：`ProjectConfig`, `SchemaConfig`, `SourceConfig`, `OutputsConfig`, `OutputConfig`, `DimensionConfig`。
- project：`Project`, `SchemaFile`, `SchemaBuild`, `SchemaSourceOverride`, `InitOutcome`。
- diagnostics/path：`ProjectDiagnostic`, `Range`, `Position`。

核心入口：

- `Project::open`, `Project::open_schema_only`。
- `Project::validate_for_data`, `validate_for_codegen`。
- `Project::schema_diagnostic_set/data_diagnostic_set/codegen_diagnostic_set`。
- `Project::resolve_path`, `schema_files`。
- `init_project`。
- `compile_schema_project`, `compile_schema_project_with_overrides`。
- `path_to_slash`, `normalize_path`。

当前问题：

- 环境变量展开问题已不成立。
- config/path/schema/diagnostic 已拆分，但 provider options 仍是弱类型 JSON。
- `coflow-project` 依赖 `coflow-api` 主要为了 source location 和 diagnostics，这个依赖方向可接受但让 project 层不再完全独立。

### 3.8 `coflow-runtime`

文件结构：

- session/build/load/index：`session.rs`, `session_build.rs`, `schema_build.rs`, `load.rs`, `indexes.rs`。
- reads/reports：`data_read.rs`, `schema_inspect.rs`, `files.rs`, `records.rs`。
- writes：`writes.rs`, `writes/path.rs`, `writes/refs.rs`, `writes/target.rs`, `writes/writer.rs`, `write_rules.rs`。
- mutations：`mutation/mod.rs`, `apply.rs`, `prepare.rs`, `coercion.rs`, `defaults.rs`, `types.rs`, `data_patch.rs`。
- dimensions：`dimensions/synthesize.rs`, `regenerate.rs`, `info.rs`。
- data file ops：`data_files.rs`。

核心类型：

- session/index：`ProjectSession`, `ProjectSchemaSession`, `RecordCoordinate`, `DiagnosticsStore`, `SourceIndex`, `ResolvedSourceEntry`, `SourceId`, `RecordIndex`, `RecordRef`, `FileIndex`。
- read/schema reports：`DataSourcesReport`, `DataListQuery`, `DataListReport`, `DataGetQuery`, `DataGetReport`, `SchemaInspectReport`, `SchemaFilesReport`, `SchemaTypeInfo`, `SchemaFieldInfo`。
- writes/mutation：`RecordView`, `RecordTarget`, runtime `WriteOutcome`, `MutationRequest`, `MutationOp`, `MutationValue`, `PreparedMutation`, `MutationReport`, `DataPatchRequest`, `DataPatchOp`。
- data files/dimensions：`DataCreateFileOptions`, `DataSyncHeaderOptions`, `DataFileReport`, `DimensionInfo`, `DimensionField`, `DimensionFieldInfo`, `DimensionGenerationTransaction`。

核心入口：

- `build_project_session_for_build`, `open_project_session_read_only`, `build_project_schema_session`, `configured_project_source`。
- `ProjectSession::record_view/record_views_in_file/file_tree/dimensions/id_for_coordinate/coordinate_of`。
- `ProjectSession::write_field/rename_record_key/insert_record/delete_record`。
- `ProjectSession::prepare_mutation/apply_prepared_mutation/apply_mutation/apply_data_patch/default_record_value`。
- `data_sources`, `data_list`, `data_get`。
- `inspect_schema`, `schema_files`。
- `create_data_file`, `sync_data_header`。
- `dimensions::inject_dimension_types/dimension_sources/regenerate_dimension_sources`。

当前问题：

- runtime 已经比上一轮模块化很多，生产依赖也 provider-neutral。
- `build_project_session_for_build` 的写副作用仍需在 public API 命名或 options 层更显式表达。
- `data_files.rs` 的本地 provider 推断已改为 `TableManagerDescriptor` driven，不再硬编码 `cfd/csv/excel/xlsx` 分支，也不再借用 source provider extension。
- runtime 已移除无消费者的 checker dependency index 存储，不再把 `DependencyIndex` 作为 session/public surface 的一部分。
- runtime 内部仍大量直接创建 `CftSchemaView`，说明统一 schema facade 还未完全下沉。

### 3.9 `coflow-exporter-core`

现状：

- 单文件共享导出遍历。
- 核心类型：`ExportEncoder`, `ExportError`, internal `Exporter`, `CftSchemaView`, `CftFieldMeta`, `TypeTagMode`。
- 核心入口：`export_model_with_encoder`, `ExportEncoder::null/bool/int/float/string/array/map`。

当前问题：

- JSON/MessagePack 共享遍历是合理的。
- exporter 已直接使用 `CftSchemaView` / `CftFieldMeta`，旧 internal schema projection 问题已不成立；剩余风险主要是导出遍历和 codegen/runtime 对默认值、多态和 type tag 的解释需要靠 golden/roundtrip 测试保持一致。

### 3.10 `coflow-exporter-json` / `coflow-exporter-messagepack`

现状：

- JSON exporter 使用 `JsonEncoder` 实现 `ExportEncoder`。
- MessagePack exporter 使用 `MessagePackEncoder` 实现 `ExportEncoder`，每个 table 输出 bytes。

当前问题：

- 没有引入用户未要求的 version/export manifest contract，这是符合当前约束的。
- 仍应靠 golden/decode roundtrip 测试保证现有输出格式稳定。

### 3.11 `coflow-codegen-csharp`

文件结构：

- `ir.rs`, `model.rs`, `schema_context.rs`, `emit.rs`, `render.rs`, `names.rs`, provider entry 和 tests。

核心类型：

- public API：`GeneratedFile`, `CsharpTemplate`, `CsharpDatabaseTemplates`, `CsharpCodegenError`, `CsharpCodeGenerator`。
- options：`CsharpCodegenOptions`, `CsharpDataFormat`, `CsharpIdAsEnumVariant`, `CsharpCodegenDiagnostic`。
- IR：`CsharpProject`, `CsharpType`, `CsharpProperty`, `CsharpEnum`, `CsharpEnumVariant`, `CsharpDatabase`, `CsharpTable`, `CsharpLoader`, `CsharpPolymorphicCase`。
- schema context：`CsharpSchemaContext`，直接消费 `CftSchemaView`、`CftTypeMeta`、`CftFieldMeta`、`CftSchemaTypeRef`。

核心入口：

- `generate_csharp`, `generate_csharp_json`, `generate_csharp_messagepack`。
- `generate_csharp_with_database_templates`, `generate_csharp_with_id_as_enum_variants`。
- `build_project`, `preflight_csharp_codegen`。
- `render_project`。
- `CsharpCodeGenerator::generate`。

当前问题：

- codegen 已没有独立 `TypeMeta`/`FieldMeta` wrapper，`schema_context.rs` 主要承载 C# 命名、loadable table 和 codegen 选项上下文；它仍是局部 schema context，但不是重复 schema model。
- 需要继续用现有 fixture/golden 保证 C# 与当前 export 数据可互操作，但不应新增未要求的 export/codegen version contract。

### 3.12 `coflow-loader-cfd`

文件结构：

- loader/parser：`lib.rs`, `parser/*`。
- writer：`writer.rs`, `writer/*`。
- table/dimension operations 由 provider 对应模块实现。

核心类型：

- loader：`CfdLoader`, `CfdTextDiagnostics`, `CfdTextDiagnostic`, `CfdTextErrorCode`, `CfdTextSpan`。
- writer：`CfdWriter`, internal cache/target/layout 类型。

核心入口：

- `parse_cfd_input_records`, `load_cfd_model`。
- `CfdLoader::probe/preflight/load`。
- `CfdWriter` 的 source write 方法。
- CFD create/sync/dimension manager 对应 provider operation。

当前问题：

- loader/writer 比上一轮拆分更细。
- parser 与 writer 不在 `coflow-cfd`，roundtrip invariant 需要继续靠 loader-cfd 测试覆盖。

### 3.13 `coflow-loader-csv` / `coflow-loader-excel`

现状：

- CSV/Excel 都使用 `coflow-loader-table-core` 做 table input collection、cell parse/render、write planning。
- 各自 provider 处理文件格式读写、diagnostic mapping、source config、writer/table/dimension manager。

当前问题：

- 共享 table-core 的方向正确。
- CSV/Excel 的格式差异仍需要 provider 自己维护，尤其 header sync、delete row、insert row、dimension source preservation 的行为需要持续保持一致测试。

### 3.14 `coflow-loader-lark`

文件结构：

- `source.rs`：source config/locator。
- `load.rs`：loader 与读取流程。
- `write.rs`, `write_http.rs`, `write_layout.rs`, `writer_cache.rs`：写入、HTTP 操作、布局、缓存。
- `http.rs`：HTTP client trait/real client。
- `dto.rs`：API DTO。
- `diagnostics.rs`：诊断。

核心类型：

- source：`LarkSheetSource`, `LarkSheetLocator`。
- diagnostics：`LarkDiagnostics`, `LarkDiagnostic`。
- HTTP：`LarkHttpClient`, `UreqLarkHttpClient`, `LarkHttpMethod`。
- loader/writer：`LarkSheetLoader`, `LarkLoaderCache`, `LarkSheetWriter`, `LarkWriterCache`, `CachedToken`。
- API DTO：`AuthResponse`, `ApiEnvelope`, `WikiNodeData`, `SheetsQueryData`, `LarkSheetMetadata`, `ValuesData`, `ValueRange`。

核心入口：

- `load_lark_table_source`, `load_lark_table_source_with_client`。
- `LarkHttpClient::get/post_json/put_json/delete_json`。
- `LarkSheetLoader::new` and source provider methods。
- `LarkSheetWriter::new`, token/sheet cache helpers, writer methods。

当前问题：

- 单文件问题已不成立。
- remote API contract、retry/backoff、权限错误分类、fake client 与真实 client 行为一致性仍是主要风险。
- Lark 属远程 provider，最好保持不被 runtime/CLI 特殊化；目前 runtime 生产层已经做到这一点，CLI 边缘仍有 Lark 创建表逻辑需要单独关注。

### 3.15 `coflow-loader-table-core`

文件结构：

- `table.rs` 与 `table/*`：table source/sheet/columns/diagnostics/input-record collection/write layout。
- `cell_value/*`：cell syntax parse/render。
- `writer.rs`：provider-neutral write planning。

核心类型：

- table：`TableSource`, `TableSheet`, `TableSheetConfig`, `TableDiagnostics`, `TableInputRecords`, `TableDiagnostic`, `TableLabel`, `TableLocation`, `TableWriteLayout`。
- cell：`ParsedCell`, `CellValueDiagnostics`, `CellValueDiagnostic`, `CellValueErrorCode`, `CellRenderError`。
- write plan：`TableWritePlan`, `TableSetCell`, `TableAppendRow`, `TableDeleteRow`, `TableInsertRecord`, `TableFieldWrite`, `TableWriteDiagnostics`。

核心入口：

- `collect_table_input_records`。
- `resolve_table_write_layout`。
- `map_table_diagnostics`, `map_label_to_table`。
- `parse_cell`, `render_cell_value`。
- `plan_field_write`, `plan_insert_record`, `plan_delete_record`。

当前问题：

- 这是重复数据结构统合最明显的成功点之一。
- 但 table abstraction 会压平 CSV/Excel/Lark 的能力差异，provider-specific behavior 仍需要回到各 provider 处理。

### 3.16 `coflow-lsp`

文件结构：

- `lib.rs`：stdio server 入口和主要 dispatch。
- `state.rs`, `protocol.rs`, `position.rs`, `text.rs`, `uri.rs`, `diagnostics.rs`。
- features：`completion.rs`, `definition.rs`, `hover.rs`, `document_symbols.rs`, `formatting.rs`, `semantic_tokens.rs`, `documentation.rs`。
- CFD：`cfd/mod.rs`。
- tests：`tests/*`。

核心类型：

- server/state：`LspServer`, `OpenDocument`, `LspBuild`, `LspDocument`, `TextRequest`, `LspPosition`, `WordAt`, `CompletionScope`, `RawSemanticToken`, `CfdProjectSource`。
- diagnostics：`LspLabelLocation`。

核心入口：

- `run`。
- server methods：`handle_message`, `initialize`, `open_document`, `change_document`, `close_document`, `validate_project`, `completion`, `hover`, `definition`, `document_symbol`, `formatting`, `semantic_tokens`。
- URI/diagnostic helpers：`path_from_file_uri`, `path_to_file_uri`, `lsp_diagnostic`, `preferred_diagnostic_uri`。

当前问题：

- 已经比上一轮拆分更多，旧“LSP 单文件”不准确。
- `semantic_tokens.rs`、`completion.rs`、`cfd/mod.rs`、`lib.rs` 仍是较大热点。
- 没有复用 runtime session，未来 data-aware 功能会和 runtime 重复。

## 4. Cross-cutting issue verification

### 4.1 Build side effects and dimension rollback

当前状态：部分修复，风险残留。

- 普通 `build_project_session_for_build` 仍会生成维度源，这是用户确认的普通 build 语义。
- 已新增 `open_project_session_read_only`。
- 已有 generation transaction 和 rollback。
- 残留问题是 API 命名和 facade 没把“有写副作用的 build”和“只读 open/check session”区分得足够强。

建议优先级：高。不是为了改变 CLI 行为，而是为了防止 host/API 误用。

### 4.2 Provider-neutral runtime

当前状态：已修复核心 runtime/provider 耦合，剩余是能力一致性治理。

- runtime 生产依赖不再包含具体 provider crates。
- create/sync/dimension source 通过 registry provider operation。
- provider id/extension/alias 推断已下沉到 `TableManagerDescriptor`，`data_files.rs` 不再枚举具体 provider。
- table layout 使用 `TableAddressing`，维度 source options 由 `DimensionSourceManager::source_options` 生成，已移除 runtime 对 CFD/CSV 的行为分支。
- 残留耦合主要是 data file 命令天然面向本地文件语义，远程 table manager 不应被扩展名推断路径误用。

建议优先级：低到中。下一步不是继续移动代码，而是统一各 provider descriptor 的 capability/preflight/diagnostic code 约定，并补 alias/extension 冲突测试。

### 4.3 Duplicate data structures and semantic projections

当前状态：核心 field 查询已收敛，完整语义 facade 仍是后续项。

- 已统合：table provider 共享结构在 `coflow-loader-table-core`；API operations 统一了 table/dimension source manager；runtime mutation/writes 已拆分；常见 field 查询统一走 `CftSchemaView`。
- 未完全统合：checker runtime、LSP 语义功能、部分 codegen 上下文仍有局部语义组织；value coercion/cell parsing 仍保留各自面向输入格式的轻量投影。

建议优先级：中到高。继续用 `CftSchemaView` 扩展更窄的 schema query/type facade，并用 conformance tests 约束 checker/LSP/codegen 行为。

### 4.4 Config purity

当前状态：已修复。

- 环境变量展开逻辑已移除。
- `${VAR}` 字符串应作为普通字符串由配置/provider 自己处理。

建议优先级：低。保留测试防回归即可。

### 4.5 Record/source index completeness

当前状态：已补强。

- data model 能产生 duplicate/rejected diagnostics。
- duplicate record diagnostic 已有 runtime baseline，确认 source file span、logical record location、diagnostics file/record index 都可用。
- runtime record index 现在保留 rejected source rows，可按 file 和 logical coordinate 查询。

建议优先级：低。后续只需决定 UI/CLI 是否把 rejected rows 纳入普通 data list，而不是继续修 source identity 丢失问题。

### 4.6 Remote writer robustness

当前状态：部分改善，风险仍在。

- Lark 模块已拆分，读写/cache/HTTP/DTO 更清晰。
- retry/backoff、分页、权限错误分类、fake/real method contract 仍需要明确。

建议优先级：中。用户已把 LSP/Lark 深改列为后置，因此当前只记录，不作为马上执行项。

## 5. Prioritized optimization roadmap

### Phase 1: Consolidate current refactor gains

1. 收窄 `coflow-runtime` public API：显式区分 build-with-generation、read-only session、write session，避免 host 直接误用有副作用的入口。
2. 统一 provider descriptor capability/preflight/diagnostic code 约定，给 table manager extension/alias 冲突补测试。
3. 给 `build_project_session_for_build` 维度生成路径继续补充事务/失败路径测试，确保 provider manager 的所有写入都可 rollback 或至少失败前置。
4. rejected/duplicate source row 可追踪测试已补；后续只需决定 UI/CLI 展示策略。

### Phase 2: Reduce semantic duplication

1. 继续扩展共享 schema query/type facade，优先覆盖 checker runtime 与 LSP 语义功能的重复解释。
2. 将 CFT type checker 与 checker runtime 的 operator/builtin signature 规则对齐，用 shared table 或 conformance test 固定。
3. 固定并文档化 checker numeric semantics，包括 integer/float division、`powf`、非有限 float 的诊断策略；NaN comparison baseline 已补。
4. 建跨 crate conformance tests：schema view、cell value、dimension default preservation、export/codegen fixture。

### Phase 3: Strengthen provider contracts

1. 统一 `SourceWriter`、`TableManager`、`DimensionSourceManager` 的 capability/preflight/diagnostic code 约定。
2. 继续补 table/dimension manager descriptor metadata：支持的 location kind、remote/local、can sync header、can preserve variants。
3. 保持 JSON/MessagePack/C# 现有输出语义，只补 golden/roundtrip/interop 测试，不增加未要求的 manifest/version contract。

### Phase 4: Deferred deeper work

1. LSP data-aware runtime 复用设计。
2. Lark remote client contract、retry/backoff、权限错误分类。
3. 大规模 crate 重命名或 aggressive compatibility-breaking API cleanup。

## 6. Summary judgment

上一轮分析里的不少问题已经实质修复：runtime 不再生产依赖具体 provider，表创建/表头同步/维度源生成已经 provider-operation 化，维度生成有 rollback，环境变量展开已移除，多处千行单文件也已经拆开，原 `coflow-engine` 已重命名为 `coflow-runtime`，薄 re-export facade 已删除。

当前最重要的残留问题已经不是“代码全堆在一起”，而是三个边界还没完全收口：第一，`build_project_session_for_build` 的写副作用需要在 runtime public API 上显式化；第二，schema/type/value 解释仍在多个 crate 重复；第三，provider API 仍直接暴露内部 schema/data model，适合同仓库迭代但不适合作为长期稳定 ABI。

如果继续按“不做 LSP/Lark 深改、不新增无关导出 contract”的约束推进，下一步最有价值的是收窄 `coflow-runtime` 的 session/build/write 入口，并开始以 conformance tests 驱动统一 schema query/type facade。
