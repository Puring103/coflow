# TSV 数据源

TSV provider id 为 `tsv`，支持 `.tsv` 文件。除分隔符固定为制表符外，它与 CSV
使用相同的表头映射、单元格值语法、`sheets` 配置、事务写入、表管理和本地化维度管理。

字段可使用双引号包围；引号内允许制表符、换行和 CRLF，`""` 表示一个双引号。

```yaml
sources:
  - path: data/Item.tsv
    type: tsv
```

