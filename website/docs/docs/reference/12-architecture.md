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
| `cfd-editor` | Tauri backend 和编辑器 wire DTO |
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
    object Read(CfdRecordNode record, CfdLoadContext context);
}

public sealed class CfdLoadContext
{
    public T Resolve<T>(string declaredType, string key);
}
```

context 以 `(declaredType, key)` 缓存对象并检测循环引用；parser 保留 `CfdSpan`，值读取器
将未知字段、缺失字段、非法 enum、数值溢出、未知 concrete type、缺少引用和资源限制转成
稳定的 `CfdDiagnostic`。生成代码使用显式构造函数和 reader lambda，不使用反射或动态构造。

## 原子性

代码生成和编辑器写入都遵循“先验证、后 staging、最后一次发布”。多目标生成中任何一个
目标失败时，之前的目标也不能出现在输出目录；发布阶段失败时按逆序恢复旧目录。
