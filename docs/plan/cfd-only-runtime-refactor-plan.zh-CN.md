# CFD-only Runtime 重构计划

## 1. 决策摘要

本次重构把 Coflow 从“多数据源 + 统一导出 + 代码生成”的工具，改成“CFD 源文件 + schema 校验 + 多语言代码生成”的工具。

这是一次不兼容的架构替换，不提供迁移工具、不保留旧字段、不保留旧命令别名，也不维护新旧两套执行路径。旧项目需要按照新配置和新文档手工调整；旧产物不会被读取，旧 exporter、loader 和 provider API 不再属于产品合同。

最终数据流只有一条：

```text
coflow.yaml
    |
    v
CFT schema compiler
    |
    v
CFD file catalog -> CFD parser/lowerer -> CfdDataModel -> checker
                                                   |
                                                   v
                                            CodegenInput
                                                   |
                         +-------------------------+-------------------------+
                         |                                                   |
                 C# code generator                                  future language generator
                         |                                                   |
                         v                                                   v
                 generated/*.cs                                    generated/<language>/*
                         |
                         v
                 Coflow.Cfd.Runtime
                         |
                         v
                    *.cfd at game/runtime load time
```

明确不再存在的路径：

- Excel、CSV、表格目录源、任意自定义 source provider。
- JSON、MessagePack 或其他中间数据导出。
- 运行时读取生成的 JSON/MessagePack 的语言 loader。
- 通过 provider registry 选择数据源、writer、table manager 或 exporter。
- 为旧 `sources`、`outputs.data`、`coflow export` 保留的兼容层。

代码生成仍然是多语言扩展点。CFD 是唯一输入格式；每个目标语言负责生成声明和调用该语言 CFD runtime 的绑定代码。C# 是本次首先完成的目标，不把 C# 特有的类型规则泄漏到 Rust runtime 或通用 codegen API。

## 2. 最终 workspace 与 crate 归属

### 2.1 保留的 crate

| crate | 最终职责 | 不允许放入的内容 |
| --- | --- | --- |
| `coflow-language` | 语言层 facade 和共享语法接口 | 项目路径、文件 IO、代码生成、运行时对象 |
| `coflow-cft` / `coflow-cfd` / `coflow-structure` | CFT schema/compiler、CFD parser/AST、结构限制等稳定语言原语 | 项目配置、文件发现、writer、目标语言类型 |
| `coflow-data-model` | CFD lower 后的 source-neutral 类型值、记录表、引用索引、来源位置、数据诊断 | provider 选择、目标语言类型、文件写回 |
| `coflow-checker` | 无状态 schema/check 执行器、依赖读取和检查诊断 | 文件发现、source registry、codegen、editor wire DTO |
| `coflow-runtime` | 项目配置、路径解析、CFD 文件发现/读取、schema/data/check generation、诊断映射、codegen 编排和发布 | 语言模板、数据 exporter、provider SPI |
| `coflow-codegen-api` | 多语言代码生成合同、注册表、代码 artifact 安全校验 | source provider、data exporter、文件系统发布 |
| `coflow-codegen-csharp` | C# 声明、C# CFD binding、C# 选项和模板 | JSON/MessagePack loader、Rust 文件 IO |
| `coflow-lsp` | CFT/CFD 语言服务适配 | 自己实现一套 runtime 或 source 解析 |
| `coflow` | CLI、应用服务、代码产物 staging | provider/exporter registry |

`editors/cfd-editor/src-tauri` 和 frontend 继续保留，但只依赖 runtime 的 project/session API；表格和 spreadsheet 专用能力从 editor 中删除或改成 CFD record/file 视图。

### 2.2 一次性删除或合并

以下 crate 不进入最终 workspace：

- 删除 `coflow-api`。其中的 provider、writer、loader-generation、export、registry 模块全部删除；仍需要的主机诊断类型归入 `coflow-runtime::diagnostics`，代码生成类型归入 `coflow-codegen-api`。
- 删除 `coflow-project`，其配置解析、路径策略、schema 文件发现、项目初始化和配置诊断合并到 `coflow-runtime::{config,project,paths}`。
- 删除 `coflow-loader-cfd`，其 CFD 解析后处理、文件读取和 writer 中仍有用的只读部分合并到 `coflow-runtime::cfd`；本次不再保留通用 `SourceProvider` trait。
- 删除 `coflow-builtins`，CLI/runtime 直接构造固定的 CFD pipeline 和显式的 codegen registry。
- 删除已经失去用途的 `coflow-loader-csv`、`coflow-loader-excel`、`coflow-loader-table-core`、`coflow-exporter-core`、`coflow-exporter-json`、`coflow-exporter-messagepack`。
- `coflow-language` 只作为语言层 facade；`coflow-cft`、`coflow-cfd`、`coflow-structure` 当前仍是独立的低层实现 crate。只有在 API 稳定后才进行目录合并，不能为了减少 manifest 把编译器、解析器和结构限制强行耦合。

合并后的依赖方向必须是单向的：

```text
coflow-language
       |
coflow-data-model -> coflow-checker
       |                    |
       +-------------> coflow-runtime <----- coflow-codegen-api
                              |
                    coflow-codegen-csharp
                              |
                         coflow / LSP / editor
```

`coflow-codegen-api` 可以依赖 language 和 data-model 的只读类型，但 language、data-model、checker 不得反向依赖任何目标语言 crate。

## 3. 项目配置新合同

### 3.1 配置形状

删除 `sources`、`outputs`、`outputs.data`、`outputs.loader` 和所有 `type: csv/excel/json/messagepack` 字段。新配置只描述 schema、CFD 数据位置、维度和代码目标：

```yaml
schema: schema/

data:
  - data/
  - overlays/base.cfd

dimensions:
  language:
    variants: [en, zh]
    data:
      en: data/lang/en/
      zh: data/lang/zh/

codegen:
  - language: csharp
    dir: generated/csharp
    options:
      namespace: Game.Config
      database_class: GameConfig
```

`data` 只接受路径字符串或路径列表；列表项不再有 `source_type`、`options` 或任意 provider 字段。目录只递归发现 `.cfd`，文件扩展名不符直接给配置诊断。`codegen` 是唯一产物列表，每个目标只有 `language`、`dir` 和目标语言 options。

### 3.2 Rust 配置类型

```rust
pub struct ProjectConfig {
    pub schema: SchemaSpec,
    pub data: CfdPathSet,
    pub dimensions: BTreeMap<DimensionName, DimensionSpec>,
    pub codegen: Vec<CodegenTargetConfig>,
}

pub struct CfdPathSet {
    pub entries: Vec<PathBuf>,
}

pub struct DimensionSpec {
    pub variants: Vec<VariantName>,
    pub data: BTreeMap<VariantName, CfdPathSet>,
}

pub struct CodegenTargetConfig {
    pub language: String,
    pub dir: PathBuf,
    pub options: serde_json::Value,
}
```

`ProjectConfig` 使用 `deny_unknown_fields`。遇到 `sources`、`outputs` 或旧 provider 字段时直接返回 `CONFIG-UNKNOWN-FIELD`，不 fallback、不合并、不记录 legacy 字段。配置解析完成后立刻规范化路径并排序，后续 runtime 不再重新解释 YAML。

### 3.3 配置验证规则

1. schema 至少包含一个 `.cft` 文件；schema 目录递归发现 `.cft`，其他文件忽略但不作为数据输入。
2. `data` 和每个维度 variant 至少解析出一个 `.cfd` 文件；空目录、重复逻辑路径和路径穿越是错误。
3. 同一个逻辑 CFD 路径只能出现一次；维度 variant 不能覆盖另一个 variant 的逻辑 identity。
4. `codegen` 的 language id 必须在 `CodegenRegistry` 中存在，输出目录不能与另一个目标重叠。
5. codegen options 由目标 generator 解码，runtime 不理解 C# 或其他语言选项。

## 4. 核心语言和数据结构

### 4.1 语言层

`coflow-language` 对外只暴露稳定的语言值：

```rust
pub struct Span {
    pub start: u32,
    pub end: u32,
}

pub struct SourceText {
    pub logical_path: String,
    pub text: Arc<str>,
}

pub struct CfdDocument {
    pub source: SourceText,
    pub records: Vec<CfdRecordNode>,
    pub diagnostics: Vec<CfdSyntaxDiagnostic>,
}

pub struct CfdRecordNode {
    pub declared_type: TypeName,
    pub key: Option<RecordKey>,
    pub fields: Vec<CfdFieldNode>,
    pub span: Span,
}

pub struct CfdFieldNode {
    pub name: FieldName,
    pub value: CfdValueNode,
    pub span: Span,
}

pub enum CfdValueNode {
    Null(Span),
    Scalar { text: String, span: Span },
    String { value: String, span: Span },
    Reference { key: String, span: Span },
    Array { values: Vec<CfdValueNode>, span: Span },
    Dictionary { entries: Vec<(CfdValueNode, CfdValueNode)>, span: Span },
    Object { declared_type: TypeName, fields: Vec<CfdFieldNode>, span: Span },
    Formatted { segments: Vec<CfdFormatSegment>, span: Span },
}
```

解析器是纯函数，不读文件、不访问 schema、不选择 provider：

```rust
pub fn parse_cfd(source: &SourceText, limits: StructuralLimits) -> CfdDocument;
pub fn compile_schema(modules: &[CftSource], limits: StructuralLimits)
    -> Result<CftSchema, CftDiagnostics>;
```

schema-guided lower 和语义值转换放在 runtime/data-model 边界，不把 `CftSchema` 引入语法 AST。这样 LSP 在没有有效 schema 时仍可完成 CFD 语法解析。

### 4.2 source catalog

runtime 对唯一输入格式定义一个具体 catalog，不再有 provider registry：

```rust
pub struct CfdSourceFile {
    pub id: SourceId,
    pub logical_path: String,
    pub physical_path: PathBuf,
    pub text: Arc<str>,
    pub origin: SourceOrigin,
}

pub struct CfdSourceCatalog {
    pub files: Vec<CfdSourceFile>,
    pub by_path: BTreeMap<String, SourceId>,
}

pub struct CfdOverlay {
    pub logical_path: String,
    pub text: Arc<str>,
}

pub fn discover_cfd_files(config: &CfdPathSet) -> Result<Vec<ResolvedCfdPath>, DiagnosticSet>;
pub fn load_cfd_catalog(
    paths: &[ResolvedCfdPath],
    overlays: &[CfdOverlay],
) -> Result<CfdSourceCatalog, DiagnosticSet>;
```

文件系统读取是 runtime 的唯一 IO；编辑器内存文本通过 `CfdOverlay` 进入同一条路径。不存在 `SourceProvider`、`ResolvedSource`、`ProbeResult`、source options 或动态 provider 选择。

### 4.3 lower 后的数据模型

```rust
pub struct CfdDataModel {
    pub tables: BTreeMap<TypeName, CfdTable>,
    pub records: BTreeMap<RecordCoordinate, CfdRecord>,
    pub references: ReferenceIndex,
    pub origins: SourceIndex,
}

pub struct CfdTable {
    pub type_name: TypeName,
    pub records: Vec<RecordCoordinate>,
    pub by_key: BTreeMap<RecordKey, RecordCoordinate>,
}

pub struct CfdRecord {
    pub coordinate: RecordCoordinate,
    pub declared_type: TypeName,
    pub key: Option<RecordKey>,
    pub value: CfdValue,
    pub origin: RecordOrigin,
}

pub enum CfdValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Enum { type_name: TypeName, variant: String },
    Reference(RecordCoordinate),
    Array(Vec<CfdValue>),
    Dictionary(BTreeMap<CfdKey, CfdValue>),
    Object(BTreeMap<FieldName, CfdValue>),
}
```

`RecordCoordinate { source_id, record_index }` 是唯一记录 identity；不再以表格行号、export 文件名或 provider-specific key 作为 identity。`SourceIndex` 保存 `SourceId -> logical path -> span`，诊断和 editor 视图只能从这里得到位置。

### 4.4 immutable project generation

```rust
pub struct ProjectGeneration {
    pub id: GenerationId,
    pub schema: Arc<CftSchema>,
    pub sources: Arc<CfdSourceCatalog>,
    pub model: Arc<CfdDataModel>,
    pub diagnostics: Arc<[Diagnostic]>,
    pub stats: ProjectExecutionStats,
}

pub struct Runtime {
    project: Project,
    published: Option<Arc<ProjectGeneration>>,
    latest_attempt: Option<Arc<ProjectGeneration>>,
}

impl Runtime {
    pub fn refresh(&mut self, overlays: &[CfdOverlay]) -> Result<RefreshResult, DiagnosticSet>;
    pub fn generation(&self) -> Option<Arc<ProjectGeneration>>;
    pub fn codegen(&self, request: &CodegenRequest) -> Result<CodeArtifactSet, DiagnosticSet>;
}
```

generation 只有在 schema 编译、CFD 全部加载、引用/默认值/维度展开和 check 全部完成后才发布。失败尝试只用于诊断，不替换上一次成功 generation；CLI 和 editor 不得各自复制这套状态机。

## 5. 代码生成合同

### 5.1 通用接口

`coflow-codegen-api` 是唯一保留的扩展 SPI，只处理“schema/model -> source files”：

```rust
pub struct CodegenTarget {
    pub language: String,
    pub output_dir: PathBuf,
    pub options: serde_json::Value,
}

pub struct CodegenInput<'a> {
    pub schema: &'a CftSchema,
    pub model: Option<&'a CfdDataModel>,
    pub sources: &'a [SourceManifestEntry],
    pub target: &'a CodegenTarget,
}

pub struct SourceManifestEntry {
    pub logical_path: String,
    pub origin: SourceOrigin,
}

pub struct CodegenDescriptor {
    pub id: &'static str,
    pub language: &'static str,
    pub runtime_package: &'static str,
    pub runtime_version: &'static str,
    pub needs_model: bool,
}

pub trait CodeGenerator: Send + Sync + Debug {
    fn descriptor(&self) -> &'static CodegenDescriptor;
    fn generate(&self, input: CodegenInput<'_>) -> Result<CodeArtifactSet, CodegenError>;
}

pub struct CodegenRegistry {
    generators: BTreeMap<String, Arc<dyn CodeGenerator>>,
}

pub struct CodeArtifactSet {
    files: Vec<CodeArtifactFile>,
}

pub struct CodeArtifactFile {
    pub relative_path: PathBuf,
    pub contents: String,
}
```

`CodeArtifactSet::new` 在 generator 返回时完成排序、重复文件检查、绝对路径和 `..` 检查。codegen API 不返回 bytes 数据集、不定义导出格式、不接受文件写入句柄。

### 5.2 代码发布

runtime/root 新增小型 `CodeArtifactPublisher`，职责只有：

1. 校验目标目录在 project 根目录内。
2. 将 `CodeArtifactSet` 写入临时 staging 目录。
3. 校验 manifest、文件数量和内容 hash。
4. 原子替换目标语言目录，并保留失败前的旧代码目录。

发布 manifest 只记录生成的源文件和 generator/runtime 版本；不得记录或复制 CFD 内容。删除旧的 `ArtifactSet` 数据导出、active data manifest、data staging 和 exporter publication 代码。

### 5.3 C# generator

`coflow-codegen-csharp` 拆成三个明确层次：

- `lowering`：CFT 类型到 C# 类型/成员的映射。
- `render`：enum、record、database 和 C# CFD binding 模板。
- `binding`：为每个 concrete/abstract type 生成 `ReadCfd`、引用解析、数组/字典/nullable、默认值、继承和 dimension overlay 的代码。

删除 `CsharpLoaderKind`、JSON/MessagePack IR 字段、`render_loader_project`、所有 `type_*_json*`/`type_*_messagepack*` 模板及 reader。生成结果不得出现 `Newtonsoft.Json`、`MessagePack`、`.json` 或 `.msgpack` 字符串。

生成的入口固定为：

```csharp
public static IReadOnlyList<string> SourceFiles { get; }

public static GameConfig Load(
    ICfdSourceProvider provider,
    CfdLoadOptions? options = null);

public static GameConfig Load(
    Func<string, string?> readText,
    CfdLoadOptions? options = null);

public static GameConfig Load(
    IEnumerable<CfdSource> sources,
    CfdLoadOptions? options = null);
```

每个生成类型提供一个内部 binding：

```csharp
internal interface ICfdTypeBinding
{
    string DeclaredType { get; }
    object Read(CfdRecordNode record, CfdLoadContext context);
}
```

生成的 `CfdBindings` 按 declared type 注册 binding，数据库入口只负责收集文档、创建 context、调用 typed binding 和构建索引；不使用反射猜字段，不在运行时解析 schema 文本。

## 6. C# CFD runtime 设计

### 6.1 公共 API

`runtimes/csharp/Coflow.Cfd.Runtime` 是独立、无生成代码依赖的 netstandard2.1/net8.0 库：

```csharp
public readonly record struct CfdSource(string Path, string Text);

public readonly record struct CfdSpan(
    int StartLine, int StartColumn,
    int EndLine, int EndColumn);

public sealed record CfdLoadOptions(
    int MaxDepth = 128,
    int MaxNodes = 1_000_000,
    int MaxRecords = 1_000_000,
    long MaxSourceBytes = 64 * 1024 * 1024);

public interface ICfdSourceProvider
{
    bool TryLoad(string logicalPath, out string? text);
}

public sealed record CfdDocument(
    string Path,
    IReadOnlyList<CfdRecordNode> Records);

public sealed record CfdRecordNode(
    string DeclaredType,
    string? Key,
    IReadOnlyList<CfdFieldNode> Fields,
    CfdSpan Span);

public abstract record CfdValueNode(CfdSpan Span);
public sealed record CfdNullValue(CfdSpan Span) : CfdValueNode(Span);
public sealed record CfdScalarValue(string Text, CfdSpan Span) : CfdValueNode(Span);
public sealed record CfdStringValue(string Value, CfdSpan Span) : CfdValueNode(Span);
public sealed record CfdReferenceValue(string Key, CfdSpan Span) : CfdValueNode(Span);
public sealed record CfdArrayValue(IReadOnlyList<CfdValueNode> Values, CfdSpan Span) : CfdValueNode(Span);
public sealed record CfdDictionaryValue(IReadOnlyList<CfdEntryNode> Entries, CfdSpan Span) : CfdValueNode(Span);
public sealed record CfdObjectValue(string DeclaredType, IReadOnlyList<CfdFieldNode> Fields, CfdSpan Span) : CfdValueNode(Span);

public static class CfdParser
{
    public static CfdDocument Parse(CfdSource source, CfdLoadOptions? options = null);
    public static IReadOnlyList<CfdDocument> ParseAll(
        IEnumerable<CfdSource> sources, CfdLoadOptions? options = null);
}

public sealed class CfdLoadContext
{
    public IReadOnlyList<CfdDocument> Documents { get; }
    public CfdLoadOptions Options { get; }
    public T Resolve<T>(string declaredType, string key);
    internal object Materialize(CfdRecordNode record, ICfdTypeBinding binding);
}

public sealed record CfdDiagnostic(
    string Code, string Message, string Path, CfdSpan Span);

public sealed class CfdLoadException : Exception
{
    public IReadOnlyList<CfdDiagnostic> Diagnostics { get; }
}
```

`CfdParser` 只负责语法树和 UTF-16 span；`CfdLoadContext` 负责文档集合、引用缓存和预算；生成的 binding 负责 schema-specific 类型转换。运行时库不需要安装 Rust、不加载 CFT、不读取 JSON。

### 6.2 运行时语义要求

1. scalar、enum、string、nullable、array、dictionary、inline object 必须有独立转换分支。
2. 数组使用目标元素类型创建 `Array`/`IReadOnlyList<T>`，禁止返回 `object[]` 伪装成功。
3. 引用使用 `(declaredType, key)` identity，并通过 context 缓存解决 forward reference；循环引用返回已创建对象或报告明确的 cycle diagnostic，不允许无限递归。
4. 继承和多态根据 `DeclaredType` dispatch 到具体 binding；抽象类型没有直接构造路径。
5. duplicate record、duplicate field、unknown field、missing required field、invalid enum、reference type mismatch 都产生带 path/span 的 `CfdLoadException`。
6. 默认值、dimension/localization overlay 和格式化字符串由 generator 生成的 binding 执行，runtime 不重复实现一套 CFT 语义。
7. 解析前检查 UTF-8/UTF-16 source size，递归过程中检查 depth/node/record budget；超限错误必须可测试且不能部分发布数据库。
8. `ICfdSourceProvider` 只读取 generator 声明的 `SourceFiles`，不负责枚举目录、不负责猜测扩展名。

### 6.3 C# 测试合同

runtime 测试工程必须覆盖：

- 基础 scalar/enum/string/nullable/array/dictionary/object。
- 多文件加载、source path 诊断和稳定 span。
- forward reference、重复记录、未知字段、错误引用和多态 dispatch。
- 继承、默认值、dimension variant 和 localization overlay。
- depth/node/source-size/record limits。
- `Func<string, string?>`、`ICfdSourceProvider`、`IEnumerable<CfdSource>` 三个入口。
- generated C# smoke test：生成示例项目、编译生成代码、从内存 CFD 文本得到 typed database，再查询一个引用字段。

没有 `dotnet` 的环境只能运行 Rust 生成器测试，并在验证报告中明确标记 C# 编译未执行；不能把 parser 单测结果描述成完整 runtime 验证。

## 7. CLI、editor 和 LSP 改造

### 7.1 CLI

保留 `check`、`build`、`codegen`、`cft`、`lsp` 和项目初始化；删除 `export`、data table create/sync/write、provider/sheet/table/export-format 参数。

- `check`：执行一次完整 runtime refresh，只输出 diagnostics 和统计，不写任何产物。
- `codegen`：要求 refresh 成功，按配置的每个 codegen target 生成并发布源文件。
- `build`：等价于 refresh + 全部 codegen target 的原子发布。
- `cft`：只负责 schema 语言检查和格式化，不读取 Excel/CSV。

命令实现直接调用 `Runtime::refresh`、`Runtime::codegen` 和 `CodeArtifactPublisher`；不再向命令传 `ProviderRegistry`。

### 7.2 editor

Tauri session 只保存 `Project`、`Runtime` 和 generation id。移除 `Arc<ProviderRegistry>`、source type selector、sheet/table writer 和 export preview。保留 CFD 文本编辑、record tree、schema inspector、diagnostic、关系查询和 CFD 写回。

编辑器内存修改统一转为 `CfdOverlay`，通过 runtime rebuild 得到新 generation；不能由 frontend 自己拼装 data model 或自行解析 provider options。

### 7.3 LSP

LSP 复用 `coflow-language` 的 CFT/CFD parser 和 runtime 的 source catalog。删除 provider-specific completion、表格 cell path 和 exporter schema；诊断位置统一来自 `SourceIndex` 和 parser span。

## 8. 文档、示例和技能清理

不迁移旧文档；直接删除失效章节和页面，避免读者看到已删除功能：

- 删除 `website/docs/docs/reference/04-sources/` 全目录。
- 删除 `website/docs/docs/reference/06-export/` 全目录。
- 重写 `05-data-model.md`、`02-project-pipeline.md`、`07-codegen/01-csharp.md`、`08-cli.md`、`10-localization.md`、`12-architecture.md` 和 runtime guide，只描述 CFD-only pipeline。
- README 删除“多数据源统合”、JSON/MessagePack、export 命令和表格写回说明，改为 CFD 文件布局、codegen target 和 C# runtime 快速开始。
- 删除或重写 skills 中关于 Excel/CSV、export、data table writer 的说明；同步脚本不再生成这些 reference。
- 将 `examples/cfd` 作为唯一完整运行时示例；`examples/rpg`、`examples/card_game`、`examples/workflow` 中的非 CFD 输入全部改成 CFD 或删除示例脚本和 xlsx 资源。
- 删除生成的 data output、旧 manifest、JSON/MessagePack fixture 和 exporter 专用测试。

文档检查使用反向搜索：`Excel`、`CSV`、`MessagePack`、`coflow export`、`outputs.data`、`sources:` 等词在公开文档、示例和技能引用中不得留下产品语义（历史变更记录可保留在 release notes，但不能作为使用说明）。

## 9. 实施阶段与提交边界

### 阶段 0：冻结目标合同

建立本计划、更新架构图和测试 fixture；先删除旧配置/命令/API 的目标声明。退出条件：所有 reviewer 按本计划判断删除项，不再有“以后兼容”的待定项。

### 阶段 1：workspace 手术

合并 language crate，删除 provider/exporter/loader/project crate，迁移依赖路径和 public re-export。先让 `cargo check --workspace` 通过；不得在此阶段偷偷保留 dead compatibility modules。

### 阶段 2：配置和 CFD catalog

实现新 `ProjectConfig`、路径规范化、`.cfd` 发现、overlay 和 `CfdSourceCatalog`。删除 `sources`/`outputs` 解析和所有 provider selection。测试 unknown field、重复路径、空目录、路径穿越和确定性排序。

### 阶段 3：runtime generation

把 schema/data/check 组装成不可变 `ProjectGeneration`，统一诊断和 source index。迁移 CLI、editor、LSP 到同一 refresh/codegen API，删除 `Runtime::with_registry`、`check_project(..., registry)` 等过渡入口。

### 阶段 4：codegen API 和 C# generator

冻结 `CodeGenerator`/`CodegenRegistry`/`CodeArtifactSet`；清理 C# 的 JSON/MessagePack IR、模板、reader 和 loader project。新增测试确保输出只有 `.cs`，且含 `SourceFiles` 与三个 CFD load overload。

### 阶段 5：C# runtime 与 generated binding

先完成 parser/AST/diagnostics，再完成 context/cache/limits，最后让 generator 输出每类型 binding。用一个最小 schema + 多文件 CFD fixture 做端到端编译和加载测试；没有 dotnet 时保留明确的未执行状态。

### 阶段 6：发布器、CLI、editor 和 LSP

删除 data artifact lifecycle，保留 code-only `CodeArtifactPublisher`。移除 CLI export/data 子命令、editor table/provider UI 和 LSP provider 分支。验证 editor 进程不被启动、停止或重启。

### 阶段 7：文档和示例

按第 8 节删除/重写页面和 fixture；同步公开网站 reference；全仓库反向搜索旧术语。

### 阶段 8：最终审计

运行 Rust workspace check/test、代码生成 golden tests、artifact path 安全测试、CLI smoke tests、frontend tests 和可用时的 dotnet runtime/generated smoke test。检查依赖图中没有 exporter/provider/table crate、没有 `cfg(any())` 死代码、没有旧配置 fallback。

每个阶段应有独立提交，最终提交前不允许把旧入口标记 deprecated 来逃避删除。

## 10. 验收矩阵

### 功能

- 只有 `.cfd` 能进入 data catalog；Excel/CSV/JSON/MessagePack 路径均被拒绝或不属于配置 schema。
- `check` 不写文件；`codegen`/`build` 只写目标语言源文件和 code manifest。
- C# 生成代码可以只拿 `ICfdSourceProvider`、delegate 或内存 `CfdSource` 加载 CFD，不需要 JSON/MessagePack 文件。
- 增加第二个语言 generator 时只需实现 `CodeGenerator` 和目标语言 binding，不修改 runtime 的 source path 或 data model。

### 架构

- `rg "ProviderRegistry|SourceProvider|DataExporter|LoaderGenerator|outputs\.data|sources:"` 在产品 Rust 代码、配置解析和公开文档中无结果；测试中只能出现删除断言。
- `coflow-api`、`coflow-project`、`coflow-loader-cfd` 和所有旧 loader/exporter crate 不在 workspace。
- runtime 是唯一的文件发现、CFD 读取、generation 发布者；editor/CLI/LSP 不复制 pipeline。
- codegen API 不导出任何数据写出接口；artifact set 不接受绝对路径和目录穿越。

### 正确性与安全

- schema、CFD source、check、codegen 任一阶段失败时，不替换已发布 generation 或代码目录。
- 文件路径、逻辑 source identity、record coordinate 和诊断 span 均可确定重现。
- C# runtime 的预算、循环引用和错误位置测试覆盖所有公开入口。

### 验证命令

普通开发至少执行：

```text
cargo check --workspace
cargo test --workspace
```

代码生成和 C# runtime 改动还必须执行对应的 generator golden tests；存在 .NET SDK 时执行 runtime 与 generated smoke test。不得启动、停止或重启正在运行的 CFD Editor。

## 11. 明确不做的事情

- 不做旧 `sources`/`outputs` 配置的自动转换。
- 不做旧 JSON/MessagePack 产物的读取兼容。
- 不保留 exporter 作为隐藏 feature，不以 `cfg` 或未注册状态保留 provider。
- 不为了“未来可能支持的数据格式”抽象第二套 source/export pipeline；未来只增加目标语言 generator，除非另有独立产品决策。
- 不把 C# runtime 的反射 fallback 作为长期方案；生成 binding 必须承担 schema-specific conversion。

## 12. 文件级执行清单与不变量

### 12.1 workspace 手术的具体顺序

1. 先从根 `Cargo.toml` 和 `Cargo.lock` 移除 `coflow-builtins`、所有 table loader、CSV/Excel loader、exporter core/JSON/MessagePack crate；同一提交删除它们的测试、fixture 和 feature flag。workspace 中不得留下“暂时不注册”的空 crate。
2. 将 `coflow-language` 作为语言层 facade，统一文档和未来扩展的入口；现阶段保留 `coflow-cft`、`coflow-cfd`、`coflow-structure` 作为稳定实现 crate，避免没有收益的目录级搬迁。只有新 facade 覆盖完整 API 后，才删除底层 manifest。
3. 将 `coflow-project` 的 `ProjectConfig`、路径 canonicalization、schema discovery 和 init 合并到 runtime 的 `config/`、`paths/`、`project/` 模块；删除旧 crate 后，CLI/editor/LSP 只从 runtime 取得规范化后的 `Project`。
4. 删除 `coflow-loader-cfd` crate；parser/lower 和 CFD writer 已移动到 `coflow-runtime::cfd_loader`，runtime 入口固定注册一个内部 CFD binding，宿主只能调用 `CfdSourceCatalog`，调用链中不能再出现按格式选择 provider 的逻辑。
5. 删除 `coflow-api` 的 exporter、loader-generation、table operation 和通用 registry；诊断及 CFD 写入合同归入 runtime，`CodeGenerator`/artifact 类型归入 `coflow-codegen-api`。编辑器 wire DTO 只保留 `WriterCapabilities` 的静态 CFD 读写能力。

### 12.2 Rust 核心接口冻结

```rust
pub struct ProjectConfig {
    pub schema: SchemaSpec,
    pub data: CfdPathSet,
    pub dimensions: BTreeMap<DimensionName, DimensionSpec>,
    pub codegen: Vec<CodegenTargetConfig>,
}

pub struct CfdSourceCatalog {
    pub files: Arc<[CfdSourceFile]>,
    pub by_logical_path: BTreeMap<String, SourceId>,
}

pub struct CfdSourceFile {
    pub id: SourceId,
    pub logical_path: String,
    pub absolute_path: PathBuf,
    pub text: Arc<str>,
}

pub struct ProjectGeneration {
    pub id: GenerationId,
    pub schema: Arc<CftSchema>,
    pub catalog: Arc<CfdSourceCatalog>,
    pub model: Arc<CfdDataModel>,
    pub diagnostics: Arc<[Diagnostic]>,
}

pub trait CfdLoader {
    fn load(&self, request: CfdLoadRequest<'_>)
        -> Result<CfdLoadResult, CfdLoadError>;
}

pub trait CodeGenerator: Send + Sync + Debug {
    fn descriptor(&self) -> &'static CodegenDescriptor;
    fn generate(&self, input: CodegenInput<'_>)
        -> Result<CodeArtifactSet, CodegenError>;
}
```

接口不变量：`SourceId` 只由 canonical logical path 派生；`RecordCoordinate` 只由 `(SourceId, record_index)` 构造；`CodeArtifactSet` 创建时排序并拒绝绝对路径、`..`、空路径和重复路径；任何失败的 generation 都不能替换 `published` 指针。

### 12.3 C# runtime 与 generated binding 的边界

```csharp
public interface ICfdSourceProvider
{
    bool TryLoad(string logicalPath, out string? text);
}

public interface ICfdTypeBinding
{
    string DeclaredType { get; }
    object Read(CfdRecordNode record, CfdLoadContext context);
}

public sealed class CfdLoadContext
{
    public IReadOnlyList<CfdDocument> Documents { get; }
    public CfdLoadOptions Options { get; }
    public T Resolve<T>(string declaredType, string key);
}
```

runtime 只实现词法/语法、span、文档集合、引用缓存和预算；generated binding 实现字段名到成员、enum、nullable、数组/字典、默认值、继承/多态和 dimension overlay。runtime 不反射扫描 schema，不从目录猜文件，不实现第二套 CFT 类型系统。

### 12.4 generation 状态机与并发规则

```text
Idle
  -> LoadingSchema -> LoadingCfd -> Checking -> Ready
                                      \-> FailedAttempt
Ready --new revision--> LoadingSchema
FailedAttempt --same revision--> FailedAttempt (只复用诊断)
```

`published` 和 `latest_attempt` 必须是两个独立指针；editor 的 overlay 只影响 candidate，不直接修改 published model。CLI、LSP、editor 所有读操作都带 generation id，旧 generation 的异步结果不能覆盖更新的 revision。代码发布器使用项目根下唯一 staging 目录，写完 manifest/hash 后再进行目录替换。

### 12.5 删除项验收命令

```text
rg "ProviderRegistry|SourceProvider|DataExporter|LoaderGenerator|outputs\.data|sources:" \
  crates src editors website/docs skills examples
cargo tree --workspace | rg "loader-(csv|excel|table)|exporter-(json|messagepack)|coflow-builtins"
```

第一条命令只能在“删除断言”测试中出现匹配；第二条命令必须无输出。公开文档和 skill 快照只描述 CFD、codegen 和 C# runtime，不保留旧命令或迁移章节。
