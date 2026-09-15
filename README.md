# Coflow

Coflow 是一个以 CFT schema 和 CFD 文本为唯一数据输入的配置工具。它在构建期编译 schema、加载并校验 `.cfd`，然后生成一个或多个目标语言的类型代码。

## 特性

- CFT 记录与 data 类型、默认值、枚举、引用、多态和维度。
- CFD 文本的结构化记录、内联对象、数组、字典和跨文件引用。
- `check`、`build`、`codegen` 三个构建入口，失败时不替换既有代码目录。
- Unity 2022+ / IL2CPP 宿主通过 `Coflow.Runtime` 加载契约与 CFD，构建只读运行时。
- 代码生成接口支持继续增加其他目标语言；数据格式不再扩展。
- CFT/CFD 的 LSP 和编辑器诊断、补全、跳转与语义高亮。

## 安装

```powershell
cargo install --git https://github.com/Puring103/coflow.git coflow
coflow --help
```

## 快速开始

```powershell
coflow check examples/showcase
coflow codegen examples/showcase
```

最小项目配置如下：

```yaml
schema: schema/
data:
  - data/
codegen:
  - language: csharp
    dir: generated/csharp
```

`data` 只能是 CFD 文件或包含 `.cfd` 文件的目录。C# 目标可设置 `namespace`，默认使用 `Coflow.Generated`。
当前版本保留函数、fstring 与 check 源码，暂不提供函数编译、执行、模板求值和 check 执行。

## C# runtime

将 `runtimes/csharp/Coflow.Runtime` 作为 Unity 包引入，安装目标平台原生插件，并使用生成的 `CoflowSchema` 入口：

```csharp
using Coflow.Generated;

using var contract = CoflowSchema.Load();
using var builder = contract.CreateBuilder();
builder.AddSource("items.cfd", itemsText);
using var runtime = builder.Build();
using var item = Item.Wrap(runtime.Record("Item", "sword"));
```

应用向构建器提供所有互相引用的 CFD 文本。构建成功后数据只读；更新数据或 Host 绑定时创建新运行时。
使用结束后释放运行时及其包装。完整用法见 [C# 接入文档](website/docs/docs/reference/07-codegen/01-csharp.md)。
