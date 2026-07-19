# 项目配置

一个 Coflow 项目由 `coflow.yaml` 定义。所有相对路径都相对于该文件所在目录解析。

```yaml
schema: schema/

sources:
  - path: data

outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
```

`schema` 可指向单个 `.cft` 文件或目录。`sources` 使用 `path` 指向本地 Excel、CSV、CFD 文件或包含这些文件的目录。省略 `type` 时会根据文件后缀选择 Provider。

Excel source 可以用 `sheets` 显式声明 worksheet、record key 和列名映射。`outputs.data.type` 支持 `json` 和 `messagepack`；`outputs.code.type` 目前支持 `csharp`。

使用 `coflow init <dir>` 创建最小项目骨架。完整字段、校验和 sheet 配置见 [`coflow.yaml` 参考](../reference/01-project-config.md)。
