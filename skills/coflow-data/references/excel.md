# Excel Source

Excel source 使用 `excel` Provider 读取 `.xlsx`、`.xlsm`、`.xls` workbook，并把 sheet 转成共享表格模型。

Excel 与 CSV 共享 [表格 Source](./table-source.md) 规则：第一行表头、`id` 列作为 key、`sheets` 映射、`columns` 映射、`#` 控制列和 `@expand`。

## 配置示例

```yaml
sources:
  - path: data/config.xlsx
    type: excel
    sheets:
      - sheet: Items
        type: Item
        key: Item ID
        columns:
          Display Name: name
          Price: price
```

省略 `type` 时，Coflow 会通过文件扩展名探测 Excel Provider。

## 单元格转换

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

## 合并表头

Excel 合并表头只有左上角单元格保留文本，后续单元格通常表现为空表头。这和 `@expand` 的相邻空表头规则兼容。

如果 `@expand` 后续相邻列出现非空表头，Coflow 会报告 `EXCEL-COLUMN` 并跳过该 sheet 的数据行，避免普通业务列被静默当作展开字段。

## 写回

Excel writer 只写 `.xlsx`。`.xlsm` 和 `.xls` 可以作为 source 读取，但其动态
`WriterCapabilities` 为只读：当前 OOXML writer 无法保证保留 `.xlsm` 的 VBA
project，也不能原样写回二进制 `.xls`。这些格式的 field edit、record
mutation、create-file 和 sync-header 都会在读取或修改 workbook 之前报告
`EXCEL-FORMAT-READ-ONLY`，不会转换扩展名或覆盖原文件。

`.xlsx` 支持通过以下命令写本地 workbook：

```powershell
coflow data patch <project> --patch '<json>'
coflow data create-file <project> --file data/items.xlsx --type Item --provider excel --sheet Item
coflow data sync-header <project> --file data/items.xlsx --type Item --provider excel --sheet Item
```

普通写回失败使用 `EXCEL-WRITE` 诊断；不安全格式使用
`EXCEL-FORMAT-READ-ONLY`。读取和解析阶段的诊断见
[错误码](https://puring103.github.io/coflow/docs/reference/09-diagnostics/02-codes)。
