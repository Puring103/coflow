# CFD-only 运行时重构计划

## 1. 决策与边界

本次重构直接切换到新的产品模型，不保留旧项目、旧配置、旧 API、旧导出格式或迁移层。旧文档不作为兼容依据；产品文档只描述重构后的行为。

最终规则只有三条：

1. `.cfd` 是唯一数据输入和编辑格式。`coflow.yaml` 的 `data` 只接受 CFD 文件或目录，目录递归发现 `.cfd`。
2. 数据导出全部移除。构建发布的唯一产物是目标语言代码；目标语言可以继续扩展，不把 C# 写死在 runtime 中。
3. C# 目标直接加载原始 CFD 文本。生成代码和 `Coflow.Cfd.Runtime` 共同完成解析、类型构造、引用解析和诊断，不经过 JSON、MessagePack 或通用导出模型。

不考虑以下事项：旧配置字段兼容、旧 provider 名称兼容、导出文件迁移、旧 C# materializer 兼容、旧 crate 的 re-export wrapper、渐进式双架构运行。

## 2. 最终 workspace

最终 workspace 只保留承担明确职责的 crate：

| crate | 职责 | 允许依赖 |
| --- | --- | --- |
| `coflow-language` | CFT schema、CFD 语法、诊断、结构限制 | parser/schema 基础依赖 |
| `coflow-runtime` | 项目配置、CFD 解析加载、数据模型、校验、查询、CFD 原地写入、代码生成输入 | `coflow-language` |
| `coflow-codegen-api` | 多语言生成器描述、生成输入、代码 artifact、生成错误 | `coflow-runtime` |
| `coflow-codegen-csharp` | C# 类型降低、显式 CFD materializer 代码生成 | `coflow-codegen-api`、`coflow-runtime` |
| `coflow-lsp` | CFT/CFD 语言服务 | `coflow-runtime` |
| `cfd-editor` | Tauri host、编辑器 wire DTO、前端交互 | `coflow-runtime`、`coflow-codegen-api` |

以下 crate 已合并或删除，不得以空壳、兼容别名或 feature 形式重新出现：

- `coflow-cft`、`coflow-cfd`、`coflow-structure`：实现并入 `coflow-language`。
- `coflow-data-model`、`coflow-checker`：实现并入 `coflow-runtime`。
- 所有 CSV/Excel/table loader、通用 loader core、JSON/MessagePack exporter、exporter core。
- `coflow-api`、`coflow-project`、`coflow-builtins`：接口按职责并入 runtime 或 codegen-api；不保留通用 provider registry。

删除 crate 时同时删除 Cargo feature、测试 fixture、bench、示例、CI 矩阵和网站导航条目。`cargo tree --workspace` 不得出现已删除名称。

## 3. 目标架构

```text
coflow.yaml
  ├─ schema -> coflow-language::CftSchema
  ├─ data   -> coflow-runtime::CfdSourceCatalog -> CfdDocument -> CfdDataModel
  └─ codegen[] -> CodegenRegistry -> target generator -> generated source files

CFD editor write: MutationRequest -> CfdWritePlan -> atomic CFD patch -> reload generation
C# runtime: ICfdTextLoader -> CfdParser -> CfdDocument -> generated Read<Type> -> user model
```

runtime 内部不再根据扩展名竞争 provider，不再解码 provider options，不再探测来源格式。只有一个固定的 CFD reader/writer；未来扩展点是新的目标语言 generator。

### 3.1 配置边界

Rust 配置模型：

```rust
pub struct ProjectConfig {
    pub schema: SchemaConfig,
    pub data: Vec<CfdPath>,
    pub codegen: Vec<CodegenTargetConfig>,
    pub dimensions: BTreeMap<String, DimensionConfig>,
}

pub struct CfdPath {
    pub path: PathBuf,
}

pub struct CodegenTargetConfig {
    pub language: String,
    pub dir: PathBuf,
    pub options: serde_json::Value,
}
```

`data` 的字符串是文件或目录路径；目录只产生 `.cfd` 输入。配置解析遇到 `source_type`、provider options、`outputs.data`、export target 等字段直接报错。

### 3.2 Rust CFD 数据结构

语法和语义边界使用 source-neutral 但 CFD 专用的结构：

```rust
pub struct CfdDocument {
    pub path: PathBuf,
    pub records: Vec<CfdRecordNode>,
}

pub struct CfdRecordNode {
    pub key: RecordKey,
    pub declared_type: TypeName,
    pub fields: Vec<CfdFieldNode>,
    pub span: TextSpan,
}

pub enum CfdValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Enum(VariantName),
    Ref(RecordKey),
    Object(BTreeMap<FieldName, CfdValue>),
    Array(Vec<CfdValue>),
    Dict(Vec<(CfdValue, CfdValue)>),
}

pub struct CfdSourceEntry {
    pub path: PathBuf,
    pub document: CfdDocument,
    pub records: Range<usize>,
}

pub struct CfdSourceCatalog {
    pub entries: Vec<CfdSourceEntry>,
    pub by_path: BTreeMap<PathBuf, SourceId>,
    pub by_record: HashMap<(TypeName, RecordKey), RecordId>,
}
```

`CfdSourceCatalog` 是具体数据索引，不是可注册的 SPI。它负责固定 CFD 文件集合、源位置、记录 identity 和反向引用查询。所有成功加载的记录必须带 `RecordOrigin::File { path, span }`；错误不得伪造 artifact 或其它格式来源。

#### 数据不变量

- `SourceId` 只由规范化后的项目相对路径产生；同一路径只能有一个 source entry。
- `RecordId` 在一次 generation 内稳定，定义为 `(SourceId, record_index)`；重新加载后通过
  `(declared_type, key)` 建立新的引用索引，不复用旧 generation 的整数 id。
- `RecordOrigin` 必须保存原始文件路径和字节 span；parser、lowerer、checker 和 writer
  的诊断都从这个 origin 生成位置，不在下游重新猜测文件。
- `CfdValue::Ref` 只保存目标 key，目标类型来自 schema 字段；解析器不接受把类型编码到引用
  文本中的第二套语法。
- `CfdDataModel` 构建分为 draft、语义校验、索引发布三步。任一步出现错误都只返回
  `DiagnosticSet`，不能发布半成品 generation。

建议固定以下内部类型，避免继续使用含义宽泛的 `String`：

```rust
pub struct SourceId(pub u32);
pub struct RecordId(pub u32);
pub struct FileRevision {
    pub size: u64,
    pub modified_ns: u128,
    pub content_hash: [u8; 32],
}

pub struct CfdDiagnostic {
    pub code: CfdErrorCode,
    pub stage: CfdStage,
    pub origin: RecordOrigin,
    pub path: CfdPath,
    pub message: String,
}
```

`FileRevision` 是写入计划的乐观锁；只要 size、mtime 或 hash 与计划不一致，commit 必须返回
`CFD-WRITE-STALE`，不能覆盖外部修改。

### 3.3 runtime 服务接口

```rust
pub struct Runtime {
    cfd: CfdReader,
    writer: CfdWriter,
}

impl Runtime {
    pub fn new() -> Self;
    pub fn open_read_only(&self, project: Project)
        -> Result<ReadOnlyProjectSession, DiagnosticSet>;
    pub fn open_write(&self, project: Project)
        -> Result<WriteProjectSession, DiagnosticSet>;
}

pub struct ReadOnlyProjectSession { /* immutable schema + model + indexes */ }
pub struct WriteProjectSession { /* generation + CfdWritePlan + revision */ }

pub struct ProjectQueries<'a> {
    pub fn source_files(self) -> impl Iterator<Item = &'a str>;
    pub fn record_views_in_file(self, file: &str) -> impl Iterator<Item = RecordView<'a>>;
    pub fn field_value(self, coordinate: &RecordCoordinate,
                       path: &[CfdPathSegment]) -> Option<&'a CfdValue>;
}

pub struct MutationRequest { pub operations: Vec<MutationOp> }
pub struct MutationReport {
    pub generation_changed: bool,
    pub diagnostics: DiagnosticSet,
}

pub enum MutationOp {
    SetField { record: RecordCoordinate, path: CfdPath, value: CfdValue },
    InsertRecord { source: SourceId, record: CfdRecordDraft },
    DeleteRecord { record: RecordCoordinate },
    RenameRecord { record: RecordCoordinate, new_key: RecordKey },
    ReorderRecords { source: SourceId, order: Vec<RecordId> },
}
```

`Runtime::new()` 是唯一公开构造入口。host 不持有 catalog，不传递 provider id，不注册 writer；编辑器的 session 只保存 `WriteProjectSession`。

### 3.4 写入接口

CFD writer 直接消费记录源位置和 mutation plan：

```rust
pub struct CfdWritePlan {
    pub path: PathBuf,
    pub patches: Vec<CfdTextPatch>,
    pub expected_revision: FileRevision,
}

pub struct CfdTextPatch {
    pub span: TextSpan,
    pub replacement: String,
}

pub trait CfdDocumentWriter {
    fn plan(&self, model: &CfdDataModel, request: &MutationRequest)
        -> Result<CfdWritePlan, DiagnosticSet>;
    fn commit(&self, plan: CfdWritePlan) -> Result<FileRevision, DiagnosticSet>;
}
```

这里的 trait 是 runtime 内部的文件写入测试边界，不是第三方格式扩展点。提交必须执行临时文件写入、fsync、原子替换和重新解析；失败时不发布新 generation。

写入执行顺序固定为：`validate request -> resolve origin -> build patches -> check revision ->
write temp -> fsync -> atomic replace -> parse/lower -> publish generation`。任何后一步失败都
删除临时文件并保留当前 generation；批量 mutation 不允许部分提交。

## 4. 多语言代码生成

`coflow-codegen-api` 只表达目标语言能力：

```rust
pub struct CodegenInput<'a> {
    pub schema: &'a CftSchema,
    pub model: Option<&'a CfdDataModel>,
    pub sources: &'a [CfdSourceDescriptor],
    pub target: &'a CodegenTarget,
}

pub trait CodeGenerator: Send + Sync {
    fn descriptor(&self) -> &'static CodegenDescriptor;
    fn generate(&self, input: CodegenInput<'_>) -> Result<CodeArtifactSet, CodegenError>;
}
```

每个 generator 自己决定生成文件和 runtime package。runtime 只负责收集 schema/model、调用 registry、校验输出目录重叠并发布代码文件；不包含任何语言模板。

registry 只按目标语言 id 做静态查找，不参与数据源解析：

```rust
pub struct CodegenRegistry {
    generators: BTreeMap<String, Arc<dyn CodeGenerator>>,
}

impl CodegenRegistry {
    pub fn register(&mut self, generator: Arc<dyn CodeGenerator>) -> Result<(), CodegenError>;
    pub fn get(&self, language: &str) -> Option<&dyn CodeGenerator>;
    pub fn generate_all(&self, input: CodegenInput<'_>) -> Result<CodeArtifactSet, CodegenError>;
}
```

`register` 拒绝重复 language id；`generate_all` 按配置顺序调用目标，合并文件前检查相对路径
冲突、目录重叠和目标 runtime 版本冲突。新增语言只新增 codegen crate 和注册项，不修改
CFD parser、runtime source resolution 或数据模型。

### C# 生成契约

`coflow-codegen-csharp` 为每个可实例化 CFT type 生成：

```csharp
private sealed class ItemCfdBinding : ICfdTypeBinding
{
    public string DeclaredType => "Item";
    public object Read(CfdRecordNode record, CfdLoadContext context) =>
        ReadItem(record, context);
}

private static Item ReadItem(CfdRecordNode record, CfdLoadContext context) =>
    new Item(record.Key,
        CfdValueReader.String(CfdValueReader.Field(record.Fields, "name")));
```

数组、字典、对象、nullable、枚举和引用均生成显式 reader lambda；生成代码不调用 `Activator`、`ConstructorInfo` 或按字段反射。抽象类型不生成 binding，未找到具体声明类型时返回稳定诊断。

## 5. C# 运行时接口

`Coflow.Cfd.Runtime` 只负责 schema-free CFD 语法、限制、source loading、identity/reference context 和基础值读取：

```csharp
public interface ICfdTextLoader
{
    bool TryLoad(string logicalPath, out string? text);
}

public sealed class CfdLoadContext
{
    public IReadOnlyList<CfdDocument> Documents { get; }
    public IReadOnlyDictionary<string, ICfdTypeBinding> Bindings { get; }
    public T Resolve<T>(string declaredType, string key);
}

public interface ICfdTypeBinding
{
    string DeclaredType { get; }
    object Read(CfdRecordNode record, CfdLoadContext context);
}

public static class CfdLoader
{
    public static IReadOnlyList<CfdDocument> LoadDocuments(
        ICfdTextLoader loader, IEnumerable<string> paths,
        CfdLoadOptions? options = null);
}

public static class CfdValueReader
{
    public static string String(CfdValueNode node);
    public static T Enum<T>(CfdValueNode node) where T : struct, Enum;
    public static T Reference<T>(CfdValueNode node, CfdLoadContext context,
                                 string declaredType);
    public static IReadOnlyList<T> Array<T>(CfdValueNode node, CfdLoadContext context,
        Func<CfdValueNode, CfdLoadContext, T> read);
}
```

`CfdLoadContext` 以 `(declaredType, key)` 做 identity cache，并检测循环引用。缺少文件、未知类型、缺少字段、类型不匹配、循环引用、深度/node/source-size 超限都返回 `CfdDiagnostic { code, path, span }`，不吞异常。

运行时加载的错误契约如下：

| 代码 | 触发条件 | 处理方式 |
| --- | --- | --- |
| `CFD-SOURCE-MISSING` | `ICfdTextLoader` 无法提供清单中的文件 | 聚合所有缺失文件后抛出一次 `CfdLoadException` |
| `CFD-PARSE-*` | token、字符串、key 或结构错误 | 保留 UTF-16 span，停止当前 document |
| `CFD-REF-MISSING` | 引用 key 不存在 | 记录引用字段 span，继续读取其它独立记录 |
| `CFD-REF-CYCLE` | identity 正在 resolving 时再次进入 | 报告完整 identity 链，不返回部分对象 |
| `CFD-VALUE-TYPE` | 生成 reader 与节点类型不匹配 | 报告字段 span 和期望类型 |
| `CFD-LIMIT-*` | 深度、节点数或源大小超限 | 在达到上限的节点处失败，避免无界分配 |

`CfdLoadContext.Resolve` 对已完成 identity 返回同一个对象实例；对 resolving identity 只报错
不缓存异常对象。生成 binding 不得自行扫描其它 document 或自行维护第二份 cache。

## 6. 删除与合并清单

### Rust

1. 删除通用 provider trait、catalog registration、source option decoding、probe confidence、provider descriptor 和动态选择错误。
2. 删除 `data_files`、`data_read`、table/sheet/header/export 命令路径。
3. 将 CFD parser/lower/writer 归并到 runtime，公共入口仅保留 `parse_cfd_input_records`、`load_cfd_model`、`CfdDocumentWriter`。
4. 将 data model/checker 的测试迁入 runtime tests；删除旧 crate 的 Cargo manifest 和 bench。
5. `Runtime::with_catalog`、`cfd_source_catalog()`、editor registry 字段全部删除。

### C#、文档和工具

1. 删除反射 materializer 及其测试；生成器 fixture 必须检查生成文本不含 `Activator`、`System.Reflection`、JSON/MessagePack。
2. 删除 export CLI、export flags、export docs、JSON/MessagePack fixtures 和网站首页中的非 CFD 示例。
3. 更新 README、website reference、skills reference、examples 和架构图；不添加迁移章节。
4. 历史 release note 可以保留，但不能出现在当前导航、安装、配置或 API 文档中。

## 7. 分阶段实施与验收

### 阶段 A：workspace 收敛

- 完成 crate 合并和 Cargo.lock 清理。
- `rg` 不得找到旧 crate 名称或 exporter 依赖。
- 通过 `cargo check --workspace`。
- 输出：新的 workspace 拓扑和依赖边界清单；删除旧 manifest、feature、bench、fixture。
- 完成条件：`cargo tree --workspace` 只出现最终 crate，任何 host 都只能依赖 runtime/codegen-api。

### 阶段 B：固定 CFD runtime

- 配置只解析 CFD path；固定目录发现和 `.cfd` 诊断。
- 移除 catalog/provider host API，编辑器改用 `Runtime::new()`。
- 补充单文件、多文件、目录、重复记录、非法扩展和 source span 测试。
- 输出：`CfdSourceCatalog`、`CfdDocument`、`CfdDataModel` 的构建入口和统一 diagnostics。
- 完成条件：配置中出现 provider/source_type/options/export 字段时直接失败；目录只发现 `.cfd`。

### 阶段 C：写入与 generation

- mutation 只生成 `CfdWritePlan`，原子提交后重新加载。
- 验证批量字段、插入、删除、重命名、重排、引用反向更新和并发 revision 冲突。
- 输出：`CfdWritePlan`/`CfdTextPatch` 测试夹具，覆盖原子替换失败和 stale revision。
- 完成条件：无部分提交；重新解析后的 generation 与写入文本一致。

### 阶段 D：C# direct load

- 完成 `ICfdTextLoader`、`CfdLoadContext`、`ICfdTypeBinding`、`CfdValueReader`。
- C# generator 为 primitive/object/array/dict/nullable/enum/ref 生成显式代码。
- 端到端 fixture：生成 C#、载入多份 CFD、前向引用、循环引用诊断和缺失字段诊断。
- 输出：不含 `Activator`、`System.Reflection`、JSON/MessagePack 的生成源码快照。
- 完成条件：仅通过 `ICfdTextLoader` 和 `SourceFiles` 清单即可加载；binding 不依赖 CFT 文件。

### 阶段 E：产品文档与清理

- 当前文档只描述 `data` + `codegen`，不描述 provider、export、table/sheet。
- 删除无效示例和死链接，运行文档搜索审计。
- 输出：README、website reference、架构图和本计划中的同一术语表；不添加迁移章节。
- 完成条件：产品文档搜索不到旧数据源、导出命令和 provider 配置示例。

## 8. 必须执行的验证

普通开发检查：

```text
cargo check --workspace
cargo test --workspace
```

生成绑定和前端检查：

```text
cargo test --features ts-export -p cfd-editor export_bindings
git diff --exit-code editors/cfd-editor/frontend/src/bindings
npm --prefix editors/cfd-editor/frontend test
npm --prefix editors/cfd-editor/frontend run build
node editors/vscode-coflow/test/extension-unit.test.js
```

C# 环境可用时执行：

```text
dotnet test runtimes/csharp/Coflow.Cfd.Runtime.Tests/Coflow.Cfd.Runtime.Tests.csproj
```

最终静态审计：

```text
rg -n "ProviderRegistry|SourceProvider|DataExporter|MessagePack|coflow export|CSV|Excel|Activator|System.Reflection" .
cargo tree --workspace
git diff --check
```

验收标准是：workspace 无旧数据源/导出 crate；runtime 只有固定 CFD 数据通路；代码生成仍可注册多语言目标；C# 生成代码可直接通过 loader 读取 CFD；Rust 检查、生成绑定检查和可用环境下的 C# 测试全部通过。
