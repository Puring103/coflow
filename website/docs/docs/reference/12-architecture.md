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
| `Coflow.Runtime` | C# 进程内 Module 管理、CFD 加载、编译、查询和函数执行 |

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

## C# Runtime 接口

生成的 `Schema` 创建一个运行时实例。应用按 Module 提交带逻辑路径的 CFD source，绑定 Host，并显式编译：

```csharp
var coflow = Schema.Create();
var module = coflow.LoadModule(new CoflowSource("items.cfd", itemsCfd));
coflow.Bind(new HostServices(environment, log));
var result = coflow.Compile();
```

一次编译统一解析全部 Module、解析跨 Module 引用、链接并编译函数。`Coflow.Table` 和
`Coflow.Singleton` 提供强类型读取。Module 替换或移除后再次编译；失败的编译不替换上一次成功状态。
parser 保留源码 span，加载、链接和函数编译错误统一返回稳定诊断。

codegen source manifest 为每个逻辑 CFD 路径标记 `Project` 或结构化的 `Dimension { dimension, source_type, field }` origin。一个 singleton 维度文件可对应多个字段 origin，但生成 loader 只读取一次物理路径，并按记录 key 分派到内部 variant binding。该规范化层只服务 direct loading，不改变公开 schema，也不生成可查询的 dimension table。

## 原子性

代码生成和编辑器写入都遵循“候选构建、验证、最后一次发布”。多目标生成中任何一个
目标失败时，之前的目标也不能出现在输出目录；发布阶段失败时按逆序恢复旧目录。
Unity `.meta` 文件随输出替换保留。
