# CSV Source

CSV source 使用 `csv` Provider 读取 `.csv` 文件，并套用共享 [表格 Source](./02-table.md) 规则。

CSV 适合维护单表、纯文本、易于 diff 的配置数据。它不支持 Excel workbook 的多 sheet 结构，但仍使用相同的表头、key、控制列、`@expand` 和单元格值规则。

## 配置示例

```yaml
sources:
  - path: data/items.csv
    type: csv
    sheets:
      - sheet: Item
        type: Item
```

省略 `type` 时，Coflow 会通过 `.csv` 扩展名探测 CSV Provider。

## 表格语义

单个 `.csv` 文件通常对应一个表格：

- 第一行是表头。
- 后续行是数据。
- `id`、`Id` 或 `ID` 列作为 record key。
- 单元格按目标字段类型解析。
- `#` 控制列可以跳过行。
- `@expand` 可以把嵌套对象展开到相邻列。

## 写回

CSV writer 支持通过以下命令写本地文件：

```powershell
coflow data patch <project> --patch patch.json
coflow data create-file <project> --file data/items.csv --type Item --provider csv
coflow data sync-header <project> --file data/items.csv --type Item
```

`data sync-header` 会重写表头，保留同名列数据，新增列填空，删除 schema 中不存在的列。

写回失败使用 `CSV-WRITE` 诊断。读取和解析阶段的诊断见 [错误码](../09-diagnostics/02-codes.md)。
