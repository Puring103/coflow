# 项目架构

## crate 边界

| crate | 职责 |
| --- | --- |
| `coflow-language` | CFT/CFD parser、AST、span 和结构限制 |
| `coflow-data-model` | source-neutral 值、记录 identity、引用和来源索引 |
| `coflow-checker` | schema-guided check 计划与诊断 |
| `coflow-runtime` | 配置、CFD catalog、generation、CLI/editor/LSP 共享会话 |
| `coflow-codegen-api` | `CodeGenerator`、`CodegenInput`、`CodeArtifactSet` |
| `coflow-codegen-csharp` | C# declarations、typed binding 和 runtime 入口 |

旧表格 loader、exporter、通用 source provider 和数据 artifact crate 不属于最终 workspace。未来扩展点只有目标语言 generator。

## 核心接口

```rust
pub trait CodeGenerator {
    fn descriptor(&self) -> &'static CodegenDescriptor;
    fn generate(&self, input: CodegenInput<'_>) -> Result<CodeArtifactSet, CodegenError>;
}

pub struct Runtime {
    project: Project,
    published: Option<Arc<ProjectGeneration>>,
}

impl Runtime {
    pub fn refresh(&mut self, overlays: &[CfdOverlay]) -> Result<RefreshResult, DiagnosticSet>;
    pub fn codegen(&self, request: &CodegenRequest) -> Result<CodeArtifactSet, DiagnosticSet>;
}
```

`CodeArtifactSet` 只接受项目内相对路径，创建时拒绝绝对路径、`..` 和重复文件。文件系统发布在应用层 staging 后原子替换；runtime 和 generator 不直接写目录。
