# 诊断与问题定位

CLI、编辑器和 LSP 使用错误码、说明与源码位置帮助定位问题。通过 `coflow check --rules --json` 获取结构化诊断；通过 `coflow explain <错误码> --json` 查看含义与修复方向。

## CLI 诊断

JSON 响应的 `diagnostics` 包含以下信息：

| 字段 | 含义 |
| --- | --- |
| `code` / `stage` / `severity` | 错误码、检查阶段与严重性 |
| `message` | 问题说明 |
| `help` | 可直接采取的简短修复方向 |
| `location` | 来源路径、源码范围或项目配置项路径 |
| `record` / `field_path` | 已识别的记录类型、ID 和字段路径 |
| `related` | 相关声明或数据的来源位置与说明 |
| `contexts` | 规则、量词、维度等上下文 |
| `explanation` | 使用 `--explain` 时提供的解释 |

源码范围的行与列从零开始，列以 UTF-16 偏移计数。无法提供的位置、记录坐标和可选解释省略；调用方应同时展示错误码与问题说明。编辑器和 LSP 将这些位置转换为可跳转的源码或字段位置。

输入规模或结构深度超过限制时，检查返回对应错误码。业务规则达到执行预算时，结果会明确报告检查未完成；完整通过要求没有错误且所选检查全部完成。错误码说明见[错误码索引](./02-codes.md)，命令退出状态见 [CLI 参考](../08-cli.md)。

## 输出范围与修复

CLI 默认返回最多 50 条诊断，`diagnostics_summary` 记录总数、已返回数量和是否截断。`data_scope` 默认只提供加载完整性与文件、拒绝记录计数，`--details` 展开来源列表与原因。先按现有诊断修复；需要展开时使用 `--details`，或用 `--limit` 指定返回数量。两者只控制输出，不改变实际校验范围；`--details --limit 20` 仍最多返回 20 条。

优先根据 `code`、`help`、`record` 和 `field_path` 定位。规则问题可用 `coflow check --rule Item::ValidPrice --explain --json` 获取单条规则的解释，减少重复输出。`complete: false` 表示检查未完整执行，应处理求值错误或预算问题后重试。

请求格式错误通过 `result.request_path` 定位 JSON 字段，操作错误同时提供 `operation_index` 和 `target`。遇到 `conflict`，重新读取目标并核对修订或旧值后提交修改；事务失败时同时查看 `written_files`、`restored_files` 和 `restoration_failed`，确认文件实际状态。
