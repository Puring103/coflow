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

`coflow-codegen-csharp` 生成业务类型、metadata contract 和强类型 `CoflowData` 入口。游戏或
服务进程引用 `Coflow.Cfd.Runtime`，读取 CFD 文本后直接加载：

```csharp
var module = Game.Config.CoflowData.LoadAndCompile(new[] {
    File.ReadAllText("data/items.cfd"),
    File.ReadAllText("data/rules.cfd"),
});

var items = module.Table<Game.Config.Item>();
```

`Load` 构造只读数据 module；`LoadAndCompile` 还会链接并编译 CFD 函数。runtime 解析同次
加载中的全部 CFD、建立 record identity、解析跨文件引用并构造生成类型。未知字段、非法值、
缺失引用、函数签名/函数体错误和资源限制都通过 `CfdLoadException.Diagnostics` 返回。

本地化字段生成为 `Localized<T>`。`value.For("zh")` 读取指定 variant，`value.Value` 使用 `Localization.CurrentLanguage`；缺失、`null` 和未知语言自动回退到基础值，不需要注册 localization provider。

## 约束

- 数据文件必须是 `.cfd`；目录发现会忽略其它扩展名。
- Rust runtime 会把 CFD 函数作为 `CfdFunction` 保留在数据模型中并校验声明签名，不会从
  `data` 中跳过含函数字段的文件。
- 写入只能通过 runtime mutation/write plan，不能绕过 revision 检查直接覆盖源文件。
