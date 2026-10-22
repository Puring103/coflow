---
name: coflow-workflow
description: "维护使用 Coflow 的配置项目：查询和修改 CFD、管理 coflow.yaml、运行业务校验、处理诊断、生成代码或接入 C#。不用于开发 Coflow 本身。"
---

# Coflow Workflow

在用户指定的项目与修改范围内工作，沿用已有授权。业务取值或范围不明确时询问用户。

## 读取与修改

- 用 `--project <项目>` 定位。未知命令先用 `coflow describe --json` 查目录，再用 `describe <命令> --json` 查参数；请求结构按需加 `--schema input`。
- 优先 `context --type <类型> --json` 和 `data get/list --fields ... --json`，按字段、分页和预算取所需内容，保存 `revision`。
- 批量修改用 `coflow apply --input changes.json --rules --if-revision <revision> --json`。需审阅补丁时加 `--dry-run`；成功后无需重复同范围检查。CFT 与 CFD 联合变更用 `kind: files`，记录、集合和维度修改用 `kind: operations`。
- 单字段用 `data set`，`--expected-cfd` 按类型与默认值比较当前基础有效值，模板比较 `f` 源表达式。项目写入与候选预览均支持 `--if-revision`。
- 直接编辑源码后用 `coflow check --rules --json`。`check` 默认只检查结构与数据；需要生成代码时用 `build` 完成全部校验与生成。

## 结果处理

使用单行 `--json`，人工查看用 `--pretty`。默认返回计数与失败信息、最多 50 条诊断；用 `diagnostics_summary` 判断截断，`--details` 展开，显式 `--limit` 仍生效。

按 `code`、`help`、`record`、`field_path` 修复。请求错误看 `result.request_path`、`operation_index`、`target`；规则解释按需用 `check --rule Type::Name --explain`。冲突时重读目标再决定修改，旧游标需去掉 `--cursor` 重查。事务失败检查 `written`、实际写入与恢复文件。以 `status` 和 `complete` 判断结果；诊断截断只表示输出省略。

## 按需参考

- [CLI](references/cli.md)：命令、变更集与类型化值；[公开文档](https://puring103.github.io/coflow/docs/reference/08-cli)。
- [项目配置](references/project-config.md)：输入与生成目录；[公开文档](https://puring103.github.io/coflow/docs/reference/01-project-config)。
- [CFD](references/cfd.md)：源码表达式；[公开文档](https://puring103.github.io/coflow/docs/reference/03-language/02-cfd)。
- [诊断](references/diagnostics.md)：错误定位与解释；[公开文档](https://puring103.github.io/coflow/docs/reference/09-diagnostics/01-diagnostics)。
- [C#](references/csharp.md)：生成类型与加载；[公开文档](https://puring103.github.io/coflow/docs/reference/07-codegen/01-csharp)。
- [Pipeline](references/project-pipeline.md)、[数据模型](references/data-model.md)：校验与加载语义；[Pipeline 公开文档](https://puring103.github.io/coflow/docs/reference/02-project-pipeline)、[数据模型公开文档](https://puring103.github.io/coflow/docs/reference/05-data-model)。
- [协作与 CI](references/best-practices.md)：检查范围与并发修改。
