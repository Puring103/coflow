# Local Coflow 工作流测试计划

本文档描述 `examples/workflow` 项目的迭代式本地工作流测试。目标是模拟一个真实配置项目从小到大演进的过程，验证本地 Excel 和 CFD 数据是否可以主要通过 Coflow 维护，而不是在文档外静默准备数据文件。

## 目标

- 以 `coflow.yaml` 作为唯一项目入口。
- 优先使用 Coflow 完成 schema 查看、source 查看、本地建表、表头维护、数据写回、检查、构建、导出和代码生成。
- 覆盖真实 schema 演进：逐步新增类型、修改已有字段、增加跨表引用、增加业务检查、引入 CFD 复杂结构、启用本地化。
- 覆盖本地维护操作：同步表头、批量增加数据、按照条件删除记录、删除整张表并恢复。
- 当 Coflow 无法完成某一步时，明确记录工作流缺口。
- 不使用 patch 文件；所有 `data patch` 都使用命令行直接传入 patch 字符串。

## Fallback 规则

默认规则：先尝试 Coflow。

如果某一步因为 Coflow 能力缺失或行为错误而被阻塞，允许使用 PowerShell、Node、Excel 工具或人工操作作为 fallback 来继续下一轮迭代。每次 fallback 都必须记录：

- 尝试过的 Coflow 命令。
- 具体失败信息或能力限制。
- 使用的 fallback 命令或人工操作。
- 影响到的本地 source、sheet、record。
- 该 fallback 属于产品缺陷、文档缺口、预期不支持边界，还是测试数据问题。

不要在文档外静默准备 Excel 或 CFD 数据。fallback 本身就是测试结果的一部分。

## 命令约定

在仓库根目录运行以下命令，使用源码仓库内的 CLI：

```powershell
cargo run -- schema inspect examples/workflow
cargo run -- data sources examples/workflow
cargo run -- check examples/workflow
cargo run -- build examples/workflow
```

本地建表、表头同步和数据写回：

```powershell
cargo run -- data create-file examples/workflow --file data/workflow.xlsx --provider excel --type <Type> --sheet <Sheet>
cargo run -- data sync-header examples/workflow --file data/workflow.xlsx --provider excel --type <Type> --sheet <Sheet>
cargo run -- data create-file examples/workflow --file data/progression.cfd --provider cfd --type <Type>
cargo run -- data write-file examples/workflow --file <file> --stdin --check
cargo run -- data patch examples/workflow --patch '<使用当前 DTO 的 JSON 字符串，见 WORKFLOW_TEST_LOG.md>' --human
```

注意：本测试禁止创建 `patches/*.json` 作为 patch 输入。需要批量操作时也直接传入 JSON 字符串；如果命令行长度成为问题，记录为 CLI 能力缺口，再决定是否需要产品支持 stdin patch。

当前 CLI 的 patch 字符串使用 `{"ops":[...]}`，每个操作使用 `op` 字段；`set_field`、`rename_record`、`delete_record` 使用 `record: {"type": "...", "key": "..."}` 选择记录。实际可执行命令以 `WORKFLOW_TEST_LOG.md` 中记录的命令形态为准。

如果这些改动要作为普通开发提交，按仓库要求运行：

```powershell
cargo check --workspace
cargo test --workspace
```

## 项目形态

示例项目要足够小，便于定位问题；同时要覆盖本地 Excel、CFD、本地化和 writer 核心能力。

推荐本地 source：

| Source | CFT 类型 | 验证目的 |
| --- | --- | --- |
| `data/workflow.xlsx` / `Item` | `Item` | 基础标量字段、enum、flags、数组、默认值、批量插入 |
| `data/workflow.xlsx` / `Skill` | `Skill` | enum、flags、可空自引用、坏引用诊断 |
| `data/workflow.xlsx` / `Monster` | `Monster` | 跨 sheet 引用、引用数组、条件删除、整表删除恢复 |
| `data/workflow.xlsx` / `Text` | `Text` | 本地化基础文本 |
| `data/progression.cfd` | `DropTable`、`Stage`、`Quest` | 复杂嵌套结构、Excel/CFD 双向引用、整文件写入 |
| `data/dimensions/language/*` | `*Variants` | `@localized` 维度生成和变体保留 |

在 `coflow.yaml` 中使用显式 `sheets` 映射，避免表头变更和中文/英文列名造成歧义：

```yaml
schema: schema/

sources:
  - path: data/workflow.xlsx
    type: excel
    sheets:
      - sheet: Item
        type: Item
        columns:
          Item ID: id
          Name: name
          Rarity: rarity
          Price: price
  - path: data/progression.cfd

dimensions:
  language:
    variants: [zh, en]
    out_dir: data/dimensions/language
```

## 第 0 轮：项目基线

目的：确认空项目可以通过 Coflow 建立本地配置入口，并明确哪些初始化动作必须由 Coflow 完成。

改动：

- 创建初始 `coflow.yaml`，配置 schema 目录、Excel source、CFD source、JSON/C# outputs。
- 创建初始 `schema/main.cft`，只包含第 1 轮需要的 `Rarity` 和 `Item`。
- 不手工创建 Excel 表头和数据；优先通过 Coflow 建表。

Coflow 命令：

```powershell
cargo run -- schema inspect examples/workflow
cargo run -- data sources examples/workflow
cargo run -- check examples/workflow
```

需要记录：

- 缺失 `data/workflow.xlsx` 时诊断是否清楚。
- 缺失 CFD source 时诊断是否清楚。
- 空 `sheets` 或缺失 sheet 的行为是否符合预期。
- `data create-file` 是否可以创建新 workbook 和指定 sheet。

## 第 1 轮：新增 `Item`

目的：验证最小可用的本地 Excel 工作流。

Schema：

- 新增 `enum Rarity`。
- 新增 `@flag enum ItemTag`。
- 新增 `type Item`，包含 `name`、`rarity`、`price`、`tags`。
- 增加 id 格式和非负价格检查。

Excel sheet：

- `Item` 映射到 `Item`。
- key 列：`Item ID`。

Coflow 流程：

```powershell
cargo run -- data create-file examples/workflow --file data/workflow.xlsx --provider excel --type Item --sheet Item --human
cargo run -- data patch examples/workflow --patch '<使用当前 DTO 的 JSON 字符串，见 WORKFLOW_TEST_LOG.md>' --human
cargo run -- check examples/workflow
cargo run -- build examples/workflow
```

需要记录：

- Coflow 是否能创建本地 Excel workbook、sheet 和表头。
- `insert_record` 是否能写入 Excel。
- enum、flags 和数组/flag 单元格语法是否能正确解析。
- 只有检查通过后，JSON 导出和 C# 代码生成是否才会写入。

## 第 2 轮：修改 `Item` 并同步表头

目的：模拟初始数据已经存在后的常规字段演进。

Schema 改动：

- 新增 `level_required: int = 1`。
- 新增 `sellable: bool = true`。
- 新增 `@localized description: string = ""`，同时启用 `dimensions.language`。

Coflow 流程：

```powershell
cargo run -- schema inspect examples/workflow
cargo run -- data sync-header examples/workflow --file data/workflow.xlsx --provider excel --type Item --sheet Item --human
cargo run -- data patch examples/workflow --patch '<使用当前 DTO 的 JSON 字符串，见 WORKFLOW_TEST_LOG.md>' --human
cargo run -- check examples/workflow
cargo run -- build examples/workflow
```

需要记录：

- `sync-header` 是否能安全追加新列。
- 旧列是保留、忽略，还是造成混淆。
- 默认值是否避免不必要的批量写回。
- build 是否生成 `data/dimensions/language/*`。

## 第 3 轮：批量增加 `Item`

目的：验证一次 patch 批量写入本地 Excel 的可用性和报告质量。

Patch 操作：

- 一次插入 10 到 50 条 `Item`。
- 至少包含不同 `Rarity`、`ItemTag`、价格和等级需求。

Coflow 流程：

```powershell
cargo run -- data patch examples/workflow --patch '<使用当前 DTO 的 JSON 字符串，见 WORKFLOW_TEST_LOG.md>' --human
cargo run -- data list examples/workflow --type Item --human
cargo run -- check examples/workflow
```

需要记录：

- 大 patch 的命令行长度是否可接受。
- `applied` / `failed` 统计是否准确。
- 失败时是否清楚标明已写入和未写入的操作。

## 第 4 轮：新增 `Skill`

目的：验证 enum、flags、可空引用和自引用。

Schema：

- 新增 `enum Element`。
- 新增 `@flag enum SkillTag`。
- 新增 `type Skill`，包含 `name`、`element`、`tags`、`power`、`follow_up: &Skill? = null`。

Excel sheet：

- `Skill` 映射到 `Skill`。
- key 列：`Skill ID`。

Coflow 流程：

```powershell
cargo run -- data create-file examples/workflow --file data/workflow.xlsx --provider excel --type Skill --sheet Skill --human
cargo run -- data patch examples/workflow --patch '<使用当前 DTO 的 JSON 字符串，见 WORKFLOW_TEST_LOG.md>' --human
cargo run -- check examples/workflow
```

诊断测试：

```powershell
cargo run -- data patch examples/workflow --patch '<使用当前 DTO 的 JSON 字符串，见 WORKFLOW_TEST_LOG.md>' --human
cargo run -- check examples/workflow
cargo run -- data patch examples/workflow --patch '<使用当前 DTO 的 JSON 字符串，见 WORKFLOW_TEST_LOG.md>' --human
```

需要记录：

- Excel 单元格中的 flag 值语法是否可用。
- 自引用行为是否正确。
- 诊断是否包含 sheet、cell、record、field path。

## 第 5 轮：新增 `Monster`

目的：验证跨 sheet 引用、引用数组和业务检查。

Schema：

- 新增 `type Monster`，包含 `name`、`level`、`hp`、`power`、`skill: &Skill`、`drops: [&Item] = []`、`enabled: bool = true`。
- 增加 level、hp、power 必须为正的检查。

Excel sheet：

- `Monster` 映射到 `Monster`。
- key 列：`Monster ID`。

Coflow 流程：

```powershell
cargo run -- data create-file examples/workflow --file data/workflow.xlsx --provider excel --type Monster --sheet Monster --human
cargo run -- data patch examples/workflow --patch '<使用当前 DTO 的 JSON 字符串，见 WORKFLOW_TEST_LOG.md>' --human
cargo run -- check examples/workflow
cargo run -- build examples/workflow
```

诊断测试：

- 写入一条 `hp = 0` 或缺少 `skill` 的怪物。
- 验证存在诊断时 `build` 不会写产物。

需要记录：

- 跨 sheet 引用解析是否正确。
- 引用数组的单元格语法是否可用。
- check 失败行为和产物安全是否符合预期。

## 第 6 轮：按照条件删除记录

目的：验证“查询、筛选、生成删除 patch、写回”的本地工作流。

条件：

- 删除 `Monster.enabled == false` 的记录，例如 `event_dummy`。

Coflow 优先流程：

```powershell
cargo run -- data list examples/workflow --type Monster --human
cargo run -- data patch examples/workflow --patch '<使用当前 DTO 的 JSON 字符串，见 WORKFLOW_TEST_LOG.md>' --human
cargo run -- data list examples/workflow --type Monster --human
cargo run -- check examples/workflow
```

需要记录：

- Coflow 是否有原生条件删除能力。
- 如果没有，`data list` 输出是否足够让 agent 可靠筛选并生成 delete patch 字符串。
- 删除被引用记录时是否产生清晰引用诊断。

## 第 7 轮：删除整张表并恢复

目的：验证本地 Excel sheet 被删除后的诊断、恢复和数据重建流程。

操作：

- 删除 `Monster` sheet。
- 先运行 Coflow 检查诊断。
- 再通过 Coflow 重建 sheet 和表头。
- 用 `--patch` 字符串批量恢复必要记录。

Coflow 流程：

```powershell
cargo run -- data sources examples/workflow
cargo run -- check examples/workflow
cargo run -- data sync-header examples/workflow --file data/workflow.xlsx --provider excel --type Monster --sheet Monster --human
cargo run -- data patch examples/workflow --patch '<使用当前 DTO 的 JSON 字符串，见 WORKFLOW_TEST_LOG.md>' --human
cargo run -- check examples/workflow
```

Fallback 只允许在 Coflow 不能删除 sheet 或测试需要制造缺失 sheet 状态时使用，并必须记录。

需要记录：

- 缺失 sheet 的诊断质量。
- `sync-header` 是否能重建缺失 sheet。
- 重建 sheet 后 patch 写入是否稳定。

## 第 8 轮：新增 CFD 复杂结构

目的：验证 Excel 和 CFD 混合 source 的真实边界。

Schema：

- 新增 `abstract type Reward`。
- 新增 `ItemReward`、`CurrencyReward`。
- 新增 `DropTable`、`Stage`、`Quest`。

CFD source：

- `data/progression.cfd`。
- `DropTable`、`Stage`、`Quest` 引用 Excel 中的 `Item`、`Skill`、`Monster`。

Coflow 流程：

```powershell
cargo run -- data create-file examples/workflow --file data/progression.cfd --provider cfd --type Stage --human
@'
DropTable {
  drop_slime_green {
    monster: &slime_green,
    rewards: [
      ItemReward { key: reward_slime_potion, item: &potion_small, count: 1 },
      CurrencyReward { key: reward_slime_gold, currency: gold, amount: 8 },
    ],
    weights: [70, 30],
  }
}
'@ | cargo run -- data write-file examples/workflow --file data/progression.cfd --stdin --check --human
cargo run -- check examples/workflow
cargo run -- build examples/workflow
```

需要记录：

- 复杂嵌套结构是否适合 `data write-file` 而不是 `data patch`。
- CFD 引用 Excel record 的诊断是否清晰。
- Excel/CFD 混合 source 的加载顺序是否符合预期。

## 第 9 轮：本地化

目的：验证 `@localized` 字段、维度 source 生成、变体保留和 C# 输出。

Schema：

- 在 `Item.name`、`Item.description`、`Skill.name`、`Monster.name` 或 `Text.value` 上使用 `@localized`。
- 在 `coflow.yaml` 中配置 `dimensions.language.variants` 和 `out_dir`。

Coflow 流程：

```powershell
cargo run -- check examples/workflow
cargo run -- build examples/workflow
cargo run -- data sources examples/workflow
```

变体编辑优先流程：

```powershell
cargo run -- data sync-header examples/workflow --file data/dimensions/language/Item_name.csv --provider csv --type Item_nameVariants --human
cargo run -- data patch examples/workflow --patch '<使用当前 DTO 的 JSON 字符串，见 WORKFLOW_TEST_LOG.md>' --human
cargo run -- check examples/workflow
cargo run -- build examples/workflow
```

需要记录：

- build 是否生成维度文件。
- 已存在 variant 列是否保留。
- 修改本地化变体是否能通过 Coflow writer 完成。
- C# 是否生成 `Localized<T>` 字段。

## 第 10 轮：类型重构

目的：测试项目后期 schema 重构，并明确 Excel 和 CFD 的边界。

选择一种重构：

- 把 `Stage.first_clear_reward` 从简单 `&Item` 改成多态 `Reward`。
- 或把 `Monster.drops` 从 Excel 列迁移到 CFD 中的 `DropTable`。

Coflow 流程：

```powershell
cargo run -- schema inspect examples/workflow
cargo run -- data sync-header examples/workflow --file data/workflow.xlsx --provider excel --type Monster --sheet Monster --human
cargo run -- data write-file examples/workflow --file data/progression.cfd --stdin --check --human
cargo run -- check examples/workflow
```

需要记录：

- 表格单元格是否能清楚表达新数据形态。
- 哪些复杂嵌套数据应该迁移到 CFD。
- writer 对不支持结构的诊断是否清晰。

## 每轮日志模板

每轮结束后追加日志：

```text
轮次：
改动：
日期：

尝试的 Coflow 命令：
-

结果：
- schema inspect：
- data sources：
- data create-file：
- data sync-header：
- data write-file：
- data patch：
- check：
- build：

Fallback：
- Coflow 命令：
  失败原因：
  Fallback 命令/操作：
  影响的 source/sheet/record：
  分类：产品缺陷 | 文档缺口 | 不支持边界 | 测试数据问题

观察到的诊断：
- code：
  stage：
  file/source：
  sheet：
  cell：
  record：
  message：

待修复问题：
-

下一轮：
-
```

## 预期发现的问题

这套流程预计会暴露以下方面的问题：

- 本地 Excel workbook 创建和追加 sheet。
- 本地 Excel 表头同步、缺失 sheet 恢复和旧列处理。
- `--patch` 字符串形式在批量写入时的可用性。
- enum、flags、数组、可空引用、引用数组的单元格语法。
- 按条件删除记录是否需要原生命令支持。
- 删除整张表后的诊断和恢复流程。
- CFD 整文件写入和复杂嵌套数据的维护边界。
- 带 sheet/cell 或 CFD span 来源的 check 诊断。
- 存在诊断时的产物安全。
- 本地化维度文件生成、变体保留和 writer 支持。

## 后续真实迭代覆盖项

第 21 轮到第 26 轮继续模拟项目进入中后期后的维护场景：

- 给已有 `Item` 增加 `@idAsEnum(ItemId)`，验证 enum lock、JSON 导出和 C# 强类型引用。
- 批量插入和批量删除 40 条临时记录，验证命令行 patch 长度、写入统计、维度清理和 enum lock 行为。
- 在 CFD 目录 source 中制造跨文件重复 key，验证诊断定位和 `data write-file --stdin --check` 恢复。
- 使用错误 `file` guard 写 Excel 记录，验证 writer 不误写其他 source。
- 临时破坏 `coflow.yaml` 的 Excel sheet 映射，验证配置/source 诊断质量。
- 使用 `schema write-file --dry-run --check` 提交无效 schema，验证 dry-run 不落盘。
- 插入 C# 关键字 record key，验证 `check` 与 codegen preflight 的边界。

第 26 轮到第 27 轮继续覆盖数据迁移和 Excel 结构异常：

- 将 `Monster.drops` 从 Excel 列迁移到 CFD `DropTable`，验证 schema、source 映射、表头同步和产物变化。
- 明确记录未来需要增加记录/字段迁移命令，例如把某类型字段批量搬迁到另一个类型或 source。
- 制造 Excel 多余列、列顺序变化和重复列，验证诊断、`sync-header` 恢复和数据保留风险。

未来建议新增 Coflow 命令：

```powershell
coflow data migrate-field <project> --from Monster.drops --to DropTable.rewards --plan --apply
coflow data migrate-records <project> --from data/workflow.xlsx --to data/progression --type <Type> --plan --apply
```

这类命令应该先输出迁移计划，列出受影响的 source/sheet/record/field，再在 apply 阶段用 provider writer 完成搬迁，并在成功后可选择同步表头删除旧字段。

第 28 轮到第 34 轮覆盖真实项目后期常见事故：

- Excel 人工破坏：重复 key、空 key、公式单元格、合并单元格。
- CFD 人工破坏：截断文件、漏写 `&`、同文件重复 key、spread 互相引用。
- 引用图级联：删除被 `Stage` / `Quest` 引用的记录，确认跨文件诊断。
- 本地化迁移：record rename 对维度行的影响、移除 localized 后旧 CSV 是否清理、维度 CSV 重复 id。
- 导出格式：JSON 切 MessagePack 再切回，验证 codegen 兼容性和失败产物安全。
- 大数据：直接字符串 patch 的 Windows 命令行长度上限，以及可通过的批量规模。
- 协作冲突：Excel 文件被外部进程占用、schema 已改但表头未同步的半迁移状态。
