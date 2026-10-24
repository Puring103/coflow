---
name: coflow-workflow
description: "维护使用 Coflow 的配置项目：查询与修改 CFD、项目配置、业务校验、诊断、代码生成、C# 接入，以及编辑器插件使用与编写。不用于开发 Coflow 本身。"
---

# Coflow Workflow

在用户指定的项目与修改范围内工作，沿用已有授权。业务取值或范围不明确时询问用户。

## 发现与读取

用 `--project <目录或 coflow.yaml>` 定位。没有项目时用 `init`；未知类型先查 `project inspect`、`project files` 和 `schema symbols`，已知类型再用 `context --type <类型>`。模型损坏时用 `source read` 定位和修复，见 [CLI 恢复流程](references/cli.md#项目发现与错误恢复)。

优先 `context` 和 `data get/list --fields ... --json`，按字段、分页和预算读取，保存 revision。`data_scope.complete` 表示加载完整性，`truncated` 表示输出分页或省略，分别判断。未知命令先用 `describe --json` 查目录，再用 `describe <命令> --json` 查参数；请求结构按需加 `--schema input`。能力缺失时核对 CLI 版本，不猜测参数。

## 选择修改方式

- 单字段：`data set` / `unset`。`--expected-cfd` 按类型与默认值比较当前基础有效值，模板比较 f 源表达式。
- 多记录、集合及维度：`apply` 的 `kind: operations`。已知目标显式列出记录；按条件批量赋值用 `set_where`，先核对匹配范围并填写 expected_count。数组、字典和维度操作见 [CLI 请求结构](references/cli.md#批量修改与请求结构)。
- CFT 与 CFD 联合变更、损坏源码修复：`kind: files`，一次提交相关源码与输入调整。类型、默认值和规则语义见 [CFT](references/cft.md)，写值见 [CFD](references/cfd.md)。
- 本地化：`dimension get/missing` 区分已填写和回退；用 set/clear 修改覆盖，字段属于哪个维度以 CFT 为准，见[维度](references/localization.md)。
- 交付：`check --rules` 完整校验，`build` 校验并生成，`codegen --check` 核对生成产物；C# 的加载与重载见 [C#](references/csharp.md)。
- 插件：用户需要专用视图、类型展示或编辑工具时，按[插件指南](references/plugins.md)创建清单与单文件 ES 模块，使用公开的 register/events/data 接口；编辑器安装和启用通过插件面板完成，CLI 不提供插件管理命令。普通数据维护不需要插件。

批量写入用 `apply --input changes.json --rules --if-revision <revision> --json`，需要审阅补丁时加 `--dry-run`。成功已包含所选校验，不重复同范围检查；直接编辑源码后用 `check --rules`。

## 结果处理

单行 `--json` 用于机器处理，`--pretty` 用于人工查看。默认返回计数与失败信息、最多 50 条诊断；`diagnostics_summary` 表示诊断截断，`--details` 展开，显式 `--limit` 仍生效。

按 code、help、record、field_path 修复；请求错误看 request_path、operation_index、target。规则解释用 `check --rule <rule list 返回的标识> --explain`；匿名规则的标识随源码位置变化，不手工构造。

冲突时重读目标、范围与旧值后重新形成修改；旧游标去掉 cursor 后重查，不仅替换 revision 重试。事务失败检查 written、written_files、restored_files、restoration_failed；恢复未完成时先确认实际文件状态。结果以退出码、status 及适用的 complete 判断，详情见[诊断](references/diagnostics.md)。

## 按需参考

- [CLI](references/cli.md)：查询、恢复、批量请求与协议；[公开文档](https://puring103.github.io/coflow/docs/reference/08-cli)。
- [项目配置](references/project-config.md)：输入与生成目录；[公开文档](https://puring103.github.io/coflow/docs/reference/01-project-config)。
- [CFT](references/cft.md)：类型、默认值与规则；[公开文档](https://puring103.github.io/coflow/docs/reference/03-language/01-cft)。
- [CFD](references/cfd.md)：字段值与来源；[公开文档](https://puring103.github.io/coflow/docs/reference/03-language/02-cfd)。
- [维度](references/localization.md)：变体与覆盖；[公开文档](https://puring103.github.io/coflow/docs/reference/10-localization)。
- [诊断](references/diagnostics.md)：位置与修复；[公开文档](https://puring103.github.io/coflow/docs/reference/09-diagnostics/01-diagnostics)。
- [C#](references/csharp.md)：类型、加载与重载；[公开文档](https://puring103.github.io/coflow/docs/reference/07-codegen/01-csharp)。
- [插件](references/plugins.md)：安装、扩展点与插件编写；[公开文档](https://puring103.github.io/coflow/docs/reference/13-plugins)。
- [协作与 CI](references/best-practices.md)：检查范围与并发修改。
