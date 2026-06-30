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
    app_id: cli_xxx
    app_secret: xxx
```

每个 source 必须且只能设置 `path` 或 `url` 之一。

| 字段 | 说明 |
| --- | --- |
| `path` | 本地文件或目录路径 |
| `url` | 远端数据源地址，例如飞书/Lark 表格 |
| `type` | 可选 Provider id；省略时由 registry 探测 |
| 其他字段 | 作为 Provider options 传给对应 loader |

飞书/Lark source 读取远端表格时需要 `app_id` 和 `app_secret`，用于获取 tenant access token。

## 内置 Provider

| Provider id | 数据源 | 常见文件/地址 | 支持写回 |
| --- | --- | --- | --- |
| `excel` | Excel workbook | `.xlsx` / `.xlsm` / `.xls` | 是 |
| `csv` | CSV 文件 | `.csv` | 是 |
| `cfd` | Coflow 文本数据 | `.cfd` | 是 |
| `lark-sheet` | 飞书/Lark 电子表格 | `lark:<spreadsheet_token>` 或表格 URL | 是 |

本地目录 source 会递归发现支持的文件。单文件或 URL source 可以省略 `type`，由 Provider registry 探测；如果多个 Provider 同等匹配，应显式设置 `type`。

## 加载流程

数据源加载分为几步：

1. `coflow-project` 读取 `coflow.yaml` 并解析路径。
2. `coflow-engine` 根据 source 的 `type`、`path` 或 `url` 选择 Provider。
3. Provider 执行 resolve / preflight / load。
4. Loader 输出统一的 input records。
5. DataModel 合并所有来源的记录，解析引用，执行 CFT `check {}`。

Provider 不决定业务合法性。字段是否存在、类型是否匹配、引用是否能解析、check 是否通过，都由 CFT schema 和 DataModel 统一判断。

## 相关页面

| 页面 | 内容 |
| --- | --- |
| [表格 Source](./02-table.md) | Excel、CSV、Lark 共享的表头、sheet、key、`@expand` 和单元格规则 |
| [Excel Source](./03-excel.md) | Excel workbook 读取、单元格转换和合并表头规则 |
| [CSV Source](./04-csv.md) | CSV 文件加载和写回边界 |
| [飞书/Lark Source](./05-lark.md) | 远端表格 source、URL、sheet 和 writer 能力 |
| [单元格值语法](../03-language/03-cell-value.md) | 表格单元格中的标量、对象、数组、字典、引用和多态对象语法 |
| [Provider API](./06-provider-api.md) | loader、writer、exporter、codegen 的公共接口边界 |
| [CFD 语法参考](../03-language/02-cfd.md) | `.cfd` 文本数据语法 |

## 引用关系

所有 source 最终合并进同一个 DataModel，因此不同来源之间可以互相引用：

- Excel 行可以引用 CFD record。
- CFD record 可以引用 CSV 行。
- 飞书/Lark 表格可以引用本地文件里的 record。

引用是否合法不由单个 Provider 决定，而是在 DataModel 阶段统一检查。

## 常见错误

| 问题 | 原因 | 处理 |
| --- | --- | --- |
| source 同时写了 `path` 和 `url` | source 入口不明确 | 只保留一个 |
| 目录里文件未加载 | 没有 Provider 支持该扩展名 | 使用 `.xlsx`、`.csv`、`.cfd` 或显式远端 Provider |
| sheet 找不到 type | sheet 名或 `type` 不在 CFT 中 | 修正 sheet/type 映射 |
| 缺少 key 列 | 表格没有 `id` / `Id` / `ID`，也未配置 `key` | 增加 key 列或配置 `key` |
| 单元格解析失败 | 单元格内容不符合目标字段类型 | 参考单元格值语法修正 |
| CFD record 引用表格记录失败 | 目标 key 不存在或类型不兼容 | 检查目标 source 是否加载、key 是否一致 |
