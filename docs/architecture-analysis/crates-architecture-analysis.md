# Coflow crates architecture analysis

本报告只依据当前仓库源码、`Cargo.toml` / `cargo metadata`、`crates/` 下的模块结构和实现细节整理；没有使用站点参考文档作为事实来源。目标是描述现状，而不是复述期望架构。

## 1. Source audit scope

当前 `crates/` 下共有 18 个 crate：

| Crate | 源码文件数 | 源码行数 | 当前角色概述 |
|---|---:|---:|---|
| `coflow-api` | 1 | 1031 | provider trait、诊断、artifact、写入协议、registry |
| `coflow-builtins` | 1 | 48 | 注册内置 loader/writer/exporter/codegen |
| `coflow-cfd` | 3 | 599 | CFD 文本 AST 与 parser |
| `coflow-cft` | 13 | 6256 | CFT lexer/parser/schema compiler/type checker/schema view |
| `coflow-checker` | 6 | 3089 | check 表达式运行、内置函数、依赖收集 |
| `coflow-codegen-csharp` | 8 | 3666 | C# IR、命名、渲染、代码生成 provider |
| `coflow-data-model` | 9 | 3922 | 输入记录编译、数据模型、索引、origin、值语义 |
| `coflow-engine` | 14 | 6963 | project runtime、load/build/check、索引、读写、文件操作 |
| `coflow-exporter-core` | 1 | 409 | 格式无关导出遍历与编码接口 |
| `coflow-exporter-json` | 1 | 128 | JSON exporter provider |
| `coflow-exporter-messagepack` | 1 | 156 | MessagePack exporter provider |
| `coflow-loader-cfd` | 2 | 2297 | CFD loader 与 CFD writer |
| `coflow-loader-csv` | 2 | 1145 | CSV loader/writer |
| `coflow-loader-excel` | 2 | 1378 | Excel loader/writer |
| `coflow-loader-lark` | 1 | 2248 | Lark sheet loader/writer/HTTP/cache/DTO |
| `coflow-loader-table-core` | 4 | 2689 | 表格抽象、cell 解析、table writer 公共逻辑 |
| `coflow-lsp` | 4 | 3836 | CFT/CFD LSP stdio server、diagnostics、URI |
| `coflow-project` | 1 | 1268 | project config、路径、schema discovery/build helpers |

行数统计不包含 `tests/`。测试覆盖分布总体较好，但有几个高风险模块以单文件形式集中，测试不能抵消模块边界不足带来的维护成本。

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
| `coflow-engine` | `coflow-api`, `coflow-cfd`, `coflow-cft`, `coflow-checker`, `coflow-data-model`, `coflow-loader-cfd`, `coflow-loader-csv`, `coflow-loader-table-core`, `coflow-project` |
| `coflow-lsp` | `coflow-api`, `coflow-cfd`, `coflow-cft`, `coflow-project` |

粗粒度上可以看出五层：

1. 语言层：`coflow-cft`, `coflow-cfd`。
2. 数据语义层：`coflow-data-model`, `coflow-checker`。
3. provider 契约与实现层：`coflow-api`, loader/exporter/codegen crates, `coflow-builtins`。
4. project/runtime 层：`coflow-project`, `coflow-engine`。
5. host 层：根 CLI 与 `coflow-lsp`。

这个分层方向基本合理，但实际依赖有几个明显偏移：

- `coflow-api` 不是纯契约层，它直接依赖 `coflow-cft` 和 `coflow-data-model`，把 schema/data model 类型暴露给 provider trait。这样 provider 接口稳定性会被语言模型和数据模型的内部变化牵动。
- `coflow-engine` 既依赖 provider trait，又直接依赖 `coflow-loader-cfd`、`coflow-loader-csv`、`coflow-loader-table-core` 等具体 provider/共享 provider 实现。结果是 runtime 层并非 provider-neutral。
- `coflow-checker` 依赖 `coflow-project`，但 check 语义本身理论上只需要 schema/data model；这会把 project 路径/配置语义引入 check runtime。
- `coflow-lsp` 主要处理 schema/LSP，但没有复用 engine 的完整 session，也没有独立 protocol 层；单文件 server 同时承担协议解析、项目构建、completion、hover、semantic tokens。

### 2.2 Actual runtime flow

当前核心路径是：

1. `coflow-project` 读取 `coflow.yaml`，解析 `schema`、`sources`、`outputs`、`dimensions`，解析路径和 schema 文件。
2. `coflow-cft` 对 schema 文件 lex/parse，再 compile 到 `CftContainer`。
3. `coflow-engine::build_project_session` 创建 schema session。
4. engine 通过 registry 选择 loader，加载 sources 为 `CfdInputRecord`。
5. `coflow-data-model` 编译输入记录，构建 table/index/ref/spread 图。
6. `coflow-checker` 执行 schema 中的 check 表达式，并返回 diagnostics 与依赖图。
7. engine 聚合 diagnostics/source/record/file/dependency index，供 CLI/editor 读写。
8. exporter/codegen 根据 project outputs 提交 artifact。

但 `build_project_session` 里存在一个非常关键的现状：当存在 dimension fields 时，它先加载一次数据，调用 `dimensions::regenerate_dimension_sources`，然后重置索引并重新加载一次数据。`regenerate_dimension_sources` 属于写入/再生成行为，意味着“构建 session”并不纯，只要项目包含维度字段就可能产生文件系统副作用和二次 load。这对 CLI 可能方便，但对 editor/LSP/只读检查/并发构建都不友好。

### 2.3 Current architectural advantages

- 语言解析、数据模型、provider、runtime 大体被拆成 crate，已经具备可测试的边界。
- provider registry 支持 loader/writer/exporter/codegen 扩展，内置 provider 集中在 `coflow-builtins` 注册。
- `coflow-loader-table-core` 抽出了 CSV/Excel/Lark 共享的表格读取与写入逻辑，避免三个 provider 完全重复。
- `coflow-exporter-core` 让 JSON 和 MessagePack 共享格式无关遍历逻辑。
- data model 编译分阶段：输入验证、索引、singleton、ref/spread 解析，结构清晰。
- diagnostics 从 API 到 engine/LSP 有统一表达，用户可恢复错误大多不是 panic。

### 2.4 Current architectural disadvantages and risks

#### High severity

1. **Session build has write side effects.**
   `coflow-engine::build_project_session` 会在维度场景调用 regeneration，再重新加载数据。构建行为和文件变更行为混在一起，导致只读命令也可能写文件；出错时还会出现“第一次 load 成功、生成失败、第二次 load 未执行”的半状态。

2. **Engine is not provider-neutral.**
   `coflow-engine/src/data_files.rs` 内部硬编码 `DataFileProvider::{Cfd,Csv,Excel}`，并直接调用 CSV/CFD/Excel 具体 writer 或文件操作。新增本地文件 provider 需要改 engine，而不是只注册 provider。

3. **Semantic model duplication exists across CFT/data-model/checker/codegen/LSP.**
   `coflow-cft::schema_view`, `coflow-data-model::schema_view`, `coflow-codegen-csharp::schema_view`, `coflow-checker::CheckValue`/type logic、LSP completion/hover 里都有 schema/type/value 解释逻辑。语义规则变动时必须多处同步，容易出现 CLI build、checker、codegen、LSP 展示不一致。

4. **Several central crates are effectively monoliths.**
   `coflow-api/src/lib.rs`、`coflow-project/src/lib.rs`、`coflow-loader-lark/src/lib.rs`、`coflow-lsp/src/lib.rs`、`coflow-checker/src/check/evaluator.rs` 都是千行级单文件或单模块。边界不清时，新增行为容易继续堆叠，测试粒度和 review 粒度会变差。

5. **Record source index can lose duplicate-loader metadata.**
   `RecordIndex::finalize_with_model` 把 pending records 放入 `BTreeMap<RecordCoordinate, PendingRecordRef>`，重复 coordinate 后者覆盖前者；注释说 model build 会拒绝重复，但 engine 在 finalize 阶段已经丢失了重复来源的完整 metadata。诊断虽然来自 model build，但后续 record/file index 无法完整映射所有 rejected duplicate source rows。

#### Medium severity

6. **`coflow-api` surface is too wide for a stable plugin ABI.**
   同一文件定义 artifacts、diagnostics、source resolution、loader/exporter/codegen/writer trait、registry、flat diagnostic。任何 trait 或 model 类型改动都会让所有 provider 重编译并理解更多上下文。

7. **Writer capability model is incomplete for current engine file operations.**
   `WriterCapabilities` 已经包含 `can_create_table`，`DataWriter` 也已有 `create_table`，但 engine 的本地 `create_data_file` / `sync_data_header` 没有统一走 provider writer。尤其 header sync 没有对应的通用 provider operation，很多实际约束仍依赖 origin、field path、source options、sheet cache、provider 细节，host 很难在调用前完整预测可写性。

8. **Project config expands environment variables during deserialize.**
   `coflow-project` 的 `expand_env_references` 在反序列化阶段把 `${VAR}` 替换成环境变量值。这样配置解析不再是纯函数，诊断和缓存也受进程环境影响；同时缺失变量会保留原字符串，不容易区分“有意写字面量”和“变量缺失”。

9. **Lark provider concentrates remote API, auth, loader, writer, cache, DTO, tests fakes in one file.**
   远程 API 变化时影响面很大。`LarkHttpClient::put_json` 默认通过 `post_json` 实现，虽然真实 client 覆盖了 PUT，但测试 fake 或第三方 client 若未覆盖，会产生方法语义偏差。

10. **Checker numeric behavior needs explicit policy.**
   integer division 使用 checked op；float division/power 直接 `lhs / rhs`, `lhs.powf(rhs)`，可能产生 `NaN`/`inf` 并继续流入 check 结果。当前实现是否允许非有限浮点值，需要明确为语言语义并覆盖测试。

11. **LSP is schema-centric and rebuilds aggressively.**
   `coflow-lsp` 对 open/change/save 都调用 project validation。没有明显的增量 AST/schema cache 分层，completion/hover/semantic tokens 和 diagnostics 共享一个大 server 状态，未来接入 data-aware 功能成本较高。

12. **Exporter/codegen share schema traversal concerns but still duplicate some interpretation.**
   JSON/MessagePack 输出格式不作为本次重构范围扩展；当前问题主要是 exporter/core、具体 exporter、codegen 仍需要围绕 schema/data model 做局部解释，应该收敛到统一 schema query 和 golden tests。

#### Low severity / maintainability

13. 多个 provider 自己做 option parsing、diagnostic mapping、source origin mapping，缺少统一 option schema。
14. table-core 以通用表格模型承载 CSV/Excel/Lark，但不同源的能力差异被压平，导致 writer 仍要回到 provider 内处理细节。
15. codegen 和 exporter 都需要遍历 schema/data，但没有共享“schema query facade”，因此重复构建自己的 view。
16. 大量公开 struct 字段直接暴露，后续想增加 invariant 或延迟计算会比较困难。

## 3. Medium-grained crate analysis

### 3.1 `coflow-api`

文件结构：单文件 `src/lib.rs`，约 1031 行。

现状：

- 定义 `ArtifactSet` / `ArtifactFile` / `ArtifactContent`。
- 定义 `DiagnosticSet`、`Diagnostic`、`Severity`、`Label`、`SourceLocation`、`FlatDiagnostic`。
- 定义 source 解析上下文和 `ResolvedSource`。
- 定义 `DataLoader`、`DataWriter`、`DataExporter`、`CodeGenerator` trait。
- 定义 `WriterCapabilities`、各类 write request、`WriteOutcome`。
- 定义 `ProviderRegistry`，负责注册、查找、选择 loader/writer/exporter/codegen。
- 直接使用 `coflow-cft::CftContainer` 和 `coflow-data-model` 的 model/value/origin 类型。

问题与风险：

- API 层当前是“契约 + 共享 DTO + registry + diagnostic utility”的混合层。短期简单，长期会变成所有 crate 的共同变更热点。
- provider trait 暴露 `CftContainer`、`CfdDataModel`、`CfdValue`、`RecordOrigin`，使 provider 与内部 schema/data model 强绑定。
- `DataWriter` trait 方法很多，默认行为和 capability 之间缺少机器可验证关系。新增写入能力会继续扩大 trait。
- `ProviderRegistry` 选择 loader 依赖 provider 自报 probe result；冲突时的策略集中在 API，但错误体验和 host 策略可能不同。

建议：

- 拆分为 `diagnostics`、`artifacts`、`provider_traits`、`registry` 子模块。
- 为 provider 暴露更窄的 schema/data facade，减少直接依赖内部模型。
- 将 writer capabilities 细化为可组合 operation descriptors，并允许 provider 给出预检诊断。

### 3.2 `coflow-builtins`

文件结构：单文件 `src/lib.rs`，约 48 行。

现状：

- `default_provider_registry` 创建 registry。
- `register_default_providers` 注册 Excel、CSV、Lark、CFD loaders/writers，JSON/MessagePack exporters，C# codegen。

问题与风险：

- 注册顺序是隐含策略。例如 loader probe 同等置信度时，顺序可能影响选择。
- 所有内置 provider 都是无条件依赖；如果未来希望裁剪 remote provider 或减少 CLI 二进制依赖，需要 feature gate。
- registry 构建没有暴露“最小本地 provider set”“无网络 provider set”等 profile。

建议：

- 明确注册顺序语义，或让 registry selection 独立于注册顺序。
- 增加 feature/profile，区分 local-only、remote、full。

### 3.3 `coflow-cfd`

文件结构：

- `ast.rs` 66 行：CFD AST。
- `parser.rs` 492 行：parser。
- `lib.rs` 41 行：导出入口。

现状：

- 提供 CFD text parse 到 AST 的能力。
- 依赖 `coflow-cft` 的 span/identifier 相关能力。
- AST 比较薄，真正数据语义在 loader/data-model 里处理。

问题与风险：

- parser 和 AST 边界清楚，但错误恢复能力有限；对 LSP/editor 场景可能不够。
- CFD 写回在 `coflow-loader-cfd::writer`，parser 与 serializer 不在同一 crate，格式 roundtrip invariant 难集中测试。
- CFD 语法与 CFT 类型语义分离是合理的，但跨 crate 修改语法时需要同步 loader、writer、LSP。

建议：

- 增加 parser recovery 模式供 LSP 使用。
- 把 CFD formatting/serializer 的公共 AST roundtrip 测试与 parser 更紧密绑定。

### 3.4 `coflow-cft`

文件结构：

- `parser.rs` 1307 行。
- `schema/compiler.rs` 1334 行。
- `schema/type_checker.rs` 701 行。
- `schema_view.rs` 509 行。
- `lexer.rs` 457 行。
- `schema/support.rs` 448 行。
- 其余 AST、container、error、identifier、span、lib。

现状：

- 这是语言核心 crate，负责 CFT lex/parse、AST、schema compile、type check、schema view。
- schema compiler 处理 annotation、继承/多态、默认值、字段、check 结构等。
- type checker 处理 check/default 相关表达式类型。
- `schema_view` 给其他 crate 查询 schema 结构。

问题与风险：

- parser/compiler/type checker 都偏大，新增语法会触及多个大文件。
- CFT schema view 不是唯一 view；data-model/codegen/checker/LSP 也有自己的语义投影。
- 编译与 type check 的错误码/错误定位若继续增长，`error.rs` 和 compiler 会更难维护。
- parser 与 compiler 测试已有不少，但边界测试和语义一致性测试需要随新增注解持续补齐。

建议：

- 将 schema compiler 拆为 name collection、annotation lowering、inheritance/default/check lowering、validation 几个模块。
- 将 type checker 的 builtin signature、表达式类型规则导出为 checker/LSP 可复用的只读服务。
- 建立跨 crate semantic conformance tests，保证 CFT view、data-model view、codegen view 一致。

### 3.5 `coflow-data-model`

文件结构：

- `compiler.rs` 1152 行。
- `model.rs` 1050 行。
- `edge_index.rs` 349 行。
- `value_semantics.rs` 333 行。
- `schema_view.rs` 315 行。
- `diagnostic.rs` 299 行。
- `origin.rs` 250 行。
- `serde_i64.rs` 121 行。
- `lib.rs` 53 行。

现状：

- 将 `CfdInputRecord` 编译成 `CfdDataModel`。
- 分阶段校验输入、构建 table/primary/domain/inheritance indexes、校验 singleton、解析 refs/spreads。
- 保存 record origin，供 diagnostics、engine index、writer 定位。
- 管理 value serialization semantics 和 ref/spread edge indexes。

问题与风险：

- `compiler.rs` 与 `model.rs` 都超过千行，是数据语义主战场。
- `SchemaView` 与 CFT/codegen/checker 的 view 重复。
- `RecordOrigin` 同时服务本地/远程/table/CFD 多种来源，后续 provider 特有 origin 信息可能让 enum 膨胀。
- `CfdRecordId` 由输入顺序派生，构建稳定性依赖 loader 顺序；跨文件/增量编辑下需要明确 ID 稳定性预期。
- duplicate 和 rejected records 在 data model 层有 diagnostics，但 engine record index 会只保留成功 model 记录，后续 source mapping 不完整。

建议：

- 把 `compiler.rs` 拆为 validation、indexing、ref resolution、singleton validation。
- 明确 `CfdRecordId` 稳定性 contract：仅 session 内稳定，还是跨 reload 尽量稳定。
- 设计 provider-agnostic origin extension，避免 `RecordOrigin` enum 持续膨胀。

### 3.6 `coflow-checker`

文件结构：

- `check/evaluator.rs` 2327 行。
- `lib.rs` 205 行。
- `check/value.rs` 288 行。
- `check/runner.rs` 179 行。
- `check/builtins.rs` 85 行。
- `check.rs` 5 行。

现状：

- 对 data model 里的 records 执行 schema check。
- `CheckEvaluator` 同时负责表达式求值、路径/诊断、内置函数、dimension variant 应用、依赖收集、错误渲染。
- 输出 dependency graph，engine 转为 `DependencyIndex`。

问题与风险：

- `evaluator.rs` 过大，是当前最明显的单模块复杂点之一。
- check value/type 语义与 CFT type checker 有重复。编译期认为合法的表达式，运行期仍可能因实现差异产生不同结果。
- float 运算没有看到统一的 finite/NaN/inf 策略；`powf`、float division 可能产生非有限值。
- checker 依赖 `coflow-project`，使纯 check runtime 混入 project 层概念。
- dependency collection 和 evaluation 混在一起；未来增量 check 或 explain/debug check 会难拆。

建议：

- 拆分 evaluator：环境/lookup、算术与比较、内置函数、诊断渲染、依赖收集、dimension handling。
- 将编译期 type checker 与运行期 value operation 的 signature/规则合并到共享模块。
- 明确 float 错误策略，并补齐除零、NaN、inf、overflow 测试。
- 移除或隔离对 `coflow-project` 的依赖。

### 3.7 `coflow-project`

文件结构：单文件 `src/lib.rs`，约 1268 行。

现状：

- 解析 project config，包括 schema、sources、outputs、dimensions。
- 拒绝 duplicate keys 和废弃字段。
- 处理 `${VAR}` 环境变量展开。
- 管理项目路径解析、schema 文件发现、schema build helper、diagnostic conversion。

问题与风险：

- 单文件承担 config schema、serde、path、schema discovery、diagnostic bridge，职责偏多。
- env 展开在 deserialize 阶段发生，影响可重复性和 cache key。
- source/output option 仍是 `serde_json::Value`，provider option schema 没有统一校验层；很多错误要到 loader/writer 才暴露。
- `coflow-project` 依赖 `coflow-api` 主要为了 diagnostics/source location，增加了低层 config crate 的 API 耦合。

建议：

- 拆分 `config`、`paths`、`schema_files`、`diagnostics`。
- 将 env expansion 变成显式 resolve step，保留 unresolved config 用于 diagnostics/cache。
- 引入 provider option schema 或 typed option decode helper，减少 provider 重复解析。

### 3.8 `coflow-engine`

文件结构：

- `mutation.rs` 1383 行。
- `lib.rs` 1277 行。
- `data_files.rs` 940 行。
- `writes.rs` 702 行。
- `dimensions/regenerate.rs` 465 行。
- `files.rs` 330 行。
- `schema_inspect.rs` 329 行。
- `data_read.rs` 268 行。
- `write_rules.rs` 250 行。
- `data_patch.rs` 222 行。
- 其余 records/dimensions info/synthesize/mod。

现状：

- 负责构建 `ProjectSession` / `ProjectSchemaSession`。
- 聚合 schema、model、diagnostics、source index、record index、file index、dependency index。
- 加载 project data，运行 check，处理 dimensions。
- 提供 data read、patch、mutation、write rules、writes、schema inspect、data file create/sync。
- `data_files.rs` 直接处理 CFD/CSV/Excel 文件创建和 header sync。

问题与风险：

- `build_project_session` 有文件系统副作用和双 load，是最需要优先处理的架构问题。
- `data_files.rs` 绕过 provider registry，硬编码三种 provider，并直接进行文件写入/Excel 操作。
- engine 依赖具体 provider crate，破坏 provider 扩展边界。
- `lib.rs` 仍承担 session、indexes、load orchestration、format helper 等多种职责。
- `RecordIndex::finalize_with_model` 会静默丢弃未进入 model 的 pending entries，并用 map 覆盖重复 coordinate 的 pending metadata。
- mutation/writes/write_rules 分离已有雏形，但 API 边界还不够清晰，host 很难判断何时需要 schema-only session、full session、或 writable session。

建议：

- 将 session build 拆为 `load/check` 与 `generate dimensions` 两个显式步骤；只读命令默认不写。
- 把 data file create/sync 下沉到 writer/provider capability，engine 只编排 registry。
- 把 `ProjectSession` indexes 拆到独立模块，并保留 rejected/pending source metadata 供 diagnostics。
- 定义 `SessionBuildOptions`，明确是否允许 side effects、是否运行 checks、是否加载 implicit dimension sources。

### 3.9 `coflow-loader-table-core`

文件结构：

- `table.rs` 1229 行。
- `cell_value/mod.rs` 977 行。
- `writer.rs` 460 行。
- `lib.rs` 23 行。

现状：

- 抽象表格 source、sheet、row、cell。
- 解析表格 cell 到 data-model input value。
- 提供表格 writer 公共逻辑，供 CSV/Excel/Lark 复用。

问题与风险：

- `table.rs` 和 `cell_value` 都很大，且承载了跨 provider 的复杂语义。
- 表格模型把 CSV/Excel/Lark 拉平，但三者能力不同：sheet、公式、远程 API、行列限制、格式保留都不同。
- cell value 语法是用户可见语义，和 CFD value/default/check 语义之间需要持续一致性测试。

建议：

- 拆分 table loading、header mapping、row origin mapping、cell parsing。
- 为 provider capability 留出更细的 table feature flags。
- 建立 cell value 与 CFT/data-model 的跨 crate conformance tests。

### 3.10 `coflow-loader-cfd`

文件结构：

- `lib.rs` 1181 行。
- `writer.rs` 1116 行。

现状：

- 加载 `.cfd` 文本为 input records。
- writer 支持字段写入、插入、删除、重命名、引用改写等。
- 复用 `coflow-cfd` parser 和 `coflow-loader-cfd::writer::serialize_value`。

问题与风险：

- loader 和 writer 都超过千行，CFD 文本编辑逻辑复杂。
- writer 需要维护格式、span、record block、field path、reference rewrite，风险高。
- parser 在 `coflow-cfd`，serializer 在 loader crate；roundtrip 规则分散。
- 对复杂嵌套对象、数组、dict、spread 的写入边界要持续用测试锁住。

建议：

- 拆分 writer 为 locating、patch planning、serialization、reference rewrite。
- 把 CFD format roundtrip 测试升为核心测试。

### 3.11 `coflow-loader-csv`

文件结构：

- `lib.rs` 691 行。
- `writer.rs` 454 行。

现状：

- CSV loader 解析文本表格，复用 table-core。
- CSV writer 做字段级/记录级写入和 header/row 处理。

问题与风险：

- CSV 是最简单的表格 provider，但仍要处理 quoting、header、row origin、field mapping。
- strictness 需要明确：空行、重复 header、缺失 id、额外列、编码/BOM 等用户输入可能不稳定。
- engine 的 data file sync 直接调用 `coflow_loader_csv::write`，使 CSV 细节泄漏到 engine。

建议：

- 明确 CSV 容错策略并补齐边界测试。
- 将 header sync/create file 作为 provider writer/capability，而不是 engine 直接调用。

### 3.12 `coflow-loader-excel`

文件结构：

- `lib.rs` 784 行。
- `writer.rs` 594 行。

现状：

- Excel loader 通过 calamine 读取 workbook/sheet。
- Excel writer 处理 sheet/header/cell 写入。
- 和 CSV 一样复用 table-core。

问题与风险：

- Excel 格式保留、公式、日期/数字格式、空单元格语义都比 CSV 复杂；当前表格抽象可能隐藏格式风险。
- engine `data_files.rs` 直接用 calamine/openxml 写 header/sheet，进一步扩大 Excel 逻辑位置。
- 对 workbook 已存在样式/公式/冻结窗格等非数据内容的保护策略需要明确。

建议：

- 将 Excel 文件创建/header sync 合并到 Excel writer capability。
- 增加格式保持和公式保护测试。

### 3.13 `coflow-loader-lark`

文件结构：单文件 `src/lib.rs`，约 2248 行。

现状：

- 定义 `LarkSheetSource`、locator、diagnostics。
- 定义 `LarkHttpClient` trait 和 `UreqLarkHttpClient`。
- 加载 Lark spreadsheet metadata 与 values，映射成 table source。
- 实现 `LarkSheetLoader` 和 `LarkSheetWriter`。
- writer 维护 token cache、sheet id cache，支持 token expired 后 invalidate/retry。

问题与风险：

- 单文件混合 HTTP、auth、API DTO、loader、writer、cache、diagnostics，是 remote provider 中最高维护风险点。
- `LarkHttpClient::put_json` 默认走 `post_json`，对 fake client 友好但 HTTP method 语义不安全；第三方实现如果没覆盖 PUT 会悄悄错用 POST。
- token cache key 只按 `app_id`，如果同一 app_id 搭配不同 secret 或租户边界变化，语义不够严谨。
- remote API pagination/rate limit/retry/backoff 策略需要系统化；目前重点是 token retry。
- writer 校验 origin/source 归属较多依赖 string/token 解析，长期应有更明确的 remote document identity。

建议：

- 拆成 `http`, `auth`, `metadata`, `loader`, `writer`, `diagnostics`, `dto` 模块。
- 让 PUT/DELETE 成为必实现方法，或默认返回错误，避免 method fallback。
- token cache key 至少包含 app_id 与 secret hash/tenant identity。
- 抽象 remote API retry/backoff，并增加 fake client contract tests。

### 3.14 `coflow-exporter-core`

文件结构：单文件 `src/lib.rs`，约 409 行。

现状：

- 定义 `ExportEncoder`。
- `export_model_with_encoder` 遍历 schema/model，将 table records 编码给具体格式。
- 封装 export error。

问题与风险：

- core 依赖 CFT 和 data-model，承担格式无关语义。若继续增加输出结构职责，会继续膨胀。
- enum/dict/ref/polymorphic 等编码策略需要对所有 exporter 保持一致；目前是优点也是风险点。
- 输出格式不应在本次重构中扩展；重点应放在内部遍历、错误上下文和测试基线上。

建议：

- 给 export ordering、empty table、dict key、enum encoding 建立跨格式 golden tests。
- 复用统一 schema query，减少 exporter 与 codegen 的重复 schema 解释。

### 3.15 `coflow-exporter-json`

文件结构：单文件 `src/lib.rs`，约 128 行。

现状：

- 实现 JSON encoder/provider。
- 通过 exporter-core 导出 model。

问题与风险：

- JSON 输出用户可读，但 schema metadata 不随数据输出。
- 空表、空对象、null/default 的语义必须和 MessagePack 保持一致，否则不同格式消费结果不同。
- 错误类型简单，定位到具体 table/field 的能力依赖 core。

建议：

- 和 MessagePack 共享更多 golden fixtures。
- 保持现有 JSON 输出语义，不新增导出元数据或包裹结构。

### 3.16 `coflow-exporter-messagepack`

文件结构：单文件 `src/lib.rs`，约 156 行。

现状：

- 实现 MessagePack encoder/provider。
- 每张表编码为裸 MessagePack array bytes。
- 做 length 到 u32 的检查。

问题与风险：

- MessagePack 当前裸 array 输出语义应保持不变；重构重点是内部编码边界和测试覆盖。
- 与 JSON 一样需要确保空表、dict key、enum/ref 编码一致。
- 二进制格式更难人工诊断，错误上下文要尽量在 export 阶段丰富。

建议：

- 增加 decode roundtrip 测试，验证跨语言消费契约。

### 3.17 `coflow-codegen-csharp`

文件结构：

- `emit.rs` 1099 行。
- `tests.rs` 898 行。
- `ir.rs` 453 行。
- `schema_view.rs` 351 行。
- `lib.rs` 347 行。
- `names.rs` 202 行。
- `model.rs` 183 行。
- `render.rs` 133 行。

现状：

- 将 CFT schema 转换为 C# IR，再 emit/render 成代码 artifact。
- 管理命名、类型映射、MessagePack/JSON 相关结构。
- 有较大内联测试文件。

问题与风险：

- `emit.rs` 是主要复杂点，代码生成规则集中。
- 自有 `schema_view` 与 CFT/data-model view 重复。
- 命名冲突、保留字、泛型/继承/多态/循环引用是长期风险。
- 如果 MessagePack 导出格式变化，C# hydration/serialization 逻辑必须同步。

建议：

- 把 emit 拆成 declarations、fields、constructors、serialization、lookup/index。
- 将 schema query 改为共享 facade。
- 增加导出数据与生成 C# runtime 的端到端 fixture。

### 3.18 `coflow-lsp`

文件结构：

- `lib.rs` 3020 行。
- `cfd/mod.rs` 573 行。
- `diagnostics.rs` 152 行。
- `uri.rs` 91 行。

现状：

- stdio LSP server。
- 管理 open documents、schema build overrides、diagnostic publish。
- 支持 initialize、completion、hover、definition、documentSymbol、formatting、semanticTokens。
- `cfd/mod.rs` 处理 CFD 相关 LSP 辅助。

问题与风险：

- `lib.rs` 超过 3000 行，是当前最大单文件；协议处理、状态、schema analysis、completion、hover、formatting 混在一起。
- 变更时直接 validate project，没有明显增量缓存层。
- LSP 对 schema 的理解与 CFT compiler/schema view 有重复；completion/hover 容易和真实 compiler 语义漂移。
- 没有依赖 engine，因此 data-aware 能力若加入，可能要重新接 runtime session 或复制更多逻辑。

建议：

- 拆分 `server`, `protocol`, `state`, `schema_features`, `completion`, `hover`, `semantic_tokens`, `formatting`。
- 引入 incremental parse/build cache。
- 复用统一 schema query/type service，减少和 compiler/codegen/checker 的重复。

## 4. Cross-crate problem list

### 4.1 Provider boundary leakage

现状：

- `coflow-api` 定义 provider trait，并已经包含 `DataWriter::create_table`。
- `coflow-builtins` 注册具体 provider。
- 但 `coflow-engine` 仍直接依赖具体 local loader/writer，并在 `data_files.rs` 里硬编码 provider id 和文件格式，尤其是本地文件创建与 header sync 没有统一通过 writer capability。

影响：

- 新增 provider 需要改多个层级。
- engine 不能作为纯 runtime crate 被外部 host 使用而不带内置 provider 依赖。
- 当前 writer capability 已覆盖 `create_table`，但 engine 的本地 create/sync header 命令没有自然纳入这条统一 provider 路径，尤其缺少 sync header operation。

建议优先级：高。复用已有 `create_table` writer operation，并把 sync header 等 provider-specific 逻辑补成 writer operation 或单独 `DataFileManager` provider trait。

### 4.2 Session purity and side effects

现状：

- `build_project_session` 会在 dimension 场景再生成 source 文件。
- 构建 session 同时承担 read、derive、write、reload。

影响：

- 只读检查、LSP/editor、CI dry-run 的行为不可预测。
- 文件变化与 diagnostics 产生顺序复杂，失败恢复难。
- 并发调用 build session 可能互相影响生成文件。

建议优先级：高。建立 `build_project_session_readonly` 与 `regenerate_dimensions` 两条显式路径，或用 option 默认禁止副作用。

### 4.3 Semantic duplication

现状：

- CFT compiler/type checker、data-model schema view、checker runtime、codegen schema view、LSP features 都在解释 schema/type/value。

影响：

- 一处语义变更容易漏改其他 crate。
- 用户可能看到“LSP 认为可用、CLI build 不通过”或“codegen 输出与 exporter 不匹配”。

建议优先级：高。提取共享 schema query/type operation facade，配跨 crate golden tests。

### 4.4 Large-module concentration

现状高风险文件：

- `coflow-lsp/src/lib.rs` 3020 行。
- `coflow-checker/src/check/evaluator.rs` 2327 行。
- `coflow-loader-lark/src/lib.rs` 2248 行。
- `coflow-engine/src/mutation.rs` 1383 行。
- `coflow-cft/src/parser.rs` 1307 行。
- `coflow-cft/src/schema/compiler.rs` 1334 行。
- `coflow-project/src/lib.rs` 1268 行。
- `coflow-api/src/lib.rs` 1031 行。

影响：

- Review 难以隔离风险。
- 功能增长倾向继续堆入大文件。
- 单元测试很难对内部小组件建立直接覆盖。

建议优先级：中高。优先拆 LSP、checker evaluator、Lark provider、engine data_files/mutation。

### 4.5 Diagnostics and source mapping completeness

现状：

- diagnostics 总体统一，但 rejected records、duplicate pending records、provider parse failures 的 source metadata 在不同层级转译。
- engine `RecordIndex` 只 finalize 成功进入 model 的 records。

影响：

- UI/editor 可能无法展示所有失败记录的完整来源。
- 自动修复/写回对错误记录的支持有限。

建议优先级：中。保留 rejected source entry index，并让 diagnostics 引用 source-row/file metadata。

### 4.6 Remote writer robustness

现状：

- Lark writer 有 token cache 和一次 token retry。
- PUT 默认 fallback 到 POST，DELETE 默认错误。
- 远程 API 的限流、分页、backoff、幂等性策略没有抽成统一层。

影响：

- fake client 和真实 client 行为可能不一致。
- 大表/网络波动/权限变化时错误可能不够可恢复。

建议优先级：中。拆 remote client contract，明确 retry/backoff/error classification。

## 5. Prioritized optimization roadmap

### Phase 1: Fix architecture-critical boundaries

1. 把 `build_project_session` 改成默认只读；dimension regeneration 变成显式命令或 option。
2. 从 `coflow-engine` 移除对具体 local provider 的硬依赖，至少先把 `data_files.rs` provider 枚举改成 registry-driven。
3. 复用已有 writer `create_table` capability，并为 sync header 建立 provider operation，避免 engine 直接写 CSV/Excel/CFD。
4. 给 `RecordIndex` 保留 rejected/pending source metadata，避免 duplicate/rejected 来源丢失。

### Phase 2: Reduce semantic duplication

1. 建立共享 schema query/type facade，替换 data-model/codegen/checker/LSP 的重复 view。
2. 将 check 编译期签名与运行期 value operation 规则统一。
3. 建跨 crate conformance tests：schema view、cell value、export/codegen、checker numeric semantics。

### Phase 3: Split high-risk modules

1. `coflow-lsp/src/lib.rs` 拆 server/protocol/state/features。
2. `coflow-checker::evaluator` 拆 evaluation/env/builtins/diagnostics/dependencies。
3. `coflow-loader-lark` 拆 HTTP/auth/metadata/loader/writer/cache/DTO。
4. `coflow-project` 拆 config/path/schema discovery/diagnostics。
5. `coflow-api` 拆 diagnostics/artifacts/provider traits/registry。

### Phase 4: Improve output and remote contracts

1. JSON/MessagePack 保持现有输出语义，补 golden 和 decode roundtrip 测试。
2. C# codegen 与 exporter 增加端到端 fixture，验证生成代码能消费现有导出数据。
3. Lark remote client 建立严格 method contract、retry/backoff、cache key 策略。

## 6. Core structures, types, and entry points by crate

这一节按“模块结构 / 核心类型 / 核心方法或入口”整理当前源码里的实际骨架，方便和上面的架构问题对应阅读。

### 6.1 `coflow-api`

核心结构：

- 单文件契约 crate，集中定义 artifact、diagnostic、source、loader/writer/exporter/codegen trait、registry。

核心类型：

- 诊断：`DiagnosticSet`, `Diagnostic`, `Severity`, `Label`, `SourceLocation`, `FlatDiagnostic`。
- artifact：`ArtifactSet`, `ArtifactFile`, `ArtifactContent`, `ArtifactContentKind`。
- source/provider：`SourceLocationSpec`, `SourceResolveContext`, `ResolvedSource`, `OutputSpec`, `LoaderDescriptor`, `WriterDescriptor`, `ExporterDescriptor`, `CodegenDescriptor`, `ProbeResult`, `ProbeConfidence`。
- provider trait：`DataLoader`, `DataWriter`, `DataExporter`, `CodeGenerator`。
- write contract：`WriterCapabilities`, `WriteCellRequest`, `InsertRecordRequest`, `CreateTableRequest`, `DeleteRecordRequest`, `RenameRecordRequest`, `RewriteRecordReferencesRequest`, `WriteOutcome`, `WriteContext`, `WriteFieldPathSegment`。
- registry：`ProviderRegistry`, `ProviderRegistrationError`, `LoaderSelectionError`。

核心入口：

- `ProviderRegistry::register_loader/register_writer/register_exporter/register_codegen`。
- `ProviderRegistry::select_loader`。
- `DataLoader::probe/preflight/load`。
- `DataWriter::write_field/insert_record/create_table/rename_record/delete_record/rewrite_record_references`。
- `DataExporter::export`, `CodeGenerator::generate`。
- `map_diagnostics_with_origins`, `origins_of`, `Diagnostic::flat_view`。

### 6.2 `coflow-builtins`

核心结构：

- 内置 provider 组装层，不承载业务语义。

核心类型：

- 无自有核心数据类型，直接使用 `ProviderRegistry`。

核心入口：

- `default_provider_registry`：创建并填充内置 registry。
- `register_default_providers`：注册 Excel/CSV/Lark/CFD loader/writer、JSON/MessagePack exporter、C# codegen。

### 6.3 `coflow-cfd`

核心结构：

- `ast.rs`：CFD AST。
- `parser.rs`：文本 parser。
- `lib.rs`：公开 parser 和诊断。

核心类型：

- AST：`CfdAst`, `CfdRecord`, `CfdField`, `CfdValue`, `CfdBlock`, `CfdBlockEntry`, `CfdRef`。
- 诊断：`CfdSyntaxDiagnostic`。
- 内部 parser：`Parser`, `Token`。

核心入口：

- `parse_cfd` / `parser::parse`。
- `CfdValue::span`。
- `CfdBlock` 的辅助方法用于块内容定位。

### 6.4 `coflow-cft`

核心结构：

- `lexer.rs`：tokenize。
- `parser.rs`：CFT AST parser。
- `ast.rs`：语法 AST。
- `schema.rs`：编译后 schema model。
- `schema/compiler.rs`：AST 到 schema container 编译。
- `schema/type_checker.rs`：默认值/check 表达式类型检查。
- `schema_view.rs`：schema query view。
- `container.rs`：多 module container。
- `error.rs` / `identifier.rs` / `span.rs`：错误、命名、span。

核心类型：

- AST：`ModuleAst`, `Item`, `ConstDef`, `EnumDef`, `TypeDef`, `FieldDef`, `TypeRef`, `DefaultExpr`, `CheckBlock`, `CheckStmt`, `CheckExpr`。
- schema：`CftSchemaModule`, `CftSchemaConst`, `CftSchemaType`, `CftSchemaField`, `CftSchemaTypeRef`, `CftSchemaDefaultValue`, `CftSchemaCheckBlock`, `CftSchemaEnum`, `CftAnnotation`。
- container/view：`ModuleId`, `CftContainer`, `CftSchemaView`, `CftTypeMeta`, `CftEnumMeta`, `CftEnumValueMeta`, `CftDimensionFieldMeta`。
- diagnostics：`CftDiagnostics`, `CftDiagnostic`, `CftErrorCode`, `CftLabel`, `CftStage`。

核心入口：

- `lex`, `parse_module`。
- `CftContainer::add_module`, `compile`, `register_runtime_type`。
- `CftContainer::resolve_type/resolve_enum/resolve_const/all_types/all_enums/is_assignable/range_is_polymorphic/assignable_target_names/enum_variant_value`。
- `CftSchemaView::new`, `is_assignable`, `checks_for_actual`, `field_type`, `dimension_field`。
- `is_cft_identifier`, `record_key_ident_error`。

### 6.5 `coflow-data-model`

核心结构：

- `model.rs`：runtime data model、record/table/ref/spread/value。
- `compiler.rs`：input records 到 validated model。
- `schema_view.rs`：data-model 内部 schema 投影。
- `edge_index.rs`：ref/spread edge 构建。
- `origin.rs`：source origin 和 diagnostic mapping。
- `value_semantics.rs`：写入/插入值语义校验。
- `diagnostic.rs`：CFD/data diagnostics。

核心类型：

- model：`CfdDataModel`, `CfdModelBuilder`, `CfdTable`, `CfdRecord`, `CfdObject`, `CfdRecordId`, `CfdTypeId`, `CfdDomainId`, `CfdDomainIndex`, `CfdPolymorphicIndex`。
- values：`CfdValue`, `CfdDictKey`, `CfdEnumValue`, `CfdInputRecord`, `CfdInputValue`, `CfdInputDictKey`。
- graph：`RefSite`, `RefEdge`, `RefEdgeId`, `SpreadSite`, `SpreadEdge`, `SpreadEdgeId`。
- origin：`RecordOrigin`, `SourceDocument`, `TextSpan`, `SourceLocation`, `MappedDiagnostic`。
- diagnostics：`CfdDiagnostics`, `CfdDiagnostic`, `CfdErrorCode`, `CfdPath`, `CfdPathSegment`。
- semantics：`CfdValueSemanticContext`, `PendingInsertRef`, `CfdValueSemanticError`。

核心入口：

- `CfdDataModel::builder` and `CfdModelBuilder::add_input_record/build`。
- `CfdDataModel::record/table/records/tables/record_by_type_key/record_by_domain_key/lookup_assignable`。
- `CfdDataModel::direct_ref_edges/spread_edges/resolve_direct_ref/resolve_effective_ref/spread_source_at_path`。
- `CfdDataModel::dimension_field_value`。
- `validate_value_for_schema`, `validate_object_type_assignable`。
- `RecordOrigin::location_for_path`, `map_diagnostics`。

### 6.6 `coflow-checker`

核心结构：

- `lib.rs`：公开 check runner API。
- `check/runner.rs`：按 record/check block 调度。
- `check/evaluator.rs`：表达式求值、诊断、依赖收集。
- `check/value.rs`：check runtime value。
- `check/builtins.rs`：内置函数枚举。

核心类型：

- public：`DependencyGraph`, `CfdCheckExt`。
- runtime：`CheckEvaluator`, `CheckValue`, `LocatedCheckValue`, `CheckRecordRef`, `CheckEntry`, `CheckExplanation`, `EvalAbort`。
- builtin：`Builtin`。

核心入口：

- `run_checks`, `run_checks_for`, `run_checks_with_deps`。
- `run_checks_for_dimensions`, `run_checks_for_dimensions_with_deps`。
- `DependencyGraph::affected_by`。
- `CfdCheckExt::check/check_with_deps`。
- evaluator 内部核心路径：`eval_expr`, `eval_field`, `eval_index`, `eval_call`, `eval_method_call`, `eval_bin_op`, `compare_values`。

### 6.7 `coflow-codegen-csharp`

核心结构：

- `lib.rs`：公开 codegen API 和 provider。
- `ir.rs`：C# codegen options、project IR build、preflight。
- `model.rs`：C# 输出 IR。
- `schema_view.rs`：C# 专用 schema view。
- `emit.rs`：schema view 到 C# IR。
- `render.rs`：IR 到文本。
- `names.rs`：命名、转义、保留字。

核心类型：

- public API：`GeneratedFile`, `CsharpTemplate`, `CsharpDatabaseTemplates`, `CsharpCodegenError`, `CsharpCodeGenerator`。
- options：`CsharpCodegenOptions`, `CsharpDataFormat`, `CsharpIdAsEnumVariant`, `CsharpCodegenDiagnostic`。
- IR：`CsharpProject`, `CsharpType`, `CsharpProperty`, `CsharpEnum`, `CsharpEnumVariant`, `CsharpDatabase`, `CsharpTable`, `CsharpLoader`, `CsharpPolymorphicCase`。
- view：`SchemaView`, `TypeMeta`, `FieldMeta`, `FieldType`。

核心入口：

- `generate_csharp`, `generate_csharp_json`, `generate_csharp_messagepack`。
- `generate_csharp_with_database_templates`, `generate_csharp_with_id_as_enum_variants`。
- `build_project`, `preflight_csharp_codegen`。
- `build_csharp_enum`, `build_csharp_type`, `build_csharp_database`。
- `render_project`。
- `CsharpCodeGenerator::generate`。

### 6.8 `coflow-engine`

核心结构：

- `lib.rs`：session、indexes、source load/build orchestration。
- `writes.rs`：session write operations。
- `mutation.rs` / `data_patch.rs`：批量 mutation/patch。
- `data_files.rs`：本地 data file create/sync。
- `data_read.rs`：data query reports。
- `schema_inspect.rs`：schema report。
- `write_rules.rs`：写入类型/路径/值校验。
- `dimensions/*`：dimension type injection、source synthesis、source regeneration、info。
- `files.rs`, `records.rs`：file tree 和 record view/outcome。

核心类型：

- session/index：`ProjectSession`, `ProjectSchemaSession`, `RecordCoordinate`, `DiagnosticsStore`, `SourceIndex`, `ResolvedSourceEntry`, `SourceId`, `RecordIndex`, `RecordRef`, `FileIndex`, `DependencyIndex`, `DiagnosticLogicalLocation`。
- read/schema reports：`DataSourcesReport`, `DataListQuery`, `DataListReport`, `DataGetQuery`, `DataGetReport`, `SchemaInspectReport`, `SchemaFilesReport`, `SchemaTypeInfo`, `SchemaFieldInfo`。
- writes/mutation：`RecordView`, `RecordTarget`, engine `WriteOutcome`, `MutationRequest`, `MutationOp`, `MutationValue`, `PreparedMutation`, `MutationReport`, `DataPatchRequest`, `DataPatchOp`。
- data files/dimensions：`DataCreateFileOptions`, `DataSyncHeaderOptions`, `DataFileReport`, `DimensionInfo`, `DimensionField`, `DimensionFieldInfo`。

核心入口：

- `build_project_session`, `build_project_schema_session`, `configured_project_source`。
- `ProjectSession::record_view/record_views_in_file/file_tree/dimensions/id_for_coordinate/coordinate_of`。
- `ProjectSession::write_field/rename_record_key/insert_record/delete_record`。
- `ProjectSession::prepare_mutation/apply_prepared_mutation/apply_mutation/apply_data_patch/default_record_value`。
- `data_sources`, `data_list`, `data_get`。
- `inspect_schema`, `schema_files`。
- `create_data_file`, `sync_data_header`。
- `write_rules::validate_record_key/expected_type_for_write_path/validate_value_for_write/validate_value_for_insert`。
- `dimensions::inject_dimension_types/dimension_fields/dimension_sources/regenerate_dimension_sources`。

### 6.9 `coflow-exporter-core`

核心结构：

- 单文件共享导出遍历。

核心类型：

- public：`ExportEncoder`, `ExportError`。
- internal：`Exporter`, `SchemaView`, `FieldMeta`, `TypeTagMode`。

核心入口：

- `export_model_with_encoder`。
- `ExportEncoder::null/bool/int/float/string/array/map`。
- internal export path：`Exporter::export`, `encode_table`, `encode_record`, `encode_object_entries`, `encode_value`。

### 6.10 `coflow-exporter-json`

核心结构：

- JSON exporter provider + JSON encoder。

核心类型：

- `JsonExportError`, `JsonExporter`, internal `JsonEncoder`。

核心入口：

- `export_json_model`。
- `JsonExporter::export`。
- `JsonEncoder` implements `ExportEncoder<Value = serde_json::Value>`。

### 6.11 `coflow-exporter-messagepack`

核心结构：

- MessagePack exporter provider + binary encoder。

核心类型：

- `MessagePackExportError`, `MessagePackExporter`, internal `MessagePackEncoder`。

核心入口：

- `export_messagepack_model`。
- `MessagePackExporter::export`。
- `MessagePackEncoder::len_as_u32`。
- `MessagePackEncoder` implements `ExportEncoder<Value = rmpv::Value>` and serializes each table to bytes.

### 6.12 `coflow-loader-cfd`

核心结构：

- `lib.rs`：CFD text loader、text diagnostics、AST-to-input-record conversion。
- `writer.rs`：CFD source writer。

核心类型：

- loader：`CfdLoader`, `CfdTextDiagnostics`, `CfdTextDiagnostic`, `CfdTextErrorCode`, `CfdTextSpan`。
- writer：`CfdWriter`, internal `CacheEntry`, `WriteTarget`。
- parser internals：`ParsedCfdInputRecord`, `ParsedObjectFields`, `FieldMeta`。

核心入口：

- `parse_cfd_input_records`, `load_cfd_model`。
- `CfdLoader::probe/preflight/load`。
- `CfdWriter::new/invalidate`。
- `CfdWriter::write_field/insert_record/rename_record/delete_record/rewrite_record_references`。
- `serialize_value`。

### 6.13 `coflow-loader-csv`

核心结构：

- `lib.rs`：CSV parse/write、source config、diagnostics、loader。
- `writer.rs`：CSV writer。

核心类型：

- `CsvSource`, `CsvSheet`, `CsvDiagnostics`, `CsvInputRecords`, `CsvDiagnostic`, `CsvLabel`, `CsvLocation`, `CsvLoader`, `CsvWriter`。
- internal writer layout：`CsvLayout`。

核心入口：

- `parse`, `write`。
- `CsvSheet::new/with_type/with_key/with_columns`。
- `collect_input_records`。
- `CsvLoader::probe/preflight/load`。
- `CsvWriter::new` and writer trait methods。

### 6.14 `coflow-loader-excel`

核心结构：

- `lib.rs`：Excel source config、diagnostics、loader。
- `writer.rs`：Excel writer。

核心类型：

- `ExcelSource`, `ExcelSheet`, `ExcelDiagnostics`, `ExcelInputRecords`, `ExcelDiagnostic`, `ExcelLabel`, `ExcelLocation`, `ExcelLoader`, `ExcelWriter`。
- internal writer layout：`SheetLayout`。

核心入口：

- `ExcelSheet::new/with_type/with_key/with_columns`。
- `collect_input_records`。
- `map_label_with_record_offset`。
- `ExcelLoader::probe/preflight/load`。
- `ExcelWriter::new` and writer trait methods。

### 6.15 `coflow-loader-lark`

核心结构：

- 单文件 remote table provider，包含 source/diagnostics/HTTP client/loader/writer/cache/API DTO。

核心类型：

- source：`LarkSheetSource`, `LarkSheetLocator`。
- diagnostics：`LarkDiagnostics`, `LarkDiagnostic`。
- HTTP：`LarkHttpClient`, `UreqLarkHttpClient`, `LarkHttpMethod`。
- loader：`LarkSheetLoader`, `LarkLoaderCache`。
- writer：`LarkSheetWriter`, `LarkWriterCache`, `CachedToken`, `LarkWriteFailure`, `LarkWriteAuth`, `LarkInsertLayoutRequest`。
- API DTO：`AuthResponse`, `ApiEnvelope`, `WikiNodeData`, `SheetsQueryData`, `LarkSheetMetadata`, `ValuesData`, `ValueRange`。

核心入口：

- `load_lark_table_source`, `load_lark_table_source_with_client`。
- `LarkHttpClient::get/post_json/put_json/delete_json`。
- `LarkSheetLoader::new` and loader trait methods。
- `LarkSheetWriter::new`, `cached_tenant_token`, `cached_sheet_id`, `invalidate_caches`。
- `LarkSheetWriter::write_field/create_table/rename_record/delete_record/insert_record` where implemented by `DataWriter`。

### 6.16 `coflow-loader-table-core`

核心结构：

- `table.rs`：table source/sheet/diagnostic/input-record collection/write layout。
- `cell_value/mod.rs`：cell syntax parse/render。
- `writer.rs`：provider-neutral table write planning。
- `lib.rs`：re-export。

核心类型：

- table：`TableSource`, `TableSheet`, `TableSheetConfig`, `TableDiagnostics`, `TableInputRecords`, `TableDiagnostic`, `TableLabel`, `TableLocation`, `TableWriteLayout`。
- cell：`ParsedCell`, `CellValueDiagnostics`, `CellValueDiagnostic`, `CellValueErrorCode`, `CellRenderError`。
- write plan：`TableWritePlan`, `TableSetCell`, `TableAppendRow`, `TableDeleteRow`, `TableInsertRecord`, `TableFieldWrite`, `TableWriteDiagnostics`, `TableWriteDiagnostic`。

核心入口：

- `collect_table_input_records`。
- `resolve_table_write_layout`。
- `map_table_diagnostics`, `map_label_to_table`。
- `parse_cell`, `render_cell_value`。
- `plan_field_write`, `plan_insert_record`, `plan_delete_record`。

### 6.17 `coflow-lsp`

核心结构：

- `lib.rs`：stdio LSP server、state、schema build、CFT completion/hover/definition/formatting/semantic tokens。
- `cfd/mod.rs`：CFD diagnostics/symbols/hover/completion/definition/semantic tokens。
- `diagnostics.rs`：API diagnostic 到 LSP JSON。
- `uri.rs`：file URI/path conversion。

核心类型：

- server/state：`LspServer`, `OpenDocument`, `LspBuild`, `LspDocument`, `TextRequest`, `LspPosition`, `WordAt`, `CompletionScope`, `RawSemanticToken`, `CfdProjectSource`。
- diagnostics：`LspLabelLocation`。
- CFD helper：internal `TokenCollector`。

核心入口：

- `run`。
- server methods：`handle_message`, `initialize`, `open_document`, `change_document`, `close_document`, `validate_project`, `completion`, `hover`, `definition`, `document_symbol`, `formatting`, `semantic_tokens`。
- CFD functions：`cfd::syntax_diagnostics`, `document_symbols`, `semantic_tokens`, `hover`, `completion`, `definition_type_name`, `definition_field_name`, `definition_ref_key`。
- URI/diagnostic helpers：`path_from_file_uri`, `path_to_file_uri`, `lsp_diagnostic`, `lsp_label_location`, `preferred_diagnostic_uri`。

### 6.18 `coflow-project`

核心结构：

- 单文件 project crate，包含 config serde、path resolution、project open/init、schema discovery/build、diagnostic conversion。

核心类型：

- config：`ProjectConfig`, `SchemaConfig`, `SourceConfig`, `OutputsConfig`, `OutputConfig`, `DimensionConfig`。
- project：`Project`, `SchemaFile`, `SchemaBuild`, `SchemaSourceOverride`, `InitOutcome`。
- internal serde/diagnostic helpers：`NoDuplicateValue`, `ProjectDiagnostic`, `Range`, `Position`。

核心入口：

- `Project::open`, `Project::open_schema_only`。
- `Project::validate_for_data`, `validate_for_codegen`。
- `Project::schema_diagnostic_set/data_diagnostic_set/codegen_diagnostic_set`。
- `Project::resolve_path`, `schema_files`。
- `init_project`。
- `compile_schema_project`, `compile_schema_project_with_overrides`。
- `resolve_config_path`, `dedupe_cft_diagnostics`, `diagnostic_set_from_cft`。
- `path_to_slash`, `normalize_path`。

## 7. Summary judgment

当前架构已经有清晰的 crate 轮廓：语言、数据模型、provider、engine、host 并不是混在一个大 crate 中，这是基础优势。但实际实现中，`coflow-engine` 仍承担过多编排和具体格式知识，`build_project_session` 不是纯构建，语义 view 在多个 crate 重复，几个关键模块体量过大。这些问题不是单个 bug，而是会持续放大维护成本的结构性缺点。

最应优先处理的是两件事：一是让 session build 只读、可预测；二是把 engine 中硬编码 provider 的 create/sync/write 逻辑推回 provider capability。随后再处理共享语义 facade 和大模块拆分，才能让后续功能增长不继续堆到当前热点文件里。
