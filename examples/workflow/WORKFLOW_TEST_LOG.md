# Local Coflow 工作流测试日志

本文档记录 `examples/workflow` 的实际执行结果。执行规则：先尝试 Coflow 命令；不使用 patch 文件；所有 `data patch` 使用 `--patch` 直接传 JSON 字符串；任何 fallback 都明确记录。

## 第 0 轮：项目基线

日期：2026-07-10

改动：

- 创建 `coflow.yaml`，配置 `schema/main.cft`、`data/workflow.xlsx`、JSON/C# outputs。
- 创建初始 `schema/main.cft`，包含 `Rarity`、`ItemTag`、`Item`。

尝试的 Coflow 命令：

- `cargo run -- schema inspect examples/workflow`
- `cargo run -- data sources examples/workflow`
- `cargo run -- check examples/workflow`

结果：

- schema inspect：通过。
- data sources：失败，`PROJECT-001`，`data/workflow.xlsx` 不存在。
- check：失败，`PROJECT-001`，诊断指向 `coflow.yaml`。

Fallback：

- 无。

观察到的诊断：

- `PROJECT-001`：缺失 workbook 的诊断清楚，能定位到 source path。

## 第 1 轮：新增 Item

日期：2026-07-10

改动：

- 用 Coflow 创建 `data/workflow.xlsx` 的 `Item` sheet。
- 用 `data patch --patch` 字符串插入 `potion_small` 和 `iron_sword`。

尝试的 Coflow 命令：

- `cargo run -- data create-file examples/workflow --file data/workflow.xlsx --provider excel --type Item --sheet Item --human`
- `cargo run -- data patch examples/workflow --patch '{...}' --human`
- `cargo run -- data sources examples/workflow`
- `cargo run -- check examples/workflow`
- `cargo run -- build examples/workflow`

结果：

- data create-file：通过，创建 workbook 和 `Item` sheet。
- data patch：先后两次失败，最终用单个 flag variant 写入成功，`applied=2`。
- data sources：通过，source capabilities 显示 Excel 可 edit/insert/delete。
- check：通过。
- build：通过，生成 `generated/data` 和 `generated/csharp`。

Fallback：

- 无。

观察到的诊断：

- `data create-file` 生成的 key 列为 `id`，没有使用计划中 `Item ID` 这样的显示列名。已调整 `coflow.yaml` 使用实际表头。
- JSON patch 写 `tags: "Consumable | Starter"` 会报 `MUTATION-VALUE unknown enum variant`。
- JSON patch 写 `tags: 9` 会报 `MUTATION-VALUE expected enum variant`。
- 当前 mutation JSON 只接受单个 enum/flag variant 字符串；flag 组合单元格语法不适用于 JSON patch 值。

## 第 2 轮：修改 Item 并同步表头

日期：2026-07-10

改动：

- `Item` 新增 `@localized name`、`@localized description`、`level_required`、`sellable`。
- `coflow.yaml` 启用 `dimensions.language`。
- 用 `data sync-header` 同步 Excel 表头。

尝试的 Coflow 命令：

- `cargo run -- schema write-file examples/workflow --file schema/main.cft --stdin --check --human`
- `cargo run -- data sync-header examples/workflow --file data/workflow.xlsx --provider excel --type Item --sheet Item --human`
- `cargo run -- data patch examples/workflow --patch '{...}' --human`
- `cargo run -- check examples/workflow`
- `cargo run -- build examples/workflow`

结果：

- schema write-file：通过。
- sync-header：通过，新增 `Description`、`Level Required`、`Sellable`。
- data patch：通过，`applied=4`。
- check/build：通过。
- build 生成 `Item_description.csv` 和 `Item_name.csv`。

Fallback：

- 无。

## 第 3 轮：批量增加 Item

日期：2026-07-10

改动：

- 用一条 `--patch` 字符串批量插入 5 条 `Item`。

结果：

- data patch：通过，`applied=5`、`failed=0`。
- 命令行直接传较长 JSON 字符串可用。

Fallback：

- 无。

## 第 4 轮：新增 Skill

日期：2026-07-10

改动：

- 新增 `Element`、`SkillTag`、`Skill`。
- 用 Coflow 创建 `Skill` sheet 并插入 `slash`、`fireball`、`guard_stance`。
- 故意设置 `fireball.follow_up -> missing_skill`。

结果：

- schema write-file：通过。
- data create-file：通过，创建 `Skill` sheet。
- data patch：正常写入通过，`applied=3`。
- 坏引用 patch：失败且 `applied=0`，没有污染 Excel。
- check：坏引用测试后仍通过。

观察到的诊断：

- `MUTATION-SHAPE`：`ref target Skill with key missing_skill was not found`。

## 第 5 轮：新增 Monster 和业务 check

日期：2026-07-10

改动：

- 新增 `Monster`，包含跨 sheet 引用和引用数组。
- 用 Coflow 创建 `Monster` sheet，插入 `slime_green`、`goblin_scout`、`event_dummy`。
- 故意把 `slime_green.hp` 设为 `0`。

结果：

- data create-file：通过。
- data patch：正常写入通过，`applied=3`。
- 坏 check patch：写入成功但 `check_ok=false`。
- check/build：均失败，诊断定位到 `data/workflow.xlsx`、`Monster`、`D2`。
- 修复 hp 后 check 恢复通过。

观察到的诊断：

- `CFD-CHECK-007`：`hp > 0` 校验失败，包含 sheet 和 cell。

## 第 6 轮：按照条件删除记录

日期：2026-07-10

改动：

- 目标条件：删除 `Monster.enabled == false` 的记录。
- 用 `data list` 查看 Monster，再生成 `delete_record` patch 删除 `event_dummy`。

结果：

- `data list --human` 只显示 key/source，不显示字段值。
- `delete_record` patch 通过，`applied=1`。
- check：通过。

待修复问题：

- 当前 CLI 没有原生条件删除命令。
- `data list --human` 不足以直接筛选字段；需要 `data get`、JSON 输出或外部筛选后生成 patch。

## 第 7 轮：删除整张表并恢复

日期：2026-07-10

改动：

- 删除 `Monster` sheet 以制造缺失整表状态。
- 用 Coflow 检查诊断。
- 用 Coflow `sync-header` 重建 sheet。
- 用 Coflow `data patch --patch` 恢复 `slime_green` 和 `goblin_scout`。

结果：

- data sources/check：失败，`EXCEL-SHEET`，定位 `data/workflow.xlsx`、`Monster`、`A1`。
- sync-header：通过，重建 `Monster` sheet 和表头。
- data patch：通过，`applied=2`。

Fallback：

- Coflow 命令：无原生“删除 Excel sheet”命令。
  失败原因：测试需要制造缺失 sheet 状态。
  Fallback 命令/操作：用 Python/openpyxl 删除 `Monster` sheet。
  影响的 source/sheet/record：`data/workflow.xlsx` / `Monster` / all Monster records。
  分类：预期不支持边界。

## 第 8 轮：新增 CFD 复杂结构

日期：2026-07-10

改动：

- 新增 `Reward`、`ItemReward`、`CurrencyReward`、`DropTable`、`Stage`、`Quest`。
- `coflow.yaml` 新增 `data/progression.cfd` source。
- 用 Coflow 创建 CFD 文件并通过 stdin 写入复杂结构。

结果：

- schema write-file：第一次失败，`isUnique()` 不支持引用数组元素类型；修正后 schema inspect 通过。
- data create-file：通过，创建 `data/progression.cfd`。
- data write-file：通过，`check_ok=true`。
- check/build：通过。

待修复问题：

- `schema write-file --check` 在 check 失败时仍写入文件；测试中已立即修正。

## 第 9 轮：本地化

日期：2026-07-10

改动：

- build 生成 `Item_description.csv`、`Item_name.csv`、`Monster_name.csv`、`Quest_title.csv`、`Skill_name.csv`、`Stage_name.csv`。
- 用 Coflow patch 修改 `Item_nameVariants.potion_small.zh/en` 和 `Stage_nameVariants.stage_forest_01.zh`。

结果：

- data patch：通过，`applied=3`。
- data sources：最终列出 Excel、CFD 和 6 个维度 CSV source。
- check：通过。
- build：通过。

## 最终验证

日期：2026-07-10

命令：

- `cargo run -- data sources examples/workflow`
- `cargo run -- check examples/workflow`
- `cargo run -- build examples/workflow`

结果：

- data sources：通过，无 diagnostics。
- check：通过。
- build：通过，生成 JSON 和 C#。

下一步：

- `cargo check --workspace`：通过。
- `cargo test --workspace`：通过。

## 第 10 轮：跨 source rename_record

日期：2026-07-10

改动：

- 尝试将 Excel 中的 `Item.iron_sword` 重命名为 `iron_blade`。
- 该 Item 被 Excel `Monster.drops`、CFD `DropTable`、CFD `Stage.first_clear_reward`、CFD `Quest.reward_item` 引用。

尝试的 Coflow 命令：

- `cargo run -- data patch examples/workflow --patch '{"ops":[{"op":"rename_record","record":{"type":"Item","key":"iron_sword"},"file":"data/workflow.xlsx","new_key":"iron_blade"}]}' --human`
- `cargo run -- data get examples/workflow Item.iron_sword`
- `cargo run -- data get examples/workflow Item.iron_blade`
- `cargo run -- check examples/workflow`

结果：

- rename patch 失败，`write_ok=false`、`applied=0`、`failed=1`。
- 但 Excel key 实际已从 `iron_sword` 变成 `iron_blade`。
- Excel 中 `Monster.goblin_scout.drops` 也被改成 `iron_blade`。
- CFD 中引用没有同步，项目进入 `CFD-REF-001` 状态。
- `data get Item.iron_sword` 和 `data get Item.iron_blade` 都找不到记录。
- 在引用诊断状态下，Coflow 也无法通过 patch 修复 `Monster.goblin_scout.drops`，报 `record Monster.goblin_scout was not found`。

Fallback：

- Coflow 命令：反向 rename 和 set_field 修复。
  失败原因：rename 失败后项目进入半状态，Coflow session 无法定位当前 record。
  Fallback 命令/操作：用 Python/openpyxl 把 Item key 改回 `iron_sword`，并把 `Monster.goblin_scout.drops` 改回 `&iron_sword | &potion_small`。
  影响的 source/sheet/record：`data/workflow.xlsx` / `Item`、`Monster` / `Item.iron_sword`、`Monster.goblin_scout`。
  分类：产品缺陷。

待修复问题：

- `rename_record` 跨 Excel/CFD 引用更新失败时不应留下半状态。
- 失败报告 `applied=0` 与实际 Excel 修改不一致。
- rename 失败后 session 读不到旧 key 或新 key，修复能力受限。

## 第 11 轮：部分失败 patch 和 stop_on_write_error=false

日期：2026-07-10

改动：

- 一个 patch 中先成功修改 `Item.potion_small.price`，再修改不存在的 `Item.missing_item`。
- 另一个 patch 使用 `stop_on_write_error=false`，混合两个失败和一个成功。

结果：

- 默认行为：第 0 步成功，第 1 步失败，第 2 步未执行，报告 `applied=1`、`failed=1`。
- `stop_on_write_error=false`：失败后继续执行，报告 `applied=1`、`failed=2`。
- 已成功写入的字段需要后续 patch 显式恢复。

## 第 12 轮：check_after_write=false

日期：2026-07-10

改动：

- 使用 `check_after_write=false` 将 `Monster.slime_green.hp` 写成 `0`。

结果：

- patch 输出 `write_ok=true`、`check_ok=true`、`applied=1`，但同时输出 `CFD-CHECK-007` 并以非零退出。
- 独立运行 `check` 和 `build` 均失败，诊断定位 `data/workflow.xlsx`、`Monster`、`D2`。
- 修复 hp 后项目恢复通过。

待修复问题：

- `check_after_write=false` 时输出里同时出现 `check_ok=true` 和 check 诊断，语义不清晰。

## 第 13 轮：CFD 嵌套路径 patch

日期：2026-07-10

改动：

- 修改 `Stage.stage_forest_02.spawns[0].count`。
- 尝试修改多态字段 `Stage.stage_forest_02.first_clear_reward.count`。

结果：

- 数组嵌套路径 `spawns[0].count` 写入成功。
- 多态字段路径失败，报 `unknown field count on type Reward`。
- 因为默认 stop-on-error，第一个成功修改保留，第二个失败后项目仍通过 check。

待修复问题：

- 多态对象字段 patch 需要明确支持方式，例如 type cast path 或替换整个对象。

## 第 14 轮：本地化变体保留和坏值诊断

日期：2026-07-10

改动：

- 再次运行 `build`，确认 `Item_nameVariants.potion_small.zh/en` 保留。
- 将 `Item_nameVariants.potion_small.zh` 写为空字符串。
- 修复后发现 `test` 是源 Excel 中的残留 `Item.test` 记录；先删除源记录，再由 build 清理维度行。

结果：

- build 保留 `potion_small` 的 `zh=小药水`、`en=Small Potion`。
- 空 zh 值触发 `CFD-CHECK-007 [language=zh]`，诊断说明 `name != ""` 失败。
- 先用 Coflow 删除 `Item_nameVariants.test` 和 `Item_descriptionVariants.test`，但 build 会从源 `Item.test` 重新生成。
- 用 Coflow 删除源记录 `Item.test` 后，build 不再生成 `test` 维度行。

观察：

- 维度 CSV 中曾残留 `test` record；最终确认它来自源 Excel 的 `Item.test` 记录，不是纯维度 stale record。

## 第 15 轮：字段重命名、删除和表头同步

日期：2026-07-10

改动：

- 将 `Item.price` 重命名为 `Item.buy_price`。
- 删除 `Item.sellable`。
- `coflow.yaml` 中 `Item` sheet 的列从 `Price` 改为 `Buy Price`，并移除 `Sellable`。
- 运行 `data sync-header`。

结果：

- `sync-header` 通过，报告新增 `Buy Price`，删除 `Price`、`Sellable`。
- Excel 旧价格列被删除，新 `Buy Price` 列使用 schema 默认值 `0`。
- 字段重命名不会自动迁移旧列数据；已用 Coflow patch 批量恢复关键记录价格。

待修复问题：

- 字段重命名/列迁移需要显式迁移命令或文档化流程，否则 `sync-header` 会丢弃旧列值。

## 第 16 轮：删除被引用记录和产物安全

日期：2026-07-10

改动：

- 删除被引用的 `Skill.slash`。
- 记录失败 build 前后 `generated/data/Skill.json`、`generated/data/Monster.json` hash。

结果：

- delete patch 写入成功，但 `check_ok=false`，报告 3 个 `CFD-REF-001`。
- `build` 失败，诊断定位 `Skill!F2`、`Monster!F2`、`Monster!F3`。
- 失败 build 后关键 generated 文件 hash 未变化，产物安全通过。
- 在引用诊断状态下，尝试用 Coflow `insert_record Skill.slash` 恢复失败，报 `MUTATION-INSERT unknown type Skill`。

Fallback：

- Coflow 命令：`data patch` 插入 `Skill.slash`。
  失败原因：引用诊断状态下恢复插入报 unknown type。
  Fallback 命令/操作：用 Python/openpyxl 补回 `Skill.slash` 行。
  影响的 source/sheet/record：`data/workflow.xlsx` / `Skill` / `Skill.slash`。
  分类：产品缺陷。

## 第 17 轮：本地化语言增删和显式维度 source 冲突

日期：2026-07-10

改动：

- 将 `dimensions.language.variants` 从 `[zh, en]` 改为 `[zh, en, ja]` 并 build。
- 再改为 `[zh, ja]` 并 build。
- 临时把 `data/dimensions/language/Item_name.csv` 作为显式 source 添加到 `coflow.yaml`。

结果：

- 新增 `ja` 后，维度 CSV 表头变为 `id,default,zh,en,ja`，原 `zh/en` 值保留。
- 删除 `en` 后，维度 CSV 表头变为 `id,default,zh,ja`，旧 `en` 列被删除。
- 显式添加维度 CSV source 会被普通 CSV provider 推断为 `Item_name` 类型，报 `CSV-TYPE unknown CFT type Item_name`。

待修复问题：

- 维度 source 不应作为普通 source 重复配置；文档可明确说明。

## 第 18 轮：多 sheet 同类型和重复 key

日期：2026-07-10

改动：

- 在同一 workbook 中新增 `ItemExtra` sheet，并映射到同一个 `Item` 类型。
- 不指定 sheet 插入 `Item.extra_charm`。
- 指定 `sheet: ItemExtra` 插入 `Item.extra_ring`。
- 在 `ItemExtra` 尝试插入重复 key `potion_small`。

结果：

- `sync-header` 可以创建第二张同类型 sheet。
- 不指定 sheet 时，`insert_record` 默认写入第一张匹配的 `Item` sheet。
- 指定 `sheet: ItemExtra` 时写入目标 sheet。
- 重复 key 在写前被拦截，报 `MUTATION-INSERT key potion_small already exists in Item inheritance domain`。

待修复问题：

- 重复 key 诊断说明了冲突 key，但没有直接给出原记录所在 source/sheet。

## 第 19 轮：CFD source 移动、拆分和 patch 目标文件

日期：2026-07-10

改动：

- 将 `data/progression.cfd` 移动到 `data/progression/main.cfd`。
- `coflow.yaml` 的 source 改为目录 `data/progression`。
- 新增 `data/progression/quests.cfd`，把 `Quest` 记录拆出。
- 用 `data patch` 指定 `file: data/progression/quests.cfd` 修改 `Quest.quest_first_blade.min_level`。

结果：

- 目录 source 能发现 `main.cfd` 和 `quests.cfd`。
- 直接 `data write-file` 到不存在的新 CFD 文件失败，需先 `data create-file`。
- 拆分后 check/build 通过。
- 指定 file guard 的 patch 正确写入 `quests.cfd`。

## 第 20 轮：输出目录安全、坏单元格和 CFD spread

日期：2026-07-10

改动：

- 临时把 `outputs.data.dir` 改为 `data/generated`。
- 用 fallback 把 Excel `Item.potion_small.Rarity` 改成 `BadRarity`。
- 在 CFD 中新增 `stage_forest_03`，使用 spread 复用 `stage_forest_02`。
- patch 修改 `stage_forest_03.recommended_power`。

结果：

- build 允许输出到 `data/generated`，没有拒绝输出目录位于 source 目录下；已清理该测试产物并恢复配置。
- 坏 enum 单元格触发 `CELL-InvalidEnumVariant`，定位 `data/workflow.xlsx` / `Item` / `D2`。
- 在坏单元格状态下，Coflow patch 无法定位 `Item.potion_small` 来修复该单元格，只能 fallback 修复。
- CFD spread 写 `...stage_forest_02` 会报 `ReferenceNeedsMarker`；改为 `...&stage_forest_02` 后 check 通过。
- CFD writer 对 spread 派生记录的字段 patch 会保留 spread 结构，只更新覆盖字段。

待修复问题：

- 输出目录位于 input source 目录下未被拒绝，存在 artifact/source overlap 风险。
- 坏解析单元格状态下，writer 无法自修复同一记录字段。

## 第 21 轮：给 Item 增加 @idAsEnum

日期：2026-07-10

改动：

- 新增空 enum `ItemId`。
- 给 `Item` 增加 `@idAsEnum(ItemId)`。

尝试的 Coflow 命令：

- `cargo run -- schema write-file examples/workflow --file schema/main.cft --stdin --check --human`
- `cargo run -- schema inspect examples/workflow`
- `cargo run -- build examples/workflow`

结果：

- schema write-file：通过，`check_ok=true`。
- schema inspect：`Item` annotations 中出现 `idAsEnum(ItemId)`，`ItemId` 是空 enum。
- build：通过，生成 `coflow.enum.lock.json` 和 `generated/csharp/ItemId.cs`。
- C# 中 `Table<string, Item>` 变为 `Table<ItemId, Item>`，引用 Item 的读取代码也切换为 `ItemId` enum。

Fallback：

- 无。

## 第 22 轮：大批量插入、删除和 enum lock 行为

日期：2026-07-10

改动：

- 用一条 `--patch` 字符串向 `ItemExtra` 插入 `season_mat_01` 到 `season_mat_40`。
- 运行 check/build。
- 再用一条 `--patch` 字符串批量删除这 40 条记录。

尝试的 Coflow 命令：

- `cargo run -- data patch examples/workflow --patch '<40 个 insert_record 的 JSON 字符串>' --human`
- `cargo run -- check examples/workflow`
- `cargo run -- build examples/workflow`
- `cargo run -- data patch examples/workflow --patch '<40 个 delete_record 的 JSON 字符串>' --human`
- `cargo run -- build examples/workflow`

结果：

- 批量插入：通过，`applied=40`、`failed=0`。
- 批量删除：通过，`applied=40`、`failed=0`。
- 删除后 `generated/data` 和维度 CSV 中没有 `season_mat` 残留。
- `coflow.enum.lock.json` 保留已删除 key 的历史数值占位，但 `generated/csharp/ItemId.cs` 不生成这些已删除 variant。

观察：

- enum lock 的保留行为有利于稳定编号，但会让 lock 文件包含当前数据中不存在的 key；这需要文档说明或提供清理策略。

## 第 23 轮：CFD 目录 source 跨文件重复 key

日期：2026-07-10

改动：

- 用 `data write-file --stdin` 临时在 `data/progression/main.cfd` 中新增 `Quest.quest_first_blade`。
- `data/progression/quests.cfd` 中已经存在同 key。
- 验证诊断后，用 `data write-file --stdin --check` 恢复 `main.cfd`。

尝试的 Coflow 命令：

- `cargo run -- data write-file examples/workflow --file data/progression/main.cfd --stdin --human`
- `cargo run -- check examples/workflow`
- `cargo run -- data sources examples/workflow`
- `cargo run -- data write-file examples/workflow --file data/progression/main.cfd --stdin --check --human`

结果：

- 临时写入：通过，未自动 check。
- check：失败，`CFD-DATA-011 duplicate key in table Quest`。
- data sources：失败并返回 diagnostics。
- 恢复：通过，`check_ok=true`。

观察到的诊断：

- `CFD-DATA-011`：诊断 file 指向 `data/progression/quests.cfd`，record_key 为 `quest_first_blade`。
- 诊断能指出重复记录之一，但没有同时列出另一个重复 key 所在文件。

## 第 24 轮：patch file guard 防误写

日期：2026-07-10

改动：

- 故意对 `Monster.slime_green.hp` 使用错误 `file` guard：`data/progression/main.cfd`。
- 真实记录位于 `data/workflow.xlsx`。

尝试的 Coflow 命令：

- `cargo run -- data patch examples/workflow --patch '{"ops":[{"op":"set_field","record":{"type":"Monster","key":"slime_green"},"file":"data/progression/main.cfd","path":[{"kind":"field","value":"hp"}],"value":999}]}' --human`
- `cargo run -- check examples/workflow`

结果：

- patch：失败，`applied=0`、`failed=1`。
- 诊断：`MUTATION-FILE-GUARD record Monster.slime_green writes to data/workflow.xlsx, not data/progression/main.cfd`。
- check：通过，确认没有误写。

## 第 25 轮：配置错误、schema dry-run 和 codegen 边界

日期：2026-07-10

改动：

- 临时在 `coflow.yaml` 中增加不存在的 Excel sheet `MissingSheet`，验证配置诊断后恢复。
- 使用 `schema write-file --dry-run --check` 输入无效 schema。
- 插入 `Item.class`，验证 C# 关键字 key 对 `@idAsEnum` codegen 的影响，再删除。

尝试的 Coflow 命令：

- `cargo run -- data sources examples/workflow`
- `cargo run -- check examples/workflow`
- `cargo run -- schema write-file examples/workflow --file schema/main.cft --stdin --dry-run --check --human`
- `cargo run -- data patch examples/workflow --patch '<insert Item.class 的 JSON 字符串>' --human`
- `cargo run -- build examples/workflow`
- `cargo run -- data patch examples/workflow --patch '<delete Item.class 的 JSON 字符串>' --human`
- `cargo run -- build examples/workflow`

结果：

- 缺失 sheet：`data sources` 和 `check` 均失败，`EXCEL-SHEET` 定位到 `data/workflow.xlsx` / `MissingSheet` / `A1`。
- schema dry-run：失败，`written=false`、`dry_run=true`、`check_ok=false`，`CFT-SCHEMA-006 unknown @idAsEnum enum MissingEnum`；检查确认 `schema/main.cft` 没有被改写。
- `Item.class`：data patch 和 check 均通过。
- build：失败，`CSHARP-CODEGEN invalid C# @idAsEnum enum variant name class: identifier is a C# keyword`。
- 删除 `Item.class` 后 build 恢复通过。

待修复问题：

- `check` 不覆盖 codegen preflight；启用 `@idAsEnum` 的项目必须把 `build` 纳入 CI，否则 C# 关键字 key 会漏到构建阶段才失败。

## 最终验证（二）

日期：2026-07-10

命令：

- `cargo run -- data sources examples/workflow`
- `cargo run -- check examples/workflow`
- `cargo run -- build examples/workflow`
- `cargo check --workspace`
- `cargo test --workspace`

结果：

- data sources：通过，无 diagnostics。
- check：通过。
- build：通过，JSON 和 C# 产物重新生成。
- cargo check：通过。
- cargo test：通过。

## 第 26 轮：Monster.drops 从 Excel 迁移到 CFD DropTable

日期：2026-07-10

改动：

- 迁移目标：`Monster.drops` 不再由 Excel `Monster` sheet 维护，掉落配置统一由 CFD `DropTable.rewards` 表达。
- 迁移前确认 `DropTable.drop_slime_green` 和 `DropTable.drop_goblin_scout` 已包含对应 ItemReward。
- 从 `coflow.yaml` 的 `Monster` sheet 映射中删除 `Drop Item IDs: drops`。
- 用 `schema write-file` 从 `Monster` 中删除 `drops: [&Item]` 字段和对应 check。
- 用 `data sync-header` 删除 Excel `Monster` sheet 的 `Drop Item IDs` 列。

尝试的 Coflow 命令：

- `cargo run -- data get examples/workflow Monster.slime_green`
- `cargo run -- data get examples/workflow DropTable.drop_slime_green`
- `cargo run -- schema write-file examples/workflow --file schema/main.cft --stdin --check --human`
- `cargo run -- data sync-header examples/workflow --file data/workflow.xlsx --provider excel --type Monster --sheet Monster --human`
- `cargo run -- check examples/workflow`
- `cargo run -- build examples/workflow`

结果：

- schema write-file：通过，`check_ok=true`。
- sync-header：通过，报告 `removed Drop Item IDs`。
- check/build：通过。
- 迁移后 `Monster` 记录不再包含 `drops` 字段，掉落数据保留在 CFD `DropTable`。

Fallback：

- 无。

待修复问题：

- 当前迁移需要人工判断“旧字段数据已在新 source 中表达”，再手工改 schema/yaml/sync-header。
- 未来应该增加记录/字段迁移命令，例如 `coflow data migrate-field` 和 `coflow data migrate-records`，支持先生成迁移计划、再执行、最后可选同步表头删除旧列。

## 第 27 轮：Excel 表头异常和 sync-header 恢复

日期：2026-07-10

改动：

- 用 fallback 修改 `data/workflow.xlsx` 的 `Monster` sheet：
  - 打乱列顺序。
  - 增加未映射列 `Designer Note`。
- 运行 `check` 和 `data sources` 观察诊断。
- 用 `data sync-header` 恢复。
- 再用 fallback 增加重复 `Power` 列，运行诊断并恢复。

尝试的 Coflow 命令：

- `cargo run -- check examples/workflow`
- `cargo run -- data sources examples/workflow`
- `cargo run -- data sync-header examples/workflow --file data/workflow.xlsx --provider excel --type Monster --sheet Monster --human`
- `cargo run -- data patch examples/workflow --patch '<恢复 Monster.power 的 JSON 字符串>' --human`
- `cargo run -- build examples/workflow`

结果：

- 多余列 `Designer Note`：触发 `EXCEL-COLUMN`，定位 `Monster!H1`，message 为 `column Designer Note maps to unknown field Designer Note on type Monster`。
- 只打乱映射列顺序不影响加载；问题来自未知列。
- sync-header 可以删除 `Designer Note`，再次 check 通过。
- 重复 `Power` 列：触发 `EXCEL-COLUMN`，报告 `field power is mapped by both Power and Power`，同时报告缺少 `enabled`。
- sync-header 恢复表头后，`Power` 值保留了后一个重复列中的 `999`，导致 `slime_green` 和 `goblin_scout` 的业务值被污染。
- 用 Coflow patch 恢复 `Monster.slime_green.power=12`、`Monster.goblin_scout.power=45` 后 check/build 通过。

Fallback：

- Coflow 命令：无原生“制造 Excel 表头异常”命令。
  失败原因：测试需要模拟人工编辑 Excel 导致的多余列、列顺序变化和重复列。
  Fallback 命令/操作：用 Python/openpyxl 修改 `data/workflow.xlsx`。
  影响的 source/sheet/record：`data/workflow.xlsx` / `Monster` / `slime_green`、`goblin_scout`。
  分类：测试数据问题，用于验证诊断和恢复能力。

待修复问题：

- `sync-header` 面对重复映射列时会保留后一个重复列的数据，可能造成静默数据污染。
- 建议 `sync-header` 在重复列诊断状态下拒绝自动恢复，或要求显式选择保留哪一列。

## 第 28 轮：Excel 人工编辑破坏

日期：2026-07-10

改动：

- 用 fallback 在 `ItemExtra` 追加重复 key `potion_small`。
- 用 fallback 在 `ItemExtra` 追加空 key 行。
- 用 fallback 把 `Item.extra_charm.buy_price` 单元格写成公式 `=10+20`。
- 用 fallback 合并 `Monster!B2:B3`。

尝试的 Coflow 命令：

- `cargo run -- check examples/workflow`
- `cargo run -- data get examples/workflow Item.extra_charm`
- `cargo run -- data patch examples/workflow --patch '<恢复 extra_charm.buy_price 的 JSON 字符串>' --human`
- `cargo run -- data patch examples/workflow --patch '<恢复 Monster.goblin_scout.name 的 JSON 字符串>' --human`

结果：

- 重复 key：`CFD-DATA-011 duplicate key in table Item`，定位 `data/workflow.xlsx` / `ItemExtra` / `A3`。
- 空 key：`EXCEL-ID empty id cell`，定位 `ItemExtra!A3`。
- 公式单元格：check 未报错，`data get` 仍读到旧缓存值 `300`，说明公式可能按 workbook 缓存值读取。
- 合并单元格：`CFD-DATA-006 missing required field name`，定位 `Monster!B3`。
- 恢复后 check 通过。

Fallback：

- Coflow 命令：无原生“制造 Excel 人工破坏”命令。
  失败原因：需要模拟人工编辑 Excel 的异常状态。
  Fallback 命令/操作：用 Python/openpyxl 追加行、写公式、合并/取消合并单元格。
  影响的 source/sheet/record：`data/workflow.xlsx` / `ItemExtra`、`Monster`。
  分类：测试数据问题。

待修复问题：

- 公式单元格没有显式诊断，可能读取旧缓存值，存在人工编辑后数据不透明风险。

## 第 29 轮：CFD 人工编辑破坏

日期：2026-07-10

改动：

- 截断 `data/progression/main.cfd` 尾部制造语法错误。
- 将 `monster: &slime_green` 改为 `monster: slime_green`。
- 在同一文件追加重复 `Stage.stage_forest_02`。
- 尝试制造 `stage_forest_02` 与 `stage_forest_03` 的 spread 互相引用。

尝试的 Coflow 命令：

- `cargo run -- check examples/workflow`
- `cargo run -- data write-file examples/workflow --file data/progression/main.cfd --stdin --check --human`

结果：

- 截断文件：`CFD-TEXT-Syntax record key is missing`。
- 无效记录引用：`CFD-TEXT-Syntax invalid record reference`。
- 同文件重复 key：`CFD-DATA-011 duplicate key in table Stage`，定位新增重复记录行。
- spread 互相引用：check 仍通过，未观察到循环诊断。
- 每次异常后均通过 `data write-file --stdin --check` 恢复。

待修复问题：

- spread 互相引用未报错，需要确认这是预期覆盖语义还是缺少循环检测。

## 第 30 轮：引用图级联删除

日期：2026-07-10

改动：

- 删除 `Stage.stage_forest_01`。
- 删除 `Quest.quest_first_potion`。
- 观察跨文件引用诊断后恢复。

尝试的 Coflow 命令：

- `cargo run -- data patch examples/workflow --patch '<delete Stage.stage_forest_01 的 JSON 字符串>' --human`
- `cargo run -- check examples/workflow`
- `cargo run -- data write-file examples/workflow --file data/progression/main.cfd --stdin --check --human`
- `cargo run -- data patch examples/workflow --patch '<delete Quest.quest_first_potion 的 JSON 字符串>' --human`
- `cargo run -- data write-file examples/workflow --file data/progression/quests.cfd --stdin --check --human`

结果：

- 删除 `stage_forest_01`：patch 写入成功但 `check_ok=false`，诊断覆盖 `main.cfd` 中的 `unlock_stage` 和 `quests.cfd` 中的 `Quest.stage`。
- 删除 `quest_first_potion`：诊断覆盖 `quest_first_blade.prerequisites`。
- 恢复后 check 通过。

## 第 31 轮：本地化迁移和维度异常

日期：2026-07-10

改动：

- 将无引用的 `Item.extra_ring` rename 为 `extra_ring_renamed`，build 后再 rename 回来。
- 临时移除 `Quest.title` 的 `@localized`，build 后恢复。
- 在 `Item_name.csv` 追加重复 `potion_small` 维度行。

结果：

- `rename_record` 对无引用记录可同步维度行：`Item_name.csv` 中出现 `extra_ring_renamed`，`ItemId.cs` 也生成 `extra_ring_renamed`。
- 移除 `Quest.title @localized` 后 build 通过，但旧 `data/dimensions/language/Quest_title.csv` 仍存在。
- 维度 CSV 追加重复 id 后 check/build 均通过，未报重复维度行诊断。

待修复问题：

- 删除 localized 字段后旧维度 CSV 未清理，存在 stale source 文件。
- 维度 CSV 重复 id 未报错，需要确认是后写覆盖、首行优先还是遗漏诊断。

## 第 32 轮：导出格式和 codegen 兼容性

日期：2026-07-10

改动：

- 临时把 `outputs.data.type` 从 `json` 改为 `messagepack`。
- 运行 build 后恢复为 `json`。

结果：

- MessagePack build 失败在 C# codegen preflight：`C# read-only immediate reference loading does not support cyclic table references: Skill -> Skill`。
- 旧 JSON 产物未被 messagepack 失败构建覆盖。
- 恢复 JSON 后 build 通过。

观察：

- 同一项目 JSON + C# 可构建，但 MessagePack + C# 因 `Skill.follow_up: &Skill?` 自引用失败。导出格式切换需要纳入项目级兼容性检查。

## 第 33 轮：大数据和命令行长度

日期：2026-07-10

改动：

- 尝试用一条直接 `--patch` 字符串插入 200 条 `Item`。
- 改为插入 80 条 `stress_item_001` 到 `stress_item_080`。
- build 后再用一条直接 `--patch` 字符串删除这 80 条。

结果：

- 200 条 patch 在 Windows 下无法启动 cargo：`文件名或扩展名太长`。
- 80 条 insert 通过，`applied=80`、`failed=0`，耗时约 10 秒。
- 80 条 delete 通过，`applied=80`、`failed=0`，耗时约 9 秒。
- build 通过。

待修复问题：

- 直接 `--patch` 字符串不适合更大规模批处理；在禁止 patch 文件的前提下，需要 stdin patch 或专用批量/迁移命令。

## 第 34 轮：协作冲突和半迁移状态

日期：2026-07-10

改动：

- 用后台 job 独占打开 `data/workflow.xlsx`，同时执行 Coflow patch。
- 临时给 `Monster` schema 增加必填字段 `threat_rating`，但不同步 Excel 表头。

结果：

- 文件被占用时 patch 失败，包含 `EXCEL-OPEN failed to open workbook ... os error 32`。
- 同一次 patch 输出还包含 `MUTATION-PATH record Item.extra_charm was not found`，错误聚合不够干净。
- 释放文件锁后 check 通过。
- 半迁移状态：`schema write-file --check` 通过，因为它只验证 schema；随后 `check` 报 `EXCEL-COLUMN sheet for type Monster is missing column for field threat_rating`。
- 恢复 schema 后 check 通过。

待修复问题：

- 文件锁场景应优先报告 source open failure，避免混入误导性的 record not found。
- schema 写入后的 `--check` 命名容易让人误以为做了项目级 check；迁移流程应明确再跑 `coflow check`。

## 最终验证（三）

日期：2026-07-10

命令：

- `cargo run -- data sources examples/workflow`
- `cargo run -- check examples/workflow`
- `cargo run -- build examples/workflow`
- `cargo check --workspace`
- `cargo test --workspace`

结果：

- data sources：通过，无 diagnostics。
- check：通过。
- build：通过，JSON 和 C# 产物重新生成。
- cargo check：通过。
- cargo test：通过。
- 临时 stress 记录已从数据源删除；`coflow.enum.lock.json` 保留 stress key 编号，符合前面观察到的 enum lock 历史保留行为。
