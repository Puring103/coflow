# Coflow 文档

Coflow 为游戏配置提供类型定义、数据编辑、业务校验和 C# 接入。程序员用 CFT 定义结构和规则，策划通过 CFD 文本或可视化编辑器维护记录，AI 与自动化工具通过 CLI 查询、修改和验证项目。

一个项目由 `coflow.yaml`、CFT 声明、CFD 数据和生成代码组成。记录可以跨文件引用，维度覆盖用于本地化和平台差异；应用加载 CFD 后，使用生成的 C# 类型读取数据。

## 开始使用

- [安装与快速开始](./guide/install.md)：创建项目并完成第一次构建。
- [Schema 建模](./guide/schema.md)与[编写 CFD](./guide/data-authoring.md)：定义结构、填写记录。
- [编辑器与 LSP](./guide/editor.md)：使用表格、记录、图谱和源码视图。
- [程序员接入](./guide/for-programmers.md)：生成类型并加载配置。
- [使用 AI Agent](./guide/ai-agent.md)：通过 CLI 与 agent 维护项目。

## 检查与交付

`coflow check` 验证声明与数据，`coflow check --rules` 进一步执行业务规则。`coflow build` 在全部检查通过后生成代码；需要单独生成代码时使用 `coflow codegen`。

完整语法见 [CFT](./reference/03-language/01-cft.md)、[CFD](./reference/03-language/02-cfd.md)和 [Check 校验](./reference/03-language/04-check.md)。项目配置、命令与 C# 接口分别见[项目配置参考](./reference/01-project-config.md)、[CLI 参考](./reference/08-cli.md)和 [C# 参考](./reference/07-codegen/01-csharp.md)。
