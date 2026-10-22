# 使用 AI Agent

Coflow 提供结构化查询、批量修改、业务校验与可定位诊断。Agent 可以先取得当前任务的上下文，再在一次候选事务中完成修改与检查。

## 安装 Skills

CLI 内置 `coflow-workflow` 与 `coflow-schema`，不依赖 Node.js。前者处理查询、CFD 编辑、项目配置、诊断与代码生成；后者处理 CFT 建模、字段、命名规则、引用与维度。

```powershell
coflow skill install --project <project>
coflow skill install -g
```

分别安装到当前项目或当前用户的通用及已检测 agent 目录。使用 `coflow skill status` / `coflow skill status -g` 查看状态，使用对应 `skill uninstall` 命令卸载。

## 给 Agent 的任务

提供项目路径、目标与修改范围，例如“在 `my-game/config` 中将所有普通药水价格降低 10%，验证业务规则”。业务取值、修改范围和发布授权由任务约定；skills 不会增加固定审批步骤。

Agent 可以按需查询：

```powershell
coflow describe --json
coflow describe apply --json
coflow describe apply --schema input --json
coflow context --type Item --project <project> --examples 2 --json
coflow data list --type Item --fields name,price --limit 20 --project <project> --json
```

`describe` 先返回命令目录，单命令查询提供参数与示例，完整 JSON Schema 按需展开。`context`、字段选择和分页帮助 agent 获取所需信息，避免反复读取整个项目。

批量修改推荐：

```powershell
coflow apply --input changes.json --project <project> --rules --if-revision <revision> --json
```

`revision` 使用前一次查询返回的值；也可放在变更集顶层的 `if_revision`。需要先审阅源码补丁时加 `--dry-run`。CFT 与 CFD 联合修改用文件模式，记录、数组、字典和维度修改用操作模式，准确格式由 `describe apply --schema input` 提供。

`--rules` 同时验证结构、数据与业务规则，成功后无需再执行相同范围的 `check`。直接编辑源码后用 `check --rules`；需要生成代码时用 `build`。普通 `check` 默认只检查结构与数据。

## 精简输出与修复

`--json` 返回单行 JSON；人工查看时用 `--pretty`。默认结果提供计数与失败信息，诊断最多 50 条；`diagnostics_summary` 明确报告总数与截断。只有需要完整任务明细或更多诊断时才加 `--details`，显式 `--limit` 继续生效。

先看 `code`、`help`、记录和字段位置；需要深入定位时用 `check --rule Type::Rule --explain --json`。请求格式错误提供 JSON 路径与操作序号。发生修订冲突时重新读取目标；旧分页游标失效时从第一页查询。失败写入结果保留实际写入与恢复情况。

详见 [CLI 参考](../reference/08-cli.md)、[诊断](../reference/09-diagnostics/01-diagnostics.md)和 [CFT 参考](../reference/03-language/01-cft.md)。Skills 中的参考快照与公开文档同步，按当前任务需要读取即可。
