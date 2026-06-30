# 给程序的接入路径

本页面向客户端、服务器、工具链和技术美术方向的开发者，说明 Coflow 如何进入现有项目工程链路。

## 需要回答的问题

- 如何安装和运行 `coflow` CLI。
- 如何组织 `coflow.yaml`、schema、数据源和输出目录。
- 如何把 `check`、`build`、`export` 和 `codegen` 接入本地流程与 CI。
- JSON、MessagePack 和 C# 运行时代码分别如何接入。
- 生成目录由 Coflow 接管时有哪些安全边界。
- 如果要扩展数据源、导出或代码生成，provider 边界在哪里。

## 推荐阅读顺序

1. [安装](/docs/guide/install)
2. [示例](/docs/guide/examples)
3. [最佳工作流](/docs/guide/best-workflow)
4. [CLI 命令参考](/docs/reference/08-cli)
5. [项目架构](/docs/reference/12-architecture)
