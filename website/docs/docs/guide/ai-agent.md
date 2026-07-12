# AI Agent Skills

本页介绍如何让 AI agent 维护 Coflow 项目。

## 安装

外部 Coflow 项目通常安装三个 skills：

```powershell
npx skills add Puring103/coflow -g --skill coflow-workflow --copy -y
npx skills add Puring103/coflow -g --skill coflow-schema --copy -y
npx skills add Puring103/coflow -g --skill coflow-data --copy -y
```

## Skill 分工

| Skill | 使用场景 |
| --- | --- |
| `coflow-workflow` | 项目流程、`coflow.yaml`、`check/build`、CI、诊断处理和最佳实践 |
| `coflow-schema` | CFT schema、类型/字段/默认值、引用、多态、`check {}`、本地化和数据结构设计 |
| `coflow-data` | CFD、Excel、CSV、飞书/Lark 数据源、创建文件、同步表头和 `data patch` |

## Agent 工作流

给 agent 任务时，尽量提供项目路径和目标，例如“在 `examples/rpg` 中新增一个稀有度字段并同步数据”。agent 应先读取 schema 和数据源，再修改：

```powershell
coflow schema inspect <project>
coflow schema files <project>
coflow data sources <project>
```

修改 schema 后运行：

```powershell
coflow schema write-file <project> --file schema/main.cft --stdin --check
coflow check <project>
```

修改数据时优先使用 writer 命令：

```powershell
coflow data patch <project> --patch '<json>'
coflow data sync-header <project> --file data/items.csv --type Item
coflow data write-file <project> --file data/items.cfd --stdin --check
coflow check <project>
```

`data patch` 会先规划和预检整批操作，再在一个 mutation transaction 中写入。任一
writer、重建或提交步骤失败都会补偿已写来源，`applied` 为空且旧 generation 保持可用。
处理结果时应查看 `write_ok`、`check_ok`、`applied`、`failed`、`affected_files`
和 `diagnostics`，再决定是否继续修复。

## 文档引用

skills 内置了从公开 reference 文档同步的本地快照，也会在 `SKILL.md` 中标出公开链接。外部引用优先使用网站文档：

- [项目配置](../reference/01-project-config.md)
- [CFT Schema](../reference/03-language/01-cft.md)
- [CFD 文本数据](../reference/03-language/02-cfd.md)
- [表格单元格值](../reference/03-language/03-cell-value.md)
- [CLI 命令](../reference/08-cli.md)
