# Aggressive Coflow architecture refactor plan

本文档是基于当前源码状态重新整理的激进架构重构计划。上一版计划中的一部分判断已经被后续代码修复，本版会先剔除过期建议，再给出可以继续深入的重构路线。

约束和前提：

- 本计划允许 breaking changes：Rust public interface、CLI 内部 JSON 字段、provider trait、runtime session 类型、模块路径和测试 fixture 都可以改。
- 不主动改变 CFT/CFD 语言语义、JSON/MessagePack 输出语义、C# 生成语义，除非某个语义本身就是设计债务且有明确迁移收益。
- 未发现 `CONTEXT.md` 或 `docs/adr/`，所以没有需要显式绕开的 ADR。
- 继续遵守 `AGENTS.md` 的 crate 职责：CLI 保持 artifact staging/commit，runtime 保持 shared project runtime，provider shared algorithms 不进 `coflow-api`。
- 文档里的架构词汇沿用：module、interface、depth、seam、adapter、leverage、locality。

## 1. Current state correction

当前主要问题已经不是“大量千行文件”，而是几个 module 的 interface 仍然偏浅，导致实现知识从 seam 泄漏到 host、provider 和语义消费者。

已经修复或不再建议作为主线的事项：

| 旧判断 | 当前状态 | 计划调整 |
| --- | --- | --- |
| runtime 生产依赖具体 CSV/Excel/CFD/Lark provider | 已修复；`coflow-runtime` 生产依赖不含具体 provider crate | 保持 dependency guard，不再作为主重构 |
| `coflow-api` 是单文件巨石 | 已拆成 artifacts/codegen/data_output/diagnostics/operations/provider/registry/writer | 不做“拆文件式”重构，改为收窄 provider interface |
| create/sync header 不是 provider operation | 已有 `TableManager` | 继续统一 capability/preflight/diagnostic contract |
| 维度生成没有 rollback | 已有 `DimensionGenerationTransaction` | 改为让有副作用 build interface 更显式 |
| config parser 展开环境变量 | 已移除 | 保留回归测试即可 |
| duplicate/rejected source rows 丢失 source mapping | 已有 rejected index 相关能力 | 后续主要是 host 展示策略，不是核心架构问题 |
| 新增 `coflow-pipeline` / `coflow-editor-core` | 已被 repo hygiene 明确禁止 | 不再建议 |
| artifact staging 放进 runtime | 与当前职责不符 | 保持在 root CLI crate |

当前仍值得激进处理的高 leverage 问题：

1. `ProjectSession` interface 太浅，公开 `project/schema/model/diagnostics/sources/records/files`，host 可以直接耦合 runtime 内部结构。
2. `build_project_session_for_build` 和 `open_project_session_read_only` 已经区分写副作用，但 runtime public surface 仍没有把 read/build/write 三类 session 做成更深的 module。
3. `coflow-checker` 依赖 `coflow-project::DimensionConfig`，check module 认识了 project config。
4. `coflow-api` 仍 re-export `CftContainer`、`CfdDataModel`、`CfdValue` 等内部模型，provider seam 比理想状态宽。
5. `SourceConfig.options`、`OutputConfig.options`、`ResolvedSource.options`、`OutputSpec.options` 仍以 `serde_json::Value` 穿透多层 module。
6. `CftSchemaView` 已经统一了不少查询，但它公开 map/meta 结构，schema/type/value 语义还没有形成足够深的 facade。
7. editor backend 和 LSP 仍直接消费较多 CFT/data-model/runtime 内部类型，adapter 层不够集中。
8. LSP、Excel writer、Lark remote provider 仍是后续维护热点，但不应抢在 runtime/schema/provider seam 前面做。

## 2. Top recommendation

优先做 runtime session interface 加深。

理由：

- `ProjectSession` 当前是 shallow module：它的 interface 几乎等于内部字段集合，删除它不会集中复杂度，只会把复杂度直接移动到 CLI/editor/LSP。
- 这个 seam 的 leverage 最大：一旦 session 分成 read-only/build/write 三类更深 module，host 就无法随手绕过 runtime invariants。
- 它能保护后续重构：schema query、provider options、write transaction 都可以先藏在 runtime 的更深 interface 后面逐步替换。
- locality 会明显变好：host 只处理报告和命令，runtime 内部才处理 schema/model/index/source/write 组合。

第一批目标不是新增很多 crate，而是在现有 crate 内把 module 变深：

- `coflow-runtime` 增加更窄的 `Runtime` / session facade。
- `ProjectSession` 字段改私有，公开只读 query interface 和 command interface。
- `coflow-checker` 接受 runtime 生成的 dimension check plan，不再依赖 `coflow-project`。
- `coflow-api` 不再作为内部模型 re-export 集散地。

## 3. Target dependency shape

目标依赖方向：

```text
coflow-cft
  |
  v
coflow-data-model  <----  coflow-checker
  ^                         ^
  |                         |
coflow-api  <----------  coflow-runtime  <---- hosts: CLI / editor / LSP
  ^
  |
providers / exporters / codegen
```

更准确地说：

- `coflow-project` 只负责 project config、path、schema discovery、diagnostics。
- `coflow-checker` 不依赖 `coflow-project`。
- `coflow-runtime` 可以依赖 `coflow-project` 和 `coflow-checker`，并把 project config 转为 runtime/check plan。
- provider crates 通过 `coflow-api` 接入，但 `coflow-api` 不承载 provider shared algorithms。
- host 通过 runtime 的深 interface 访问数据，不直接拼装 schema/model/source/index。

禁止重新引入的依赖和 module：

- `coflow-runtime -> concrete provider implementation` 生产依赖。
- `coflow-checker -> coflow-project`。
- `coflow-api -> table/cell/export implementation algorithms`。
- `coflow-pipeline`、`coflow-engine`、`coflow-editor-core`。
- low-level read-only session build 写文件。
- host 绕过 runtime writer 直接改 source 文件。

## 4. Phase 0: behavior and architecture baselines

目标：先锁住行为，再破 interface。

动作：

1. 增加 architecture regression tests：
   - `coflow-checker` 不依赖 `coflow-project`。
   - `coflow-runtime` 生产依赖不包含 concrete provider crates。
   - `ProjectSession` 核心字段不再 public。
   - host 不直接访问 `session.schema` / `session.model` / `session.records`。
2. 增加 runtime session 行为 baseline：
   - read-only session 不写 generated dimension files。
   - build session 可以生成 dimension files，并在后续 diagnostics 失败时 rollback。
   - write 成功后的 reload 路径仍保持现有 observable behavior。
3. 增加 schema/value conformance fixture：
   - object assignability。
   - enum value/variant。
   - nullable/dict/ref validation。
   - table cell parse/render 与 data-model validation 一致。
4. 增加 export/codegen golden：
   - JSON golden。
   - MessagePack decode roundtrip。
   - C# generated code fixture。

验收：

- `cargo check --workspace` 通过。
- `cargo test --workspace` 通过。
- 新增测试先在旧实现上通过或明确标注当前缺口。

## 5. Phase 1: deepen runtime session module

目标：让 runtime 的 interface 表达读、构建、写入三种不同能力，而不是让所有 host 拿到同一个全字段 `ProjectSession`。

建议目标形状：

```rust
pub struct Runtime {
    registry: ProviderRegistry,
}

pub struct ProjectHandle { /* project config + root + config diagnostics */ }

pub struct SchemaSession { /* private project + schema + diagnostics */ }
pub struct ReadOnlyDataSession { /* private ProjectSession */ }
pub struct BuildDataSession { /* private ProjectSession, build effects explicit */ }

pub struct BuildEffects {
    pub generated_dimensions: Vec<GeneratedDimensionSource>,
}
```

关键动作：

1. 在 `coflow-runtime` 增加 `Runtime` facade：
   - `open_project(path) -> ProjectHandle`
   - `build_schema(&ProjectHandle) -> SchemaSession`
   - `open_read_only(&ProjectHandle) -> ReadOnlyDataSession`
   - `build_with_generated_sources(&ProjectHandle) -> BuildDataSession`
   - `plan_write(&ReadOnlyDataSession, WriteCommand) -> WritePlan`
   - `commit_write(WritePlan) -> WriteCommitResult`
2. `ProjectSession` 改为内部类型或字段全部私有：
   - `project()`, `schema()`, `model()` 这类宽 getter 不作为长期 host interface。
   - 面向 host 的数据通过 query/report 输出：`data_sources`, `data_list`, `data_get`, `schema_inspect`, `file_tree`, `diagnostics`。
3. 把 `build_project_session_for_build` 降级为 compatibility wrapper，只在迁移期间保留。
4. editor backend 改为只保存 `ReadOnlyDataSession` 或 runtime-defined snapshot，不再直接持有完整 `ProjectSession` 字段。
5. write mutation 从 `impl ProjectSession` 移到显式 write module：
   - plan 阶段只做 validation 和 source target resolve。
   - commit 阶段才执行 provider writer。
   - reload 阶段通过 runtime facade 返回新 session。

涉及文件：

- `crates/coflow-runtime/src/session.rs`
- `crates/coflow-runtime/src/session_build.rs`
- `crates/coflow-runtime/src/load.rs`
- `crates/coflow-runtime/src/writes.rs`
- `crates/coflow-runtime/src/mutation/*`
- `src/commands.rs`
- `src/data_commands.rs`
- `editors/cfd-editor/src-tauri/src/editor/session/*`

breaking changes：

- 删除或私有化 `ProjectSession` public fields。
- host 不再直接拿 `CftContainer` / `CfdDataModel` 做 UI 转换。
- `build_project_session_for_build` / `open_project_session_read_only` 可以重命名或变为 wrapper。

验收：

- read-only/build/write 的 type name 就能看出是否可能写文件。
- editor 和 CLI 没有直接访问 `session.schema` / `session.model` / `session.records` 的生产路径。
- write 成功后 session reload 行为保持现状。
- `cargo test --workspace` 通过。

## 6. Phase 2: remove checker -> project dependency

目标：check module 只认识 schema、model 和 check execution plan，不认识 project config。

当前证据：

- `crates/coflow-checker/Cargo.toml` 依赖 `coflow-project`。
- `crates/coflow-checker/src/lib.rs` 公开函数接收 `BTreeMap<String, DimensionConfig>`。
- `crates/coflow-runtime/src/load.rs` 在 runtime 中把 `project.config.dimensions` 直接传给 checker。

目标 interface：

```rust
pub struct DimensionCheckPlan {
    pub rounds: Vec<DimensionCheckRound>,
}

pub struct DimensionCheckRound {
    pub dimension: String,
    pub variant: String,
}
```

关键动作：

1. 在 `coflow-checker` 定义 `DimensionCheckPlan`，或更窄地定义 `IntoIterator<Item = DimensionCheckRound>` 参数。
2. runtime 从 `ProjectConfig.dimensions` 生成 check plan。
3. checker tests 不再构造 `coflow_project::DimensionConfig`。
4. 删除 `coflow-checker` 对 `coflow-project` 的依赖。

leverage：

- check module 的 seam 更清晰，未来 project config 变化不会波及 checker。
- checker 可在没有 project crate 的独立 fixture 中测试，locality 更好。

验收：

- `cargo metadata` 显示 `coflow-checker` 内部依赖只有 `coflow-cft`、`coflow-data-model`。
- checker dimension tests 仍覆盖 default round 和每个 variant round。

## 7. Phase 3: deepen schema/type/value semantic module

目标：让 schema/type/value 语义由一个深 module 提供，而不是由各消费者拿 `CftSchemaView` 的 public maps 自己解释。

当前状态：

- `CftSchemaView` 已经提供 `field_type`、`fields_slice`、`is_assignable`、`concrete_assignable_types` 等查询。
- 但 `CftSchemaView` 暴露 `consts/types/enums` public maps，`CftTypeMeta` 也暴露很多 projection。
- `coflow-data-model::value_semantics`、table cell parser、runtime mutation、checker ops、codegen context 都仍以自己的用例组织语义。

目标 module：

```rust
pub struct SchemaQuery<'a> { /* private */ }
pub struct TypeSystem<'a> { /* private */ }
pub struct ValueTypeRules<'a> { /* private */ }

pub enum FieldLookup<'a> {
    Found(FieldRef<'a>),
    UnknownField,
    UnknownType,
}
```

关键设计：

- `CftSchemaView` 保留名字也可以，但内部 map 和 meta 字段私有。
- 引入 typed IDs 或轻量 handle：`TypeId`、`FieldId`、`EnumId`，避免跨 module 传字符串再查一次。
- `TypeSystem` 负责 assignability、nullable、dict key、ref target、polymorphic range、abstract/singleton instantiation。
- `ValueTypeRules` 只做 schema shape 规则；data-model 仍负责 record domain、source origin、ref graph。
- data-model 的 `validate_value_for_schema` 改为接收 query/type-system，不再每次从 `CftContainer` new 一个 view。

迁移顺序：

1. 先把 `CftSchemaView` public maps 改为 private，并补齐现有消费者所需 query。
2. 迁移 runtime mutation/write rules，减少到只调用 query/type-system。
3. 迁移 data-model value semantics，避免重复构造 view。
4. 迁移 table cell parser/render 的 type helpers。
5. 迁移 codegen schema context，只保留 C# 命名和 emit 所需 adapter。
6. 最后迁移 checker runtime ops 和 LSP 语义功能。

leverage：

- schema/type 规则改一处，所有 host/provider/codegen 受益。
- conformance tests 通过 interface 测语义，不再依赖某个实现文件。
- 新语言特性进入 schema module 后，不需要散落改多个 consumer。

验收：

- `CftSchemaView` 不暴露 public maps。
- data-model、runtime、table-core、codegen 不直接遍历 `CftTypeMeta::all_fields` 来实现新语义。
- assignability、dict key、enum、nullable、ref target 有跨 crate conformance tests。

## 8. Phase 4: provider option and operation contract

目标：让 provider seam 更深。project 可以保留 raw config parse，但 runtime/provider operation 不应长期传递裸 `serde_json::Value` 并让每个调用点自己解释。

当前状态：

- `SourceConfig.options` 和 `OutputConfig.options` 是 `serde_json::Value`。
- `ResolvedSource.options`、`OutputSpec.options` 继续传递 raw value。
- CLI Lark helper 仍直接读取 option JSON。
- `SourceProviderDescriptor.option_keys` 只能辅助 provider selection，不能表达 option type、requiredness、diagnostic path。

目标设计：

```rust
pub struct RawOptions {
    value: serde_json::Value,
    location: ConfigLocation,
}

pub struct ProviderOptionSchema {
    pub fields: &'static [ProviderOptionField],
}

pub struct DecodedSourceOptions {
    provider_id: String,
    raw: RawOptions,
    normalized: NormalizedProviderOptions,
}
```

关键动作：

1. project config 继续只解析出 raw options，但保留 key path/location。
2. provider 增加 option schema/decoder：
   - source provider decode source options。
   - table/dimension manager decode operation-specific options。
   - exporter/codegen decode output options。
3. runtime 在 resolve/load/export/codegen 前做统一 option decode/preflight。
4. `ResolvedSource` 不再暴露裸 `serde_json::Value` 给 host。
5. CLI Lark helper 不再直接解析 `source.options`，改调用 provider/table manager 的 layout/query interface。

注意：

- 不建议把 provider shared algorithms 放进 `coflow-api`。
- 不建议用宏或大型反射系统做第一版。
- 可以先用 provider-owned typed structs 和统一 diagnostic adapter，raw `Value` 只停留在 project/config adapter。

leverage：

- option 错误能在 project/config 阶段给出稳定位置。
- provider selection 不再靠 option key 猜测。
- editor/LSP 未来可以从 option schema 生成配置提示。

验收：

- runtime 生产路径不直接读取 provider-specific option key。
- `ResolvedSource` host-facing report 中不包含 raw option object。
- provider option 错误都带 config key path。

## 9. Phase 5: provider interface narrowing

目标：`coflow-api` 继续作为 provider trait crate，但不要 re-export 大量内部模型作为事实上的共享模型入口。

当前证据：

- `crates/coflow-api/src/lib.rs` re-export `CftContainer`、`CfdDataModel`、`CfdValue`、`CfdInputRecord`、`RecordOrigin` 等。
- `SourceProvider` / `SourceWriter` / `TableManager` context 直接携带 `CftContainer` 或 `CfdDataModel`。

目标：

- `coflow-api` 暴露 provider contract types、diagnostics、artifacts、operation requests。
- schema/model 通过窄 query facade 进入 context，而不是把完整 container/model 暴露给 provider。
- provider load 仍可产出 source-neutral input records，但 record/value 类型的 owner 要清楚：data-model 拥有 value/model，api 只引用窄 DTO 或 facade。

迁移顺序：

1. 删除 `coflow-api` 对 schema/model 类型的泛 re-export，先让调用点显式从 owner crate import。
2. 将 `SourceLoadContext.schema: &CftContainer` 换成 schema query facade。
3. 将 `ExportContext` / `CodegenContext` 改为 schema/model query facade。
4. provider writer context 改为 write-specific view，不给完整 session/model。
5. 对 `WriterCapabilities`、`TableManagerDescriptor`、`DimensionSourceManagerDescriptor` 做统一 capability vocabulary。

leverage：

- provider seam 的 interface 更小，depth 更高。
- provider 不容易依赖内部模型实现细节。
- schema/model owner crate 的变更不会通过 `coflow-api` 扩散。

验收：

- `coflow-api/src/lib.rs` 不再 re-export `CftContainer` / `CfdDataModel` / `CfdValue`。
- provider context 只暴露 query，不暴露完整 mutable 或结构字段。
- provider shared algorithms 仍留在 `coflow-loader-table-core` / `coflow-exporter-core`。

## 10. Phase 6: host adapters and LSP reuse

目标：host 只通过 adapter 消费 runtime/query，减少对内部模型的直接耦合。

Editor backend：

- 当前 `editors/cfd-editor/src-tauri` 直接 import `coflow_cft::CftSchemaView`、`coflow_data_model::{CfdValue, CfdRecordId, ...}`。
- 第一阶段允许为了 TS binding 保留 value DTO export，但 UI snapshot 转换应集中在 runtime/editor adapter。
- 目标是 `EditorSessionSnapshot`、`EditorRecordView`、`EditorGraphView` 由 runtime 或 editor adapter 从深 session query 生成。

LSP：

- 当前 LSP 是 schema/text oriented，这合理。
- 如果未来做 data-aware LSP，不应重复 runtime source/record/file index。
- 目标是 LSP 使用 `SchemaSession` 做 CFT/CFD 语义，使用 `ReadOnlyDataSession` 做 data-aware features。

CLI：

- CLI 继续拥有参数解析、human/JSON 输出、artifact staging/commit。
- CLI 不直接读取 provider options 细节。
- CLI 不直接处理 schema/model 组合逻辑。

验收：

- editor 生产代码不直接访问 `ProjectSession` 内部字段。
- LSP schema-only 功能不需要 data load；data-aware 功能复用 runtime read-only session。
- root CLI artifact staging 仍留在 `src/artifacts.rs` / `src/artifacts/staging.rs`。

## 11. Phase 7: focused provider maintenance after core seams

目标：等 runtime/schema/provider seam 稳定后，再处理具体 provider 热点。

Lark：

- 保持 remote provider 不被 runtime/CLI 特殊化。
- 补 method contract tests：GET/POST/PUT/DELETE 不 fallback。
- token cache key 包含 app identity / tenant identity / secret fingerprint。
- retry/backoff、permission、rate limit、sheet missing 分成稳定 diagnostic codes。

Excel writer：

- `crates/coflow-loader-excel/src/writer.rs` 仍约 595 行，是生产代码热点。
- 先不要为了行数拆，等 table/provider option contract 稳定后，再按 workbook adapter、header sync、row mutation、diagnostics 拆。

LSP large files：

- `coflow-lsp/src/lib.rs`、`semantic_tokens.rs`、`completion.rs`、`cfd/mod.rs` 是热点。
- 先不要大动，等 schema query facade 完成后再拆 feature module，避免重复迁移。

验收：

- 每次 provider 深改都有 fake client / file golden tests。
- 远程 provider 不引入 runtime special case。

## 12. Suggested implementation order

建议按下面 PR 序列推进：

1. `checker-dimension-plan`
   - 新增 `DimensionCheckPlan`。
   - 删除 `coflow-checker -> coflow-project`。
   - 加 cargo metadata guard。
2. `runtime-session-facade`
   - 新增 `Runtime` facade 和 session kind。
   - 把 `ProjectSession` 字段私有化。
   - 迁移 CLI/editor 调用。
3. `schema-query-private-fields`
   - 私有化 `CftSchemaView` maps。
   - 补 query/type-system methods。
   - 迁移 runtime/data-model/table-core/codegen。
4. `provider-option-decode`
   - 引入 raw option location。
   - provider-owned typed decode。
   - 去掉 CLI Lark option direct parsing。
5. `api-reexport-cleanup`
   - 删除 `coflow-api` 内部模型 re-export。
   - provider contexts 改为 query facade。
6. `host-adapter-cleanup`
   - editor snapshot adapter。
   - LSP schema session reuse。
7. `provider-hotspots`
   - Lark remote contract。
   - Excel writer split。
   - LSP feature split。

如果希望更激进，可以把 1 和 2 合并成一个 breaking PR；但 3、4、5 不建议一起做，因为 schema query、provider options、api re-export 三者同时迁移会让 review locality 变差。

## 13. Stop conditions

暂停扩大重构的条件：

- `cargo check --workspace` 或 `cargo test --workspace` 不通过。
- read-only session 出现文件写入。
- build session 维度 rollback 行为退化。
- export/codegen golden 出现未解释差异。
- runtime 重新引入 concrete provider 生产依赖。
- `coflow-api` 开始承载 table/cell/export implementation algorithms。
- 新 facade 只包了一层转发，没有提升 depth。

## 14. Final target state

完成后应达到：

- runtime 有深 interface：read-only/build/write session 能力由 type 表达。
- `ProjectSession` 不再是公开字段袋，host 只能通过 query/report/command 使用它。
- checker 不依赖 project config。
- schema/type/value 语义集中在一个深 module，通过 query/type-system 提供 leverage。
- provider options 在 project config adapter 之后被 typed decode，不再裸 `serde_json::Value` 穿透 runtime。
- `coflow-api` 是 provider contract crate，不是 schema/model re-export crate。
- editor/LSP/CLI 都通过 adapter 使用 runtime，不复制核心 pipeline。
- 具体 provider 的复杂性留在 provider module，runtime 保持 provider-neutral。
