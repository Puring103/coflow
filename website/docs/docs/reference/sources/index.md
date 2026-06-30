# 数据源与 Provider

Coflow 通过 Provider 读取和写回不同来源的数据。CFT 负责定义结构，Provider 负责把 Excel、CSV、CFD、飞书/Lark 表格等来源转换成统一的输入记录，再交给 DataModel 做默认值、引用解析和业务校验。

## Source 配置

数据源写在 `coflow.yaml` 的 `sources` 中：

```yaml
sources:
  - path: data

  - path: data/items.xlsx
    sheets:
      - sheet: Item
        type: Item
        key: Item ID
        columns:
          Name: name

  - url: lark:sht_xxxxx
    type: lark-sheet
```

每个 source 必须且只能设置 `path` 或 `url` 之一。

| 字段 | 说明 |
| --- | --- |
| `path` | 本地文件或目录路径 |
| `url` | 远端数据源地址，例如飞书/Lark 表格 |
| `type` | 可选 Provider id；省略时由 registry 探测 |
| 其他字段 | 作为 Provider options 传给对应 loader |

本地目录 source 会递归发现支持的文件。当前内置 Provider 覆盖：

| Provider id | 数据源 | 常见文件/地址 | 支持写回 |
| --- | --- | --- | --- |
| `excel` | Excel workbook | `.xlsx` / `.xlsm` / `.xls` | 是 |
| `csv` | CSV 文件 | `.csv` | 是 |
| `cfd` | Coflow 文本数据 | `.cfd` | 是 |
| `lark-sheet` | 飞书/Lark 电子表格 | `lark:<spreadsheet_token>` 或表格 URL | 是 |

## Provider 边界

Provider 是 Coflow 接入外部数据和产物格式的扩展点。当前公开 Provider 分为四类：

| Provider 类型 | Trait | 职责 |
| --- | --- | --- |
| Loader | `DataLoader` | resolve / preflight / load source，输出 input records |
| Writer | `DataWriter` | 根据 record origin 写回字段、插入、删除或重命名记录 |
| Exporter | `DataExporter` | 把验证后的 DataModel 导出为文件集合 |
| Codegen | `CodeGenerator` | 根据 schema 或 model 生成运行时代码 |

Provider 通过 `ProviderRegistry` 注册。默认 registry 由 `coflow-builtins` 组装，CLI 和编辑器使用它获得内置 provider。

Provider 不负责发现 `coflow.yaml`，不持有项目运行时状态，也不直接替换导出目录。项目生命周期由 `coflow-project` 和 `coflow-engine` 编排；产物落盘由 CLI 宿主负责。

## 加载流程

数据源加载分为几步：

1. `coflow-project` 读取 `coflow.yaml` 并解析路径。
2. `coflow-engine` 根据 source 的 `type`、`path` 或 `url` 选择 Provider。
3. Provider 执行 resolve / preflight / load。
4. Loader 输出统一的 input records。
5. DataModel 合并所有来源的记录，解析引用，执行 CFT `check {}`。

Provider 不决定业务合法性。字段是否存在、类型是否匹配、引用是否能解析、check 是否通过，都由 CFT schema 和 DataModel 统一判断。

## Source Resolve

source resolve 会把配置项展开为具体 Provider source。

| 配置形态 | 行为 |
| --- | --- |
| `path` 指向文件，且未写 `type` | 通过扩展名和 Provider probe 选择 loader |
| `path` 指向目录，且未写 `type` | 目录交给各 loader resolve，发现可处理文件 |
| `url` 未写 `type` | 通过 URI scheme 或 Provider probe 选择 loader |
| 写了 `type` | 直接使用指定 provider |

如果多个 Provider 同等匹配，配置应显式写 `type`，避免来源解释不确定。

## 表格 Source

Excel、CSV 和飞书/Lark 表格共享表格加载语义。

第一行是表头，后续行是数据。每一行对应一条顶层 record。

默认规则：

- sheet 名映射到 CFT type。
- `id`、`Id` 或 `ID` 列作为 record key。
- 表头文本映射到同名 CFT 字段。
- 单元格内容按目标字段类型使用 [单元格值语法](./cell-value.md) 解析。

表格 source 的第一行必须是表头。空数据行会被跳过。某个 sheet 的表头无法可靠映射时，该 sheet 的数据行会被跳过，但其他 sheet 和其他 source 仍会继续收集诊断。

### `sheets`

`sheets` 用来显式配置 sheet 到 type、key 列和字段名的映射：

```yaml
sources:
  - path: data/config.xlsx
    sheets:
      - sheet: Items
        type: Item
        key: Item ID
        columns:
          Display Name: name
          Price: price
```

| 字段 | 说明 |
| --- | --- |
| `sheet` | Excel worksheet 或飞书 sheet 名 |
| `type` | CFT type 名；省略时使用 sheet 名 |
| `key` | record key 表头列名；省略时使用 `id` / `Id` / `ID` |
| `columns` | 表头文本到 CFT 字段名的映射 |

未列入 `columns` 的表头仍会按原文本匹配字段。

显式配置 `key` 时，按配置值精确匹配表头；未配置时按 `id`、`Id`、`ID` 依次查找。key 列不映射到 CFT 字段。

`columns` 只做表头重命名，不限制未列出的字段。Coflow 会拒绝重复的源表头 key，避免 YAML map 后写覆盖导致配置被静默丢弃。

### `#` 控制列

表格可以包含名为 `#` 的控制列。数据行中该列单元格去掉首尾空白后等于 `##` 时，整行跳过。

这个控制列不映射到 CFT 字段，也不参与未知字段检查。

### `@expand`

CFT 字段标记 `@expand` 后，表格中可以把嵌套对象展开到相邻列。父字段列承载第一个子字段，后续相邻列必须连续且表头为空。

适合在 Excel 中维护结构较浅、但希望策划按列编辑的对象字段。复杂数组、字典和多态对象通常更适合 CFD。

## Excel Source

Excel loader 负责读取 `.xlsx`、`.xlsm`、`.xls` workbook，并把单元格转换成共享表格模型。

Excel 原生单元格会先转成文本，再交给 schema-guided cell parser：

| Excel 单元格 | 转换 |
| --- | --- |
| 文本 | 原文本 |
| 整数 | 十进制整数文本 |
| 浮点 | 十进制浮点文本，整数值会去掉 `.0` |
| 布尔 | `true` / `false` |
| error | 报 `EXCEL-CELL` |
| date/time | 报 `EXCEL-CELL` |
| duration | 报 `EXCEL-CELL` |

如果日期、时间或持续时间需要进入 Coflow，应在 Excel 中保存为普通文本，并在 CFT 中用合适的字段类型表达。

Excel 合并表头只有左上角单元格保留文本，后续单元格通常表现为空表头。这和 `@expand` 的相邻空表头规则兼容。

## CSV Source

CSV source 使用共享表格语义。单个 `.csv` 文件通常对应一个表格：

- 未配置 `sheets` 时，type 可由文件名或配置推断。
- 配置 `sheets` 时，使用 `sheet` / `type` / `key` / `columns` 的同一套结构。
- CSV writer 支持通过 `data patch`、`data create-file` 和 `data sync-header` 修改本地文件。

CSV 不支持 Excel workbook 的多 sheet 结构，但仍使用相同的表头、key、控制列、`@expand` 和单元格值规则。

## CFD Source

CFD 文件不使用表头，由文本中的记录声明决定 type 和 key：

```text
sword_fire: Item {
  name: "Fire Sword",
  price: 100,
}
```

单个 `.cfd` 文件 source 不配置 `sheets`。目录 source 中可以同时包含 Excel、CSV 和 CFD 文件，CFD 文件会按文本记录类型加载。

CFD 适合：

- 嵌套对象。
- 数组和字典。
- 多态对象。
- 路径引用。
- spread 覆盖。

详细语法见 [CFD 语法参考](../cfd.md)。

## 飞书/Lark Source

飞书/Lark 表格使用 `lark-sheet` Provider：

```yaml
sources:
  - url: lark:sht_xxxxx
    type: lark-sheet
    sheets:
      - sheet: Item
        type: Item
```

它与 Excel 共享表格语义：sheet、type、key、columns 和单元格值解析规则一致。

Lark writer 面向远端表格写回。它可以编辑字段和 key，但插入、删除等能力受远端表格结构和 writer 能力限制。自动化命令会根据 writer 报告的能力和诊断返回结果。

## 引用关系

所有 source 最终合并进同一个 DataModel，因此不同来源之间可以互相引用：

- Excel 行可以引用 CFD record。
- CFD record 可以引用 CSV 行。
- 飞书/Lark 表格可以引用本地文件里的 record。

引用是否合法不由单个 Provider 决定，而是在 DataModel 阶段统一检查。

## Writer 能力

部分 CLI、编辑器和 AI agent 命令会通过 Provider writer 修改数据：

- `data create-file` 创建本地数据文件。
- `data sync-header` 同步 CSV / Excel 表头或 CFD 顶层字段。
- `data write-file` 整文件写入本地 CFD。
- `data patch` 通过 writer 应用批量修改。

Writer 会遵守 Provider 的文件边界和 schema 约束，不会绕过数据源直接改 DataModel。

Writer 的能力由 provider 描述，常见能力包括：

| 能力 | 说明 |
| --- | --- |
| `can_edit_field` | 能修改字段值 |
| `can_edit_key` | 能修改 record key |
| `can_insert_record` | 能插入新记录 |
| `can_delete_record` | 能删除记录 |
| `requires_full_refresh_after_write` | 写后需要重新加载项目 |
| `is_remote` | 数据源是远端 source |

CLI 写入命令和编辑器都应通过 writer，而不是直接修改 DataModel。

## 常见错误

| 问题 | 原因 | 处理 |
| --- | --- | --- |
| source 同时写了 `path` 和 `url` | source 入口不明确 | 只保留一个 |
| 目录里文件未加载 | 没有 Provider 支持该扩展名 | 使用 `.xlsx`、`.csv`、`.cfd` 或显式远端 Provider |
| sheet 找不到 type | sheet 名或 `type` 不在 CFT 中 | 修正 sheet/type 映射 |
| 缺少 key 列 | 表格没有 `id` / `Id` / `ID`，也未配置 `key` | 增加 key 列或配置 `key` |
| 单元格解析失败 | 单元格内容不符合目标字段类型 | 参考单元格值语法修正 |
| CFD record 引用表格记录失败 | 目标 key 不存在或类型不兼容 | 检查目标 source 是否加载、key 是否一致 |
