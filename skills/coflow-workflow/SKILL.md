---
name: coflow-workflow
description: "Coflow CFD-only 项目工作流：当用户需要规划 coflow.yaml、CFT/CFD 校验、代码生成、C# runtime 接入、CI 或诊断处理时使用。"
---

# Coflow Workflow

使用本 skill 处理外部 Coflow 项目的整体工作流。仓库内部开发流程按项目 `AGENTS.md` 执行。

## 基本流程

1. 定位项目：使用用户给出的 `CONFIG_OR_DIR`，否则从当前目录查找 `coflow.yaml` 或 `coflow.yml`。
2. 读取工程结构：查看 `coflow.yaml`，必要时运行 `coflow schema inspect <project>` 和 `coflow schema files <project>`。
3. 区分任务边界：schema 建模走 `coflow-schema`；CFD 文件由 runtime 统一加载，不存在可选输入格式。
4. 修改后先运行 `coflow check <project>`；需要产物时再运行 `coflow build <project>`。
5. 如果诊断返回非零，按 `file`、`range`、`code` 和消息定位后继续修复，不要跳过检查。

## 常用命令

```powershell
coflow init <project-dir>
coflow schema inspect <project>
coflow schema files <project>
coflow check <project>
coflow build <project>
coflow codegen <project>
```

在 Coflow 源码仓库内试跑未安装版本时，用：

```powershell
cargo run -- <command>
```

## Reference

- 工作流、CI、团队协作和 AI agent 最佳实践：读 `references/best-practices.md`。
- 项目配置字段和路径语义：读 `references/project-config.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/01-project-config>。
- 项目 pipeline、check/build/codegen 阶段：读 `references/project-pipeline.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/02-project-pipeline>。
- CLI 命令行为：读 `references/cli.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/08-cli>。
- 诊断格式和处理方式：读 `references/diagnostics.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/09-diagnostics/01-diagnostics>。
- CFD 数据模型阶段语义：读 `references/data-model.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/05-data-model>。
