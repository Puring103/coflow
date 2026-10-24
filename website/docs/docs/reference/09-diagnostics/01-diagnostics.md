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

## 严重性与检查阶段

`severity` 为 `error`、`warning` 或 `info`：错误阻止完整成功，警告和提示可以随成功结果返回。`stage` 表示问题发生的位置，`code` 标识具体原因；定位时结合两者及源码坐标。

| 阶段 | 处理内容 |
| --- | --- |
| `LEX` / `SYN` | CFT 字符、字符串和语法结构 |
| `SCHEMA` / `TYPE` | 声明、继承、默认值、注解和校验表达式类型 |
| `CFD` | CFD 文件读取、语法与赋值 |
| `DATA` | 字段值、必填项、记录身份与数据结构 |
| `REF` | 引用目标及类型 |
| `CHECK` | 业务断言、求值错误与执行预算 |
| `PROJECT` | 配置、输入路径与项目文件 |
| `CLI` | 请求、查询条件、修改和冲突 |
| `ARTIFACT` | 生成代码与产物发布 |
| `WRITE` | 写入来源、排序、迁移及内容冲突 |
| `MUTATION` | 记录、字段与维度修改请求 |
| `RUNTIME` | 项目执行状态 |
| `GIT` | Git 差异与历史输入 |

其他操作阶段随具体命令返回，按[完整错误码索引](./02-codes.md)查询触发条件与修复方向。

## 位置与上下文的完整结构

| 字段 | 内容 |
| --- | --- |
| `location.path` | 来源文件路径；项目内位置通常相对项目根目录 |
| `location.range.start` / `end` | 各含 `line`、`character`，范围起点包含、终点不包含 |
| `location.config_path` | 配置项路径数组，例如 `["dimensions", "language", "dir"]` |
| `record.type` / `record.id` | 记录声明类型与 ID |
| `field_path` | 记录中已定位的字段或嵌套值路径 |
| `related[]` | 每项含 `location` 和可选 `message`，指向关联来源 |
| `contexts[].kind` | 上下文类别 |
| `contexts[].name` / `expression` | 规则或语句名称、相关表达式 |
| `contexts[].quantifier` / `binding` / `item` | 量词、绑定名及元素信息 |
| `contexts[].dimension` / `variant` | 失败所属维度和变体 |
| `explanation` | `--explain` 请求的诊断解释，具体内容随诊断而定 |

只有可获得的信息才返回。类型规则的名称形如 `Item::ValidPrice`；项目规则直接使用名称。规则声明位置与 CFD 数据位置都可用于跳转，量词和维度上下文帮助定位具体失败项。

以下示意一个数据诊断的定位信息（消息以实际输出为准）：

```json
{
  "code": "DATA-006",
  "stage": "DATA",
  "severity": "error",
  "message": "缺少必填字段 name",
  "location": {
    "path": "data/items.cfd",
    "range": {
      "start": { "line": 0, "character": 0 },
      "end": { "line": 0, "character": 27 }
    }
  },
  "record": { "type": "Item", "id": "sword" },
  "field_path": "name"
}
```

## 规则结果与完整性

`coflow check --rules --details --explain --json` 返回规则结果和逐目标任务状态。默认输出用 `rule_counts` 及任务计数汇总，`--details` 展开 `results` 与 `tasks`。

| 状态 | 含义与处理 |
| --- | --- |
| `passed` | 已执行，断言成立 |
| `not_triggered` | 根 when 条件不成立，主体无需执行 |
| `failed` | 业务断言未成立，按消息修正数据 |
| `evaluation_error` | 除零、索引越界等求值问题，修正规则或数据 |
| `budget_exhausted` | 求值预算耗尽，简化规则或缩小检查范围后重试 |
| `not_executed` | 同规则前置错误或预算导致未执行，处理原因后重试 |

普通业务失败会继续收集其他断言错误；求值错误停止当前实例的该条规则，其余规则继续。`complete` 表示检查是否完成，不能用诊断数量或是否分页截断代替判断。

数据查询的 `data_scope.complete` 则表示输入数据是否完整可用；还包含可用记录数、所选文件数、不完整文件数和被拒绝记录数。`--details` 展开文件与拒绝原因。查询有返回数据不代表所有输入都加载成功。

## 错误码解释与命令结果

```sh
coflow explain DATA-006 --json
coflow schema check --explain --json
coflow check --rule Item::ValidPrice --explain --json
```

`explain` 的结果包含 `code`、`stage`、`meaning`、`trigger`、`repair`、`queries`，分别表示错误码、阶段、含义、触发条件、修复方向和建议查询入口。[错误码索引](./02-codes.md)列出全部已定义代码及触发、修复说明。

命令响应使用 `protocol_version`、`command`、`status` 和 `result`，按需附带 `error_category`、`project`、`revision`、`data_scope` 和 `diagnostics`。`diagnostics_summary.total` 是实际诊断数，`returned` 是本次返回数，`truncated` 表示输出截断。

退出码 0 表示完整成功，1 表示校验失败、状态不满足或结果不完整，2 表示请求错误，3 表示修订或前置条件冲突，4 表示 I/O 或执行失败。参数错误、冲突等情况需同时查看响应状态与结果，不能只检查 diagnostics 是否为空。完整协议见 [CLI 输出与退出状态](../08-cli.md#输出与退出状态)。
