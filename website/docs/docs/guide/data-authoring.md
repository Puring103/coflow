# 数据维护

## 编辑前先查询

先确认 source、record 和 schema 字段，避免靠文件名或列名猜测：

```powershell
coflow data sources .
coflow data list . --type Item
coflow schema inspect . --type Item
```

## 选择编辑方式

- 人工批量编辑 Excel 时，保留稳定 record key，不要把显示名当作引用。
- CSV 应保持单表职责，嵌套数组、字典和对象使用标准单元格值语法。
- CFD 适合直接审查嵌套和多态结构，也适合 Git 合并。

## 结构化写入

```powershell
coflow data sync-header . --file data/items.csv --type Item
coflow data write-file . --file data/items.cfd --check
coflow data patch . --patch '<json>'
coflow check .
```

`data patch` 会把一批操作作为一个整体写入。任一操作失败时不会保留部分修改。

## 常见任务

修改一条记录的字段：

```powershell
coflow data patch . --patch '{"ops":[{"op":"set_field","record":{"type":"Item","key":"sword"},"path":[{"kind":"field","value":"price"}],"value":125}]}'
```

schema 新增字段后同步 CSV 表头：

```powershell
coflow data sync-header . --file data/items.csv --type Item
coflow check .
```

大范围整理 CFD 文件时，先预览检查结果，再执行写入：

```powershell
Get-Content data/items.cfd | coflow data write-file . --file data/items.cfd --dry-run --check
Get-Content data/items.cfd | coflow data write-file . --file data/items.cfd --check
```

## 处理诊断

诊断会尽可能附带文件、sheet、行列、record 和字段路径。修复时先处理 schema/type 错误，再处理引用和业务 check，避免后续错误被前置问题放大。

完整命令和 patch 请求格式见 [CLI 命令参考](../reference/08-cli.md)。
