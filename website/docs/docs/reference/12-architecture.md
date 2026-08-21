# 项目架构

## crate 边界

| crate | 职责 |
| --- | --- |
| `coflow-language` | CFT/CFD parser、AST、span 和结构限制 |
| `coflow-runtime` | 配置、CFD 文档/数据模型、checker、generation、CLI/editor/LSP 共享会话 |
| `coflow-codegen-api` | `CodeGenerator`、`CodegenInput`、`CodeArtifactSet` |
| `coflow-codegen-csharp` | C# declarations、typed binding 和 runtime 入口 |

输入格式固定为 CFD；架构扩展点只有目标语言 generator。

## 核心接口

```rust
pub trait CodeGenerator {
    fn descriptor(&self) -> &'static CodegenDescriptor;
    fn generate(&self, input: CodegenInput<'_>) -> Result<CodeArtifactSet, CodegenError>;
}

impl Runtime {
    pub fn new() -> Self;
    pub fn open_read_only(&self, project: Project) -> Result<ReadOnlyProjectSession, DiagnosticSet>;
    pub fn open_write(&self, project: Project) -> Result<WriteProjectSession, DiagnosticSet>;
}
```

`CodeArtifactSet` 只接受项目内相对路径，创建时拒绝绝对路径、`..` 和重复文件。文件系统发布在应用层 staging 后原子替换；runtime 和 generator 不直接写目录。
