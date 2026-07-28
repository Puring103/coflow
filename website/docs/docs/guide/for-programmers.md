# 给程序的接入路径

## 项目边界

`coflow.yaml` 声明 schema、本地 source 和输出。CFT 是类型与业务规则的权威定义，Excel/CSV/CFD 是可编辑数据，JSON/MessagePack 和 C# 是构建产物。

## 本地和 CI

```powershell
coflow check path/to/project
coflow build path/to/project
```

Pull request CI 应使用 `check`，只校验而不写产物。需要发布全部运行时产物的交付阶段使用
`build`；只需要数据或代码时，分别使用 `export` 或 `codegen`。这些命令处理 YAML 中配置的
全部对应 target。

## 运行时接入

- JSON 适合调试、工具链和需要可读性的环境。
- MessagePack 适合更紧凑的发布数据。
- C# codegen 生成与 schema 同步的只读类型和 loader，可读取 JSON 或 MessagePack。

`outputs.data.dir` 和 `outputs.code.dir` 由 Coflow 整体管理。只有完整构建成功后才会替换这些目录；不要在其中放入手写文件。

## 扩展 Coflow

需要扩展数据源、写入格式、导出格式或代码生成时，实现 `coflow-api` 提供的对应 Provider 接口，并在宿主应用中注册。

详细合同见 [项目配置](../reference/01-project-config.md)、[CLI 命令](../reference/08-cli.md) 和 [项目架构](../reference/12-architecture.md)。
