# Coflow

[English](README.md) | 简体中文

Coflow 将数据定义、编辑、校验和应用接入连接起来：用 CFT 定义类型与业务规则，用可版本管理的 CFD 文本保存配置，为应用生成类型安全的代码。

**AI 原生的数据配置工作流。人与 AI 共同配置。**

[网站](https://puring103.github.io/coflow/) · [文档](https://puring103.github.io/coflow/docs/) · [下载](https://github.com/Puring103/coflow/releases/latest)

## 特性

- **AI 参与高速迭代。** 针对 AI 设计的命令行工具，配合专属 skills，提供上下文查询、修改预览、带修订条件的批量修改与可操作的诊断。
- **始终保持人类参与。** 在关系图、表格、记录和源码视图间切换，编辑同一份数据，审查差异并处理诊断。
- **建立统一的数据模型。** 用 CFT 描述类型、字段、引用、继承、枚举和默认值，让策划、AI 和程序遵循共同的数据语言。
- **为数据定义规则。** 声明类型内规则和跨记录检查，校验内联对象与维度变体，将错误精准定位到记录和字段。
- **可版本管理的配置。** CFD 支持嵌套对象、数组、字典、跨文件引用与显式 `f"..."` 字符串插值。
- **本地化与维度。** 为字段提供语言、平台等变体，检查覆盖完整性，并按使用场景读取配置。
- **类型安全的 C# 接入。** 生成业务类型，通过 `Coflow.Loader` 加载应用提供的 CFD，支持对象引用与独立的维度视图。
- **灵活可扩展的编辑器。** 通过插件添加专用视图与字段呈现；CFT/CFD 语言服务提供诊断、补全、跳转和语义高亮。
- **交付前完成校验。** 使用 `check`、`build` 和 `codegen`，校验失败时保留已有生成代码。

## 安装

从 [GitHub Releases](https://github.com/Puring103/coflow/releases/latest) 下载适合系统的 CLI，将其所在目录加入 `PATH`；也可以安装 Rust 后从源码安装：

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

`data` 只能是 CFD 文件或包含 `.cfd` 文件的目录。C# 目标可设置 `namespace`。从零创建项目可参考[快速开始](https://puring103.github.io/coflow/docs/guide/install)。

## AI 与自动化

```powershell
coflow describe --json
coflow context --type Item --project <project> --json
coflow apply --input changes.json --project <project> --rules --if-revision <revision> --json
```

读取结果的 `revision` 可用于保护后续修改。`apply --rules` 在发布前完成结构、数据与业务校验；需要审阅时加 `--dry-run`。默认 JSON 保留计数与失败信息，完整请求结构用 `describe apply --schema input` 查询，执行明细按需加 `--details`。用 `coflow skill install -g` 安装配套 skills，详见 [CLI 命令](https://puring103.github.io/coflow/docs/reference/08-cli)。

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

加载结果包含成功状态和诊断。`int`、`long`、`float`、`double` 对应同名 C# 类型，可选值使用可嵌套的 `Option<T>`。
table 和 singleton 生成 partial class；data 默认生成 partial class，`@struct sealed data` 生成 partial struct。
通过 `Table<T>()`、`Singleton<T>()` 获取业务数据；枚举主键提供对应表的 `Get(enum)`、`TryGet(enum, out record)` 扩展。记录引用直接连接生成对象。
直接读取字段得到基础值；`record.For("language", "ja")` 返回独立的变体视图。缺少覆盖时，基础表达式在所选维度上下文中求值；`record.HasVariant("name", "ja")` 可以判断字段是否显式填写。
C# 加载只使用 CFD 和生成代码，CFT 用于声明检查和代码生成，check 规则由编辑器和 CLI 的 `check --rules`、`build` 执行。
详见 [C# 数据访问](https://puring103.github.io/coflow/docs/reference/07-codegen/01-csharp)。
