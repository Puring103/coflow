# AI Agent Skills

本页介绍如何让 AI agent 维护 Coflow 项目。

## 安装

`coflow` CLI 内置两个 skills，不依赖 Node.js。安装到当前 Coflow 项目：

```powershell
coflow skill install <project>
```

安装到当前用户的通用和已检测 agent 目录：

```powershell
coflow skill install -g
```

使用 `coflow skill status` / `coflow skill status -g` 查看状态，使用对应的
`coflow skill uninstall` 命令卸载。

## Skill 分工

| Skill | 使用场景 |
| --- | --- |
| `coflow-workflow` | 项目流程、`coflow.yaml`、`check/build`、CI、诊断处理和最佳实践 |
| `coflow-schema` | CFT schema、类型/字段/默认值、引用、多态、`check {}`、本地化和数据结构设计 |

## Agent 工作流

给 agent 任务时，尽量提供项目路径和目标，例如“在 `examples/cfd` 中新增一个字段并更新 CFD 记录”。agent 应先读取 schema 和 CFD 文件，再修改：

```powershell
coflow schema inspect <project>
coflow schema files <project>
```

修改 schema 后运行：

```powershell
coflow schema write-file <project> --file schema/main.cft --check
coflow check <project>
```

修改 CFD 后运行 `coflow check <project>`，需要目标语言源文件时运行 `coflow codegen <project>`。

## 文档引用

skills 内置了从公开 reference 文档同步的本地快照，也会在 `SKILL.md` 中标出公开链接。外部引用优先使用网站文档：

- [项目配置](../reference/01-project-config.md)
- [CFT Schema](../reference/03-language/01-cft.md)
- [CFD 文本数据](../reference/03-language/02-cfd.md)
- [数据模型](../reference/05-data-model.md)
- [CLI 命令](../reference/08-cli.md)
