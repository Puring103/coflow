---
name: coflow-workflow
description: "使用 Coflow CLI 查询、维护和批量修改 CFD 配置，处理本地化、校验与错误恢复，生成 C# 代码、接入应用或编写编辑器插件。适用于已有配置结构下的操作；类型设计与 CFT 结构变更使用 coflow-schema。不用于开发 Coflow 本身。"
---

# Coflow Workflow

在用户指定的项目与业务范围内工作，沿用已有授权。业务取值、批量范围或规则调整目标不明确时询问用户。

## 执行主线

1. 用 `--project <目录或 coflow.yaml>` 固定项目。未知类型先 `project inspect`、`project files`、`schema symbols --kind type`，已有类型再 `context --type <类型>`。首次使用读[项目发现](references/discovery.md)和[核心概念](references/concepts.md)。
2. 使用 `--json`，按字段、分页和预算读取任务所需信息，保存响应顶层的 `revision`。查询结果有数据时仍检查 `data_scope.complete`。
3. 单字段用 `data set/unset`；多项关联修改用 `apply`。写入携带读取修订 `--if-revision`，业务修改加 `--rules`。需要审阅时使用 `--dry-run`。
4. 以退出码、`status`、适用的 `complete` 和实际写入信息判断结果。成功写入已包含所选校验；直接编辑源码后执行 `check --rules`。发生冲突时重读目标和范围，重新形成修改。

命令不明确时用 `coflow describe --json` 找入口，再用 `coflow describe data.set --json` 这样的点分名称查参数；构造请求前按需加 `--schema input`。以安装版本返回的协议为准，能力不匹配时先核对 `coflow --version`。

## 按任务读取

| 任务 | 资料 |
| --- | --- |
| 定位项目、发现类型、处理 CLI 缺失 | [项目发现](references/discovery.md) |
| 调整输入、维度目录或生成目标 | [项目配置](references/project-config.md) |
| 理解记录、字段值、默认值和模板 | [核心概念](references/concepts.md) |
| 查询、筛选、分页、查引用或统计 | [读取数据](references/reading.md) |
| 新增、修改、改名、迁移或删除记录 | [记录维护](references/record-editing.md) |
| 按条件批量赋值、修改数组与字典 | [批量修改](references/batch-editing.md) |
| 补翻译、维护平台或地区变体 | [维度维护](references/dimensions.md) |
| 解释规则失败、修复坏文件、处理写入失败 | [校验与恢复](references/validation-and-recovery.md) |
| 生成代码、检查产物或接入 CI | [代码生成](references/generation.md) |
| 获取 Loader、接入 C# / Unity、重新加载 | [C# 接入](references/csharp-integration.md) |
| 创建或安装编辑器专用视图与工具 | [插件编写](references/plugin-authoring.md) |
| 构造 JSON 值、解析响应、修订与路径 | [CLI 协议](references/protocol.md) |

仅读取当前任务需要的资料。类型设计、默认值策略和 CFT 结构变更由 `coflow-schema` 承担；普通数据维护可仅使用本 skill 完成。参考资料和示例随本 skill 提供，常规操作无需联网查网站。
