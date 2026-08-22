# 面向程序员

Coflow 项目由 CFT schema、`.cfd` 数据文件和一个或多个代码生成目标组成。Rust host 使用
`coflow-runtime` 打开只读 session。

## 目标语言生成

目标语言 generator 实现 `coflow_runtime::codegen::CodeGenerator`：

```rust
pub trait CodeGenerator: Send + Sync + std::fmt::Debug {
    fn descriptor(&self) -> &'static CodegenDescriptor;
    fn generate(&self, input: CodegenInput<'_>)
        -> Result<CodeArtifactSet, CodegenError>;
}
```

`CodegenInput` 提供不可变 schema、可选 `CfdDataModel`、CFD source 清单和目标选项；返回的
`CodeArtifactSet` 只能包含安全的项目内相对路径。generator 不读取文件，也不发布目录。

增加语言时新增 generator 和目标语言 runtime，并在应用层注册 descriptor。一个项目可以配置
多个语言目标，所有 generator 都读取同一份 schema、`CfdDataModel` 和 CFD source 清单；Coflow
会在全部目标生成成功后统一发布。

## C# 进程内加载

`coflow-codegen-csharp` 生成类型声明、`ICfdTypeBinding` 和 `Load(ICfdTextLoader)` 入口。
游戏或服务进程引用 `Coflow.Cfd.Runtime` 后提供文件读取函数：

```csharp
var tables = CoflowTables.Load(
    path => File.Exists(path) ? File.ReadAllText(path) : null);
```

生成代码会按照 `SourceFiles` 清单调用 loader，runtime 解析 CFD、建立
`(DeclaredType, Key)` identity cache，并解析跨文件引用。缺少文件、未知字段、非法值、
循环引用和资源限制都通过 `CfdLoadException.Diagnostics` 返回；生成 binding 直接构造
目标类型。

本地化字段生成为 `Localized<T>`。`value.For("zh")` 读取指定 variant，`value.Value` 使用 `Localization.CurrentLanguage`；缺失、`null` 和未知语言自动回退到基础值，不需要注册 localization provider。

## 约束

- 数据文件必须是 `.cfd`；目录发现会忽略其它扩展名。
- 写入只能通过 runtime mutation/write plan，不能绕过 revision 检查直接覆盖源文件。
