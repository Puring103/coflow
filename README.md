# Coflow

Coflow 是一个以 CFT schema 和 CFD 文本为唯一数据输入的配置工具。它在构建期编译 schema、加载并校验 `.cfd`，然后生成一个或多个目标语言的类型代码。

## 特性

- CFT 类型、默认值、枚举、引用、多态、维度和 check。
- CFD 文本的结构化记录、内联对象、数组、字典和跨文件引用。
- `check`、`build`、`codegen` 三个构建入口，失败时不替换既有代码目录。
- C# 生成代码直接调用 `Coflow.Cfd.Runtime` 从文件、delegate 或内存文本加载 CFD。
- 代码生成接口支持继续增加其他目标语言；数据格式不再扩展。
- CFT/CFD 的 LSP 和编辑器诊断、补全、跳转与语义高亮。

## 安装

```powershell
cargo install --git https://github.com/Puring103/coflow.git coflow
coflow --help
```

## 快速开始

```powershell
coflow check examples/cfd
coflow codegen examples/cfd
```

最小项目配置如下：

```yaml
schema: schema/
data:
  - data/
codegen:
  - language: csharp
    dir: generated/csharp
    namespace: Example.Config
```

`data` 只能是 CFD 文件或包含 `.cfd` 文件的目录。`codegen` 是唯一产物配置；每个目标包含 `language`、`dir` 和目标语言选项。

## C# runtime

将 `runtimes/csharp/Coflow.Cfd.Runtime` 引入生成代码所在项目，并使用生成的数据库入口：

```csharp
var database = Game.Config.GameConfig.Load(
    new DelegateCfdTextLoader(File.ReadAllText));
```

生成代码声明 `SourceFiles`，runtime 只按这个清单读取 CFD，不扫描目录，也不解析 CFT。

## 开发

```powershell
cargo check --workspace
cargo test --workspace
```

编辑器进程不属于构建验证的一部分；测试和检查均使用无头方式运行。
