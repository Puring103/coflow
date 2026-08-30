# CFD-only 运行时重构计划

## 1. 目标和边界

本次重构直接切换到新架构，不提供旧配置、旧 crate、旧导出格式或旧 API 的迁移层。

- 输入只有 CFT schema 和 CFD 文本源；数据源选择、provider 注册和格式探测全部删除。
- 产品输出只有目标语言代码；JSON、MessagePack、表格文件和通用 `export` 命令全部删除。
- 代码生成仍然允许多个目标语言。每种语言由独立 generator 和对应的目标语言 runtime
  组成，但所有 generator 共享同一个只读 schema/model 快照。
- C# 应用通过 `Coflow.Cfd.Runtime` 直接读取 CFD 文件。生成的 C# binding 负责类型构造，
  runtime 负责词法/语法、source 清单、引用缓存和诊断；中间不产生 JSON/MessagePack。
- 编辑器、LSP、CLI 使用同一个 Rust runtime；宿主不能自行选择 provider 或复制一套数据模型。

非目标：不保留旧格式兼容，不增加格式转换工具，不迁移历史文档，不维护第二套导出对象模型。

## 2. 最终模块拓扑

工作区只保留 5 个 Rust package（根应用也计为一个 package）以及一个独立的 C# runtime：

| package | 保留职责 | 明确删除 |
| --- | --- | --- |
| `coflow-language` | CFT schema、CFD 值语法、AST、span、结构限制 | 独立 `coflow-cft`、`coflow-cfd`、`coflow-structure` |
| `coflow-runtime` | 项目配置、固定 CFD source resolve/load/write、数据模型、检查、查询、诊断、codegen SPI | `coflow-api`、`coflow-project`、`coflow-data-model`、`coflow-checker`、provider/catalog crate |
| `coflow-codegen-csharp` | C# 声明、显式 reader、C# 目标描述 | 反射 materializer、JSON/MessagePack writer |
| `coflow` | CLI、命令编排、LSP、代码生成 staging/发布、应用级错误输出 | 独立 `coflow-lsp`、所有 `export` 子命令 |
| `cfd-editor` | Tauri backend、编辑器 DTO、图/表视图、写入桥接 | 独立 `extension-api`、编辑器 provider registry |
| `Coflow.Cfd.Runtime` | C# schema-free CFD parser、source loader、binding context、值读取 | C# JSON/MessagePack loader、反射和构造函数发现 |

依赖方向固定为：

```text
coflow-language <- coflow-runtime <- coflow / cfd-editor
                         ^               ^
                         |               +-- coflow-codegen-csharp
                         +-- codegen SPI (runtime::codegen)
```

`coflow-runtime` 不依赖具体 generator；根应用显式注册内置 generator。C# runtime 不读取
CFT 文件，也不依赖 Rust 或生成器。

## 3. 项目配置和数据流

`coflow.yaml` 只允许 `schema`、`data` 和 `codegen`：

```yaml
schema: schema/
data: data/
codegen:
  - language: csharp
    dir: generated/csharp
    namespace: Game.Config
```

`data` 可以是 CFD 文件或目录；目录递归发现 `.cfd`，其它扩展名忽略。`codegen` 目标按
配置顺序生成，但所有目标在发布前必须完成。数据阶段顺序固定为：

```text
读取 coflow.yaml
  -> 发现 schema 和 *.cfd
  -> 编译 CFT
  -> 解析/降低 CFD
  -> 发布不可变 CfdDataModel + indexes
  -> check 或调用 generator
  -> 所有目标写 staging
  -> 一次性备份/原子发布代码目录
```

任何 schema、source、generator 或发布错误都不得留下部分新代码目录；已有目录必须恢复。

## 4. 核心 Rust 数据结构

以下类型是跨 CLI、编辑器和 LSP 的唯一语义边界，字段名可以继续细化，但不能再退回
`String + provider id` 的弱类型接口。

### 4.1 Source 和记录身份

```rust
pub struct SourceId(pub u32);
pub struct RecordId(pub u32);

pub struct CfdSource {
    pub id: SourceId,
    pub logical_path: String,       // 项目相对路径，使用 /
    pub text: String,
    pub revision: FileRevision,
}

pub struct FileRevision {
    pub size: u64,
    pub modified_ns: u128,
    pub content_hash: [u8; 32],
}

pub struct RecordCoordinate {
    pub source: SourceId,
    pub record: RecordId,
    pub declared_type: TypeName,
    pub key: RecordKey,
}
```

`SourceId` 只在一次 runtime generation 内有效；写入计划必须携带 `FileRevision`，发现
文件被外部修改时返回 `CFD-WRITE-STALE`，不能覆盖新内容。

### 4.2 数据模型和值

`CfdDataModel` 是 schema-guided、不可变、可查询的发布快照：

```rust
pub struct CfdDataModel {
    pub sources: Arc<[CfdSourceInfo]>,
    pub records: Arc<[CfdRecord]>,
    pub values: Arc<[CfdValue]>,
    pub source_index: SourceIndex,
    pub record_index: RecordIndex,
    pub ref_edges: Arc<[RefEdge]>,
}

pub struct CfdRecord {
    pub coordinate: RecordCoordinate,
    pub origin: RecordOrigin,
    pub fields: Arc<[CfdField]>,
}

pub enum CfdValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(CfdFormattedString),
    Function(CfdFunction),
    Enum(CfdEnumValue),
    RecordRef { expected: TypeName, key: RecordKey },
    Object { ty: TypeName, fields: Arc<[CfdValue]> },
    Array(Arc<[CfdValue]>),
    Dict(Arc<[(CfdDictKey, CfdValue)]>),
}
```

parser 只负责文本结构；`CfdModelBuilder` 执行默认值、类型、enum、ref、object 和
dimension 语义校验，再一次性发布 model。失败时返回 `DiagnosticSet`，不暴露半成品索引。
函数字段不是数据加载例外：CFD parser 保留完整函数源码和 span，lowering 必须校验参数与返回
签名并把 `CfdFunction` 发布到 model；项目的 `data` 不能通过排除含函数的 CFD 文件规避检查。

### 4.3 诊断

```rust
pub struct CfdDiagnostic {
    pub code: CfdErrorCode,
    pub stage: CfdStage,
    pub origin: RecordOrigin,
    pub path: CfdPath,
    pub message: String,
}

pub struct DiagnosticSet {
    pub diagnostics: Vec<Diagnostic>,
}
```

每条诊断必须能定位到项目配置 key、CFD source 和文本 span；禁止用字符串拼接吞掉
source origin。错误阶段至少包括 `PROJECT`、`SCHEMA`、`PARSE`、`MODEL`、`CHECK`、`WRITE`
和 `CODEGEN`。

## 5. Rust runtime 服务和写入接口

```rust
pub struct Runtime;

impl Runtime {
    pub fn new() -> Self;
    pub fn open_read_only_session(
        &self, project: Project,
    ) -> Result<ReadOnlyProjectSession, DiagnosticSet>;
    pub fn open_write_session(
        &self, project: Project,
    ) -> Result<WriteProjectSession, DiagnosticSet>;
}

pub struct ReadOnlyProjectSession {
    pub fn schema(&self) -> &CftSchema;
    pub fn model(&self) -> &CfdDataModel;
    pub fn queries(&self) -> ProjectQueries<'_>;
}

pub struct ProjectQueries<'a> {
    pub fn source_files(self) -> impl Iterator<Item = &'a str>;
    pub fn record_views_in_file(self, path: &str) -> impl Iterator<Item = RecordView<'a>>;
    pub fn field_value(
        self, coordinate: &RecordCoordinate, path: &[CfdPathSegment],
    ) -> Option<&'a CfdValue>;
}
```

写入路径只接受语义 mutation，不接受任意文件格式：

```rust
pub struct MutationRequest { pub operations: Vec<MutationOp> }

pub enum MutationOp {
    SetField { record: RecordCoordinate, path: CfdPath, value: CfdValue },
    InsertRecord { source: SourceId, record: CfdRecordDraft },
    DeleteRecord { record: RecordCoordinate },
    RenameRecord { record: RecordCoordinate, new_key: RecordKey },
    ReorderRecords { source: SourceId, order: Vec<RecordId> },
}

pub struct CfdWritePlan {
    pub path: PathBuf,
    pub patches: Vec<CfdTextPatch>,
    pub expected_revision: FileRevision,
}

pub trait CfdDocumentWriter {
    fn plan(&self, request: &MutationRequest)
        -> Result<CfdWritePlan, DiagnosticSet>;
    fn commit(&self, plan: CfdWritePlan)
        -> Result<FileRevision, DiagnosticSet>;
}
```

提交顺序固定为 `validate -> resolve origin -> build patches -> check revision -> write temp /
fsync -> atomic replace -> reparse/lower -> publish generation`。批量 mutation 任何一步失败都
回滚，不允许部分提交。

## 6. 多语言代码生成 SPI

SPI 已并入 `coflow-runtime::codegen`，不是独立 package：

```rust
pub struct CodegenTarget {
    pub id: String,
    pub output_dir: PathBuf,
    pub options: serde_json::Value,
}

pub struct CodegenInput<'a> {
    pub schema: &'a CftSchema,
    pub model: Option<&'a CfdDataModel>,
    pub sources: &'a [SourceManifestEntry],
    pub target: &'a CodegenTarget,
    pub id_as_enum_lock: &'a serde_json::Value,
}

pub trait CodeGenerator: Send + Sync + std::fmt::Debug {
    fn descriptor(&self) -> &'static CodegenDescriptor;
    fn generate(&self, input: CodegenInput<'_>)
        -> Result<CodeArtifactSet, CodegenError>;
}

pub struct CodeArtifactSet { /* 已排序、无重复、仅项目内相对路径 */ }
```

`CodeArtifactSet::new` 拒绝绝对路径、`..`、空路径和重复文件。registry 只负责 language id
查找和重复注册检测，不读取 source、不写文件、不参与 model 构造。新增语言只需新增
generator package 和根应用注册项。

根应用的代码生成事务：

1. 校验所有 `codegen[].dir` 的规范化路径，拒绝相同或祖先/后代目录。
2. 顺序调用所有 generator，把 `PendingCodegen` 保存在内存。
3. 所有目标分别写入唯一 staging 目录，校验 artifact 相对路径。
4. 统一把旧目录移动到 backup，再逐个 rename staging 到目标目录。
5. 任一 backup/publish 失败时删除已发布的新目录，按逆序恢复 backup。
6. 成功后清理 backup，返回所有 `CodegenReport`。

发布还要写入 `.coflow/artifacts/active.json` 并保留 generation history。`build --status` 只在
内存生成并比较活动输出，不得发布；`clean` 只删除非活动 generation 和遗留 staging。输出安全
检查必须解析符号链接，并拒绝项目根、配置、schema、data、维度目录和输出之间的重叠。

## 7. C# direct-load runtime 契约

`Coflow.Cfd.Runtime` 面向 `netstandard2.1` 和 `net8.0`，只包含 schema-free CFD 语法和
binding 执行：

```csharp
public readonly struct CfdSource(string Path, string Text);
public sealed class CfdDocument(string Path, IReadOnlyList<CfdRecordNode> Records);
public interface ICfdTextLoader {
    bool TryLoad(string logicalPath, out string? text);
}
public interface ICfdTypeBinding {
    string DeclaredType { get; }
    IReadOnlyList<string> AssignableTypes { get; }
    string? ObjectFieldType(string fieldName);
    string? ReferenceFieldType(string fieldName);
    object Read(CfdRecordNode record, CfdLoadContext context);
}
public sealed class CfdLoadContext {
    public IReadOnlyList<CfdDocument> Documents { get; }
    public IReadOnlyDictionary<string, ICfdTypeBinding> Bindings { get; }
    public T Resolve<T>(string declaredType, string key);
}
public static class CfdLoader {
    public static IReadOnlyList<CfdDocument> LoadDocuments(
        ICfdTextLoader loader, IEnumerable<string> paths,
        CfdLoadOptions? options = null);
}
```

`CfdValueReader` 提供 `String`、`Int32`、`Int64`、`Float32`、`Float64`、`Boolean`、`Enum`、
`Reference`、`Object`、`Array`、`Dictionary`、`Field`、`FindField`、`ValidateFields` 和 `Nullable`。
所有 reader 把格式错误转换为 `CfdLoadException`，不泄漏 `FormatException`、
`OverflowException` 或 `ArgumentException`。

binding 的 `AssignableTypes` 包含 concrete type 自身及全部 ancestor。identity cache 的 key 是
`(DeclaredType, Key)`；构造 context 时拒绝同一继承域跨 document 重复 identity，
resolve 时检测 resolving 栈，缺失目标返回 `CFD-REF-MISSING`，循环返回 `CFD-REF-CYCLE`。
parser 在 `CfdLoadOptions` 中限制 source bytes、records、nodes 和 depth，并为每个错误保留
UTF-16 `CfdSpan`。

C# generator 必须为每个 concrete type 生成 `ICfdTypeBinding`、source-name/ancestor 映射、显式构造函数调用和
`Read<Type>Fields`；抽象 type 只生成多态 dispatch。生成的数据库类暴露：

```csharp
public static IReadOnlyList<string> SourceFiles { get; }
public static CoflowTables Load(ICfdTextLoader loader, CfdLoadOptions? options = null);
public static CoflowTables Load(Func<string, string?> loadText, CfdLoadOptions? options = null);
public static CoflowTables Load(IEnumerable<CfdSource> sources, CfdLoadOptions? options = null);
```

生成源码不得出现 `Activator`、`System.Reflection`、JSON 或 MessagePack；C# 真实编译必须
覆盖 abstract/polymorphic、前向 ref、inline object、array/dictionary、nullable 和 enum。

## 8. 删除和合并清单

### Rust 和 CLI

- 删除 `coflow-codegen-api`，把 SPI 移入 `coflow-runtime::codegen`。
- 删除 `coflow-lsp`，把 LSP 模块移入根 crate 的 `src/lsp/`。
- 删除 `extension-api`，编辑器 backend 内置 `ExtensionManifest`。
- 删除 provider trait、catalog、source option decode、probe confidence、动态 source type。
- 删除 `loader_extensions`、`extra_extensions` 等格式扩展点；文件树和目录发现直接以 `.cfd`
  为唯一规则，维度目录只作为 runtime 管理的 CFD 子树处理。
- 删除 CSV/Excel/table/sheet loader、通用 loader core、JSON/MessagePack exporter 和 exporter core。
- 删除 `export` 命令、export flags、serialized artifact DTO、旧 C# materializer。
- 清理 Cargo features、workspace members、bench、fixture 和依赖锁文件。

### 文档和示例

- `README`、website reference、程序员指南和架构文档只描述 `schema + data(.cfd) + codegen`。
- 删除 provider/export/API 入口、JSON/MessagePack 示例和非 CFD 配置片段。
- 不写迁移章节；历史 release notes 可以保留为历史记录，但不得被当前导航引用。

## 9. 分阶段实施和完成条件

### 阶段 A：workspace 收敛

- 删除旧 manifest 和依赖，workspace 只出现最终 package。
- `cargo tree --workspace` 不含旧 provider、loader、exporter、独立 codegen-api/LSP。
- 完成条件：`cargo check --workspace`、`cargo test --workspace` 通过。

### 阶段 B：固定 CFD runtime

- source resolve 只接受 `.cfd`，目录发现、span、重复 identity 和 limits 统一进入 runtime。
- editor、LSP、CLI 都通过 `Runtime::new()` 打开 session。
- 完成条件：非 CFD 扩展、provider 字段、export 字段均产生配置诊断；目录只加载 `.cfd`。

### 阶段 C：写入和 generation

- mutation 生成 `CfdWritePlan`，执行 revision 检查、原子替换和重新 lower。
- 覆盖批量 set/insert/delete/rename/reorder、引用影响和 stale revision。
- 完成条件：写入失败没有部分提交，generation 与文本重新解析结果一致。

### 阶段 D：C# direct load

- 完成 parser、loader、context、binding、value reader 和诊断码。
- 生成器输出 source list、binding、typed readers 和顶层 `Load`。
- 完成条件：runtime 双目标编译；生成示例通过真实 .NET 编译；负面语义测试通过。

### 阶段 E：文档和静态审计

- 删除无效文档、死链接和过时示例，统一术语 `CFD source`、`CfdDataModel`、`CodeArtifactSet`。
- 完成条件：当前文档不出现 provider/export 配置；搜索只剩历史 release notes 中的历史名称。

## 10. 验证命令

普通 Rust 检查：

```text
cargo check --workspace
cargo test --workspace
```

发布前额外检查：

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --features ts-export -p cfd-editor export_bindings
npm --prefix editors/cfd-editor/frontend test
npm --prefix editors/cfd-editor/frontend run build
node editors/vscode-coflow/test/extension-unit.test.js
git diff --check
cargo tree --workspace
```

C# 检查（使用已安装 SDK 时）：

```text
DOTNET_ROOT=/home/wtl/.dotnet DOTNET_ROLL_FORWARD=LatestMajor \
  /home/wtl/.dotnet/dotnet test runtimes/csharp/Coflow.Cfd.Runtime.Tests/Coflow.Cfd.Runtime.Tests.csproj
```

生成集成夹具后，必须把 `tests/csharp-runtime-integration/generated/**/*.cs` 与 runtime 一起执行真实
`dotnet build`。静态审计：

```text
rg -n "ProviderRegistry|SourceProvider|DataExporter|MessagePack|coflow export|CSV|Excel|Activator|System.Reflection|coflow-codegen-api|coflow-lsp|extension-api" . --glob '!target/**' --glob '!**/bin/**' --glob '!**/obj/**'
```

验收结果必须同时满足：固定 CFD 数据通路、无导出 API、多语言 codegen SPI 可注册、C# 可
直接加载 CFD、所有 Rust/前端检查通过、C# runtime 测试和生成源码编译通过。
