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

`CodeArtifactSet` 只接受项目内相对路径，拒绝绝对路径、`..` 和重复文件。generator 不访问
文件系统；根应用先收集所有目标的 artifacts，再统一 staging、备份和原子发布。

## C# direct-load 接口

生成的数据库类暴露 `SourceFiles` 和 `Load(ICfdTextLoader)`。它把文件清单交给：

```csharp
public interface ICfdTextLoader
{
    bool TryLoad(string logicalPath, out string? text);
}

public interface ICfdTypeBinding
{
    string DeclaredType { get; }
    IReadOnlyList<string> AssignableTypes { get; }
    string? ObjectFieldType(string fieldName);
    string? ReferenceFieldType(string fieldName);
    object Read(CfdRecordNode record, CfdLoadContext context);
}

public sealed class CfdLoadContext
{
    public T Resolve<T>(string declaredType, string key);
}
```

`AssignableTypes` 包含 concrete type 自身及其全部 CFT ancestor；两个 field-type 查询用于让
schema-free parser 沿内联 object 和 record ref 正确求值格式化字符串。context 用这些信息建立继承域内的
record key identity，以 `(declaredType, key)` 缓存对象并检测循环引用；parser 保留 `CfdSpan`，值读取器
将未知字段、缺失字段、非法 enum、数值溢出、未知 concrete type、缺少引用和资源限制转成
稳定的 `CfdDiagnostic`。生成 reader 会在字段缺失时应用 CFT 默认值；无默认值的字段仍是必填。
生成代码使用显式构造函数和 reader lambda，不使用反射或动态构造。

codegen source manifest 为每个逻辑 CFD 路径标记 `Project` 或结构化的 `Dimension { dimension, source_type, field }` origin。一个 singleton 维度文件可对应多个字段 origin，但生成 loader 只读取一次物理路径，并按记录 key 分派到内部 variant binding。该规范化层只服务 direct loading，不改变公开 schema，也不生成可查询的 dimension table。

## 原子性

代码生成和编辑器写入都遵循“先验证、后 staging、最后一次发布”。多目标生成中任何一个
目标失败时，之前的目标也不能出现在输出目录；发布阶段失败时按逆序恢复旧目录。
