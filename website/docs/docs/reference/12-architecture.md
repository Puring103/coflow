# 项目架构

## 固定边界

Coflow 的项目输入是 CFT schema 加 CFD 文本文件，项目输出是一个或多个目标语言的代码
目录。

```text
schema/*.cft + data/**/*.cfd
        -> coflow-runtime::CfdDataModel
        -> coflow-runtime::codegen::CodeArtifactSet
        -> generated/<language>/
```

## package 边界

| package | 职责 |
| --- | --- |
| `coflow-language` | CFT schema、CFD 值语法、AST、span、结构限制 |
| `coflow-runtime` | 项目配置、固定 CFD resolve/load/write、数据模型、查询、检查、诊断和 codegen SPI |
| `coflow-codegen-csharp` | C# 声明、显式 CFD readers、数据库入口 |
| `coflow` | CLI、LSP、生成事务和 staging 发布 |
| `cfd-editor-core` | 宿主无关的编辑器 session、wire DTO、读写操作、文件监听和编辑器事件 |
| `cfd-editor` | Tauri 命令/事件适配以及窗口、对话框、更新器等原生宿主集成 |
| `Coflow.Cfd.Runtime` | C# 进程内 CFD parser、source loader、binding context、值转换 |

LSP 已并入根 `coflow`，extension manifest 已并入 editor backend。

## Rust 核心接口

```rust
pub struct Runtime;

impl Runtime {
    pub fn new() -> Self;
    pub fn open_read_only_session(
        &self, project: Project,
    ) -> Result<ReadOnlyProjectSession, DiagnosticSet>;
}

pub struct CfdDataModel {
    pub sources: Arc<[CfdSourceInfo]>,
    pub records: Arc<[CfdRecord]>,
    pub values: Arc<[CfdValue]>,
    pub source_index: SourceIndex,
    pub record_index: RecordIndex,
}

pub trait CodeGenerator: Send + Sync + std::fmt::Debug {
    fn descriptor(&self) -> &'static CodegenDescriptor;
    fn generate(&self, input: CodegenInput<'_>)
        -> Result<CodeArtifactSet, CodegenError>;
}
```

`CodeArtifactSet` 只接受相对路径，拒绝绝对路径、`..` 和重复文件。generator 不访问文件系统；
根应用先收集所有目标的 artifacts，再统一 staging、备份和原子发布。发布前会规范化现有祖先
和符号链接，拒绝覆盖项目根、配置、schema、data、维度目录或相互重叠的输出。`@idAsEnum`
的稳定编号保存在项目根的 `coflow.enum.lock.json`。

## C# direct-load 接口

生成的 `CoflowData` 接受一个或多个 CFD 文本，并把生成 contract 交给固定 runtime：

```csharp
public static CoflowModule Load(string cfd);
public static CoflowModule Load(string[] cfdSources);
public static CoflowModule LoadAndCompile(string cfd);
public static CoflowModule LoadAndCompile(string[] cfdSources);
```

`Load` 解析普通数据，`LoadAndCompile` 进一步链接函数引用、检查函数体并生成 VM bytecode。
`CoflowModule.Table<T>()` 和 `Singleton<T>()` 提供强类型读取；多个独立 module 通过不可变的
`CoflowModuleSet` 组合。parser 保留源码 span，加载、链接和函数编译错误统一返回稳定诊断。

codegen source manifest 为每个逻辑 CFD 路径标记 `Project` 或结构化的 `Dimension { dimension, source_type, field }` origin。一个 singleton 维度文件可对应多个字段 origin，但生成 loader 只读取一次物理路径，并按记录 key 分派到内部 variant binding。该规范化层只服务 direct loading，不改变公开 schema，也不生成可查询的 dimension table。

## 原子性

代码生成和编辑器写入都遵循“候选构建、验证、最后一次发布”。多目标生成中任何一个
目标失败时，之前的目标也不能出现在输出目录；发布阶段失败时按逆序恢复旧目录。
Unity `.meta` 文件随输出替换保留。
