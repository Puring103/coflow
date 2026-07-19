# 给程序的接入路径

## 项目边界

`coflow.yaml` 声明 schema、本地 source 和输出。CFT 是类型与业务规则的权威定义，Excel/CSV/CFD 是可编辑数据，JSON/MessagePack 和 C# 是构建产物。

## 本地和 CI

```powershell
coflow check path/to/project
coflow build path/to/project
```

Pull request CI 应使用 `check`，只校验而不写产物。需要发布运行时数据的交付阶段使用 `build`。如果只需要单个产物，使用 `export json`、`export messagepack` 或 `codegen csharp`。

## 运行时接入

- JSON 适合调试、工具链和需要可读性的环境。
- MessagePack 适合更紧凑的发布数据。
- C# codegen 生成与 schema 同步的只读类型和 loader，可读取 JSON 或 MessagePack。

`outputs.data.dir` 和 `outputs.code.dir` 由 Coflow 整体接管。构建会先在 staging 中生成并验证完整 snapshot，然后才替换稳定目录；不要在这些目录放入手写文件。

## 扩展 Coflow

数据源、writer、exporter 和 codegen 通过 `coflow-api` 中的 provider traits 与宿主解耦。CLI、编辑器和 LSP 使用 `coflow-builtins` 构建默认 registry，不应在 runtime 中特判具体 Provider。

详细合同见 [项目配置](../reference/01-project-config.md)、[CLI 命令](../reference/08-cli.md) 和 [项目架构](../reference/12-architecture.md)。
