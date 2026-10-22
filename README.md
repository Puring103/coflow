# Coflow

Coflow 是一个以 CFT schema 和 CFD 文本为唯一数据输入的配置工具。它在构建期编译 schema、加载并校验 `.cfd`，然后生成一个或多个目标语言的类型代码。

## 特性

- CFT table、data、singleton 声明、默认值、枚举、引用、多态和维度。
- CFD 文本的结构化记录、内联对象、数组、字典和跨文件引用。
- 命名 `check` 支持内联 data 自检、集合与关联检查、维度遍历和可定位诊断。
- 字符串支持显式 `f"..."` 插值，字段省略由声明默认值控制。
- `check`、`build`、`codegen` 三个构建入口，失败时不替换既有代码目录。
- C# 通过 `Coflow.Loader` 将调用方提供的 CFD 直接加载为生成类型。
- CFT/CFD 的 LSP 和编辑器诊断、补全、跳转与语义高亮。

## 安装

```powershell
cargo install --git https://github.com/Puring103/coflow.git coflow
coflow --help
```

## 快速开始

```powershell
coflow check --rules --project examples/showcase
coflow codegen --project examples/showcase
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

`data` 只能是 CFD 文件或包含 `.cfd` 文件的目录。C# 目标可设置 `namespace`。

## C# 数据加载

引用 `runtimes/csharp/Coflow.Loader` 和生成的 C# 文件。加载器支持 netstandard2.1 与 net8.0。
应用自行选择 CFD 文件并提供内容：

```csharp
using Coflow.Loader;

var result = CoflowData.Load(new[] {
    new CfdSource("items.cfd", itemsText),
    new CfdSource("ja.cfd", japaneseText),
});
if (result.Success) {
    var sword = result.Data.Table<Item>()["sword"];
    var japaneseName = sword.For("language", "ja").name;
}
```

加载结果包含成功状态和诊断。`int`、`float` 对应 C# `long`、`double`，可选值使用可嵌套的 `Option<T>`。
table 和 singleton 生成 partial class；data 默认生成 partial class，`@struct sealed data` 生成 partial struct。
通过 `Table<T>()`、`Singleton<T>()` 获取业务数据；枚举主键提供对应表的 `Get(enum)`、`TryGet(enum, out record)` 扩展。记录引用直接连接生成对象。
直接读取字段得到基础值；`record.For("language", "ja")` 返回独立的变体视图。缺少覆盖时，基础表达式在所选维度上下文中求值；`record.HasVariant("name", "ja")` 可以判断字段是否显式填写。
C# 加载只使用 CFD 和生成代码，CFT 用于声明检查和代码生成，check 规则由编辑器和 CLI 的 `check --rules`、`build` 执行。
详见 [C# 接入文档](docs/integration/csharp-unity.md)。
