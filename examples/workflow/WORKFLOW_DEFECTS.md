# Local Coflow 工作流缺陷清单

本文档汇总 `examples/workflow` 迭代测试中发现的待修复问题。详细执行过程见 `WORKFLOW_TEST_LOG.md`，测试方案见 `WORKFLOW_TEST_PLAN.md`。

## 可以一口气处理的小优化

这些项相对独立、风险较低，适合集中做一轮文档或小型回归补强：

- #20：文档说明 `coflow.enum.lock.json` 保留已删除 key 是稳定编号策略，并标注未来可加 prune/compact。
- #24：文档强调 CFD spread 必须写 `...&key`，不是 `...key`。

## P0：高风险缺陷

### 3. 项目处于引用/解析诊断状态时 writer 自修复能力不足

- 复现轮次：第 16 轮、第 20 轮。
- 现象：
  - 删除被引用的 `Skill.slash` 后，尝试用 `insert_record Skill.slash` 恢复失败，报 `MUTATION-INSERT unknown type Skill`。
  - Excel 单元格出现坏 enum 时，Coflow patch 无法定位 `Item.potion_small` 来修复该字段。
- 影响：
  - 一旦项目进入 `REF` / `CELL` / `DATA` 失败状态，Coflow 无法完成常见自愈操作。
- 建议：
  - writer 需要在部分加载失败状态下保留可写 source/type metadata。
  - 对坏单元格记录提供按 file/sheet/row/key 的降级修复路径。

## P1：重要能力缺口

### 5. 缺少记录/字段迁移命令

- 复现轮次：第 15 轮、第 26 轮。
- 现象：
  - 字段重命名 `price -> buy_price` 时，`sync-header` 删除旧列，新增列使用默认值，旧值丢失。
  - `Monster.drops` 从 Excel 迁移到 CFD `DropTable` 需要人工判断和手工编排。
- 影响：
  - 真实项目 schema 演进时容易丢数据。
  - 迁移流程不可审计，不易回滚。
- 建议新增命令：

```powershell
coflow data migrate-field <project> --from Monster.drops --to DropTable.rewards --plan --apply
coflow data migrate-records <project> --from data/workflow.xlsx --to data/progression --type <Type> --plan --apply
```

- 命令应支持：
  - dry-run 迁移计划。
  - 受影响 source/sheet/record/field 列表。
  - 冲突检测。
  - apply 后项目 check。
  - 可选同步表头删除旧列。

### 10. MessagePack + C# 对自引用项目不兼容

- 复现轮次：第 32 轮。
- 现象：
  - JSON + C# 可构建。
  - 切换到 MessagePack 后，C# codegen 报 `Skill -> Skill` cyclic table reference 不支持。
- 影响：
  - 导出格式切换可能在 build 阶段失败。
- 建议：
  - 在配置检查或 build preflight 中更早提示。
  - 文档说明 MessagePack C# loader 对循环引用的限制。

## P2：诊断与恢复体验

### 11. 文件锁场景错误聚合不干净

- 复现轮次：第 34 轮。
- 现象：
  - Excel 被外部进程独占时，patch 输出 `EXCEL-OPEN`。
  - 同时混入 `MUTATION-PATH record Item.extra_charm was not found`。
- 影响：
  - 调用方可能误判为 record 不存在。
- 建议：
  - source open failure 应作为前置终止错误。
  - 避免继续执行依赖该 source 的 mutation 解析。

### 12. 重复 key 诊断缺少完整冲突位置

- 复现轮次：第 18 轮、第 23 轮、第 28 轮。
- 现象：
  - 多 sheet 同类型重复 key 可被拦截，但没有列出原记录所在 source/sheet。
  - CFD 跨文件重复 key 只定位了其中一个重复位置。
- 影响：
  - 人工修复时仍需手动搜索。
- 建议：
  - duplicate key diagnostic 增加 related labels。
  - 输出所有冲突 record 的 file/sheet/cell/line。

### 13. `data list --human` 不足以支持条件删除

- 复现轮次：第 6 轮。
- 现象：
  - `data list --human` 只显示 key/source，不显示字段值。
  - 删除 `enabled == false` 需要额外 `data get` 或外部筛选。
- 影响：
  - 条件删除工作流繁琐且易错。
- 建议：
  - 增加原生条件删除。
  - 或让 `data list` 支持字段选择、过滤、JSONPath 类筛选。

### 14. 多态对象字段 patch 缺少明确支持方式

- 复现轮次：第 13 轮。
- 现象：
  - `Stage.first_clear_reward.count` 报 `unknown field count on type Reward`。
- 影响：
  - 无法直接编辑多态字段的派生类型字段。
- 建议：
  - 支持 type cast path。
  - 或支持替换整个多态对象。
  - 文档明确当前限制。

### 16. JSON patch flag 组合不支持

- 复现轮次：第 1 轮。
- 现象：
  - `"Consumable | Starter"` 报 unknown enum variant。
  - 数值 `9` 报 expected enum variant。
- 影响：
  - JSON mutation 与表格单元格语法不一致。
- 建议：
  - 支持 flag 数组或组合表达。
  - 文档明确 patch value 语法。

## 本地化与 enum lock

### 20. enum lock 保留已删除 key 需要文档或清理能力

- 复现轮次：第 22 轮、第 33 轮。
- 现象：
  - 删除临时记录后，`coflow.enum.lock.json` 保留历史 key 编号。
  - C# enum 不生成这些已删除 variant。
- 影响：
  - 行为可能是正确的稳定编号策略，但文件会持续增长。
- 建议：
  - 文档说明 lock 文件保留策略。
  - 可选提供 prune/compact 命令。

## Excel / CFD 数据安全边界

### 23. CFD spread 互相引用未报错

- 复现轮次：第 29 轮。
- 现象：
  - 尝试制造 `stage_forest_02` 与 `stage_forest_03` 互相 spread，check 仍通过。
- 影响：
  - 如果语义上不允许，可能缺少循环检测。
- 建议：
  - 确认 spread 语义。
  - 如果不允许循环，增加诊断和回归测试。

### 24. CFD spread 语法需要文档强调

- 复现轮次：第 20 轮。
- 现象：
  - `...stage_forest_02` 报 `ReferenceNeedsMarker`。
  - `...&stage_forest_02` 才通过。
- 影响：
  - 语法易写错。
- 建议：
  - CFD reference 文档中明确 spread 引用必须使用 `&key`。

## 当前表现良好的能力

- 缺 workbook、缺 sheet、坏 enum 单元格、空 id、业务 check 失败等诊断能定位到文件/sheet/cell。
- build 失败时关键 generated 文件未被覆盖，产物安全表现良好。
- patch file guard 能防止写错 source。
- `schema write-file --dry-run --check` 不会落盘。
- `sync-header` 能重建缺失 sheet、删除未知列、同步表头。
- 无引用的 `rename_record` 能同步维度行和 `ItemId.cs`。

## 建议优先级

建议优先处理：

1. 引用/解析失败状态下 writer 自修复能力。
2. 记录/字段迁移命令。
3. 本地化重复 id 和 stale CSV 清理。
4. 文件锁场景错误聚合。

## 已完成修复

### 1. 跨 source `rename_record` 失败后留下半状态

- 复现轮次：第 10 轮。
- 修复提交：`9432e250 Fix local rename rollback`。
- 当前状态：
  - 已修复直接失败原因：CFD writer 现在会在内联多态对象带有类型标记时，用实际子类型导航字段路径。
  - 已验证 `Item.iron_sword -> iron_blade` 在 `examples/workflow` 临时副本上可成功同步 Excel key、Excel 引用和 CFD 多态引用。
  - 已补充本地文件事务：`rename_record` 在所有受影响 source 都是本地文件时，会在写入前保存 bytes 快照；任意引用写入或 rebuild 失败会恢复快照。
  - 仍需后续单独处理：远端 source（例如 Lark）的补偿/事务能力。
- 原始现象：
  - 将 Excel 中的 `Item.iron_sword` 重命名为 `iron_blade`。
  - 命令失败并报告 `applied=0`。
  - 但 Excel key 和 Excel 内引用已经被修改。
  - CFD 引用未同步，项目进入 `CFD-REF-001` 状态。
  - 之后 `data get Item.iron_sword` 和 `data get Item.iron_blade` 都无法定位记录。

### 2. `sync-header` 遇到重复列可能造成静默数据污染

- 复现轮次：第 27 轮。
- 修复提交：`873de0b2 Reject duplicate table headers`。
- 当前状态：
  - 重复原始表头和重复映射表头由 table 层统一诊断为 `TABLE-COLUMN` / `TABLE`。
  - 项目 `check` / load 阶段会直接报错。
  - `sync-header` 不再自行做重复表头修复；它依赖项目加载诊断拒绝继续，从而避免静默保留错误列值。
- 原始现象：
  - Excel `Monster` sheet 人工制造两个 `Power` 列。
  - `check` 能诊断重复映射。
  - 随后 `data sync-header` 恢复表头时保留了后一个重复 `Power` 列中的 `999`。
  - `Monster.slime_green.power` 和 `Monster.goblin_scout.power` 被污染。

### 4. 输出目录位于 source 目录下的安全边界需要确认

- 复现轮次：第 20 轮。
- 修复提交：`b02a5060 Fix patch status and output safety`。
- 当前状态：
  - artifact safety 已把本地文件 source 的父目录纳入 overlap 边界。
  - 已补充回归：`sources: data/items.cfd` 与 `outputs.data.dir: data/generated` 会被拒绝。
- 原始现象：
  - 临时设置 `outputs.data.dir = data/generated` 后 build 被允许。
  - 该目录位于 input source 目录 `data/` 下。

### 6. 大规模 patch 缺少 stdin 输入方式

- 复现轮次：第 33 轮。
- 修复提交：`8bf6cf7b Support stdin data patches`。
- 当前状态：
  - `coflow data patch` 支持 `--stdin`。
  - patch 输入统一为 `--patch JSON`、`--patch-file PATCH_FILE`、`--stdin` 三选一。
  - 已补充 200-op stdin patch 回归测试，覆盖 Windows 命令行长度限制场景。
- 原始现象：
  - 80 条 `insert_record` / `delete_record` 可通过直接 `--patch` 字符串执行。
  - 200 条在 Windows 下无法启动 cargo，报“文件名或扩展名太长”。

### 7. `check_after_write=false` 输出语义不清晰

- 复现轮次：第 12 轮。
- 修复提交：`b02a5060 Fix patch status and output safety`。
- 当前状态：
  - `write_ok` 继续表示写入是否成功。
  - `check_ok` 现在反映最终 session diagnostics 中是否仍有 error，不再因 `check_after_write=false` 误报 true。
- 原始现象：
  - patch 输出 `write_ok=true`、`check_ok=true`、`applied=1`。
  - 同时输出 check diagnostics，并以非零退出。

### 8. `schema write-file --check` 容易被误解为项目级检查

- 复现轮次：第 34 轮。
- 当前状态：
  - 已在 CLI / pipeline 文档明确：`schema write-file --check` 只编译 schema。
  - 它不加载数据源、不同步表头、不构建 DataModel，也不执行 CFT `check {}`。
- 原始现象：
  - schema 新增 `Monster.threat_rating` 后，`schema write-file --check` 通过。
  - 随后 `coflow check` 才发现 Excel 表头缺列。

### 9. `check` 不覆盖 codegen preflight

- 复现轮次：第 25 轮。
- 当前状态：
  - 已在 CLI / pipeline 文档明确：`coflow check` 不执行 exporter / codegen preflight。
  - 启用 codegen 或产物输出的项目，提交产物前应运行 `coflow build`。
- 原始现象：
  - 插入 `Item.class` 后 `check` 通过。
  - `build` 因 C# enum variant 是关键字而失败。

### 15. 新 CFD 文件必须先 `create-file`，不能直接 `write-file`

- 复现轮次：第 19 轮。
- 当前状态：
  - 已在 CLI 文档明确：`data write-file` 不创建新 source。
  - 新增 CFD 文件应先用 `data create-file --provider cfd` 创建并纳入可写范围，再用 `data write-file` 重写内容。
- 原始现象：
  - `data write-file` 到不存在的新 CFD 文件失败。
  - 需要先 `data create-file`。

### 17. 删除 localized 字段后旧维度 CSV 未清理

- 复现轮次：第 31 轮。
- 当前状态：
  - build/generation 阶段会拒绝 `dimensions.*.out_dir` 下不再由当前 schema 管理的 `.csv/.cfd` 文件。
  - Coflow 不自动删除旧变体文件；用户需要确认后手动移除。
- 原始现象：
  - 移除 `Quest.title @localized` 后 build 通过。
  - `data/dimensions/language/Quest_title.csv` 仍存在。

### 18. 维度 CSV 重复 id 未报错

- 复现轮次：第 31 轮。
- 当前状态：
  - 维度 CSV/CFD sync 会拒绝重复 id。
  - 维度表只能修改已有变体值；不能人工新增额外记录。
- 原始现象：
  - `Item_name.csv` 追加重复 `potion_small` 后 check/build 均通过。

### 19. 维度 CSV 不应作为普通 source 重复配置

- 复现轮次：第 17 轮。
- 当前状态：
  - project config 校验会拒绝显式 source 位于 `dimensions.*.out_dir` 下。
  - 报错使用 `DIM-SOURCE-003`，提示该 source 由 Coflow 管理，应从 `sources` 移除。
- 原始现象：
  - 显式添加 `data/dimensions/language/Item_name.csv` 到 `sources` 后，普通 CSV provider 推断为 `Item_name`，报 `CSV-TYPE unknown CFT type Item_name`。

### 21. Excel 公式单元格没有显式诊断

- 复现轮次：第 28 轮。
- 当前状态：
  - Excel loader 会拒绝公式单元格，即使该公式有可读缓存结果。
  - 公式需要先在 Excel 中转成静态值再作为配置输入。
- 原始现象：
  - 将 `Item.extra_charm.buy_price` 写成 `=10+20`。
  - check 通过，`data get` 读到旧缓存值。

### 22. 合并单元格诊断可以更明确

- 复现轮次：第 28 轮。
- 当前状态：
  - Excel loader 会拒绝 `.xls` / `.xlsx` / `.xlsm` 中的合并单元格。
  - 诊断定位到合并区域左上角单元格。
- 原始现象：
  - 合并 `Monster!B2:B3` 后，第二行报 `missing required field name`。
