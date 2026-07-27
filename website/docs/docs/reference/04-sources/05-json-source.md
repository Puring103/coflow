# JSON 数据源

JSON source provider id 为 `json`。每个文件表示一个表，根节点必须是记录数组；默认用文件名
推断 CFT 类型，也可以用 `record_type` 指定类型。首版 provider 为只读。

记录字段沿用 JSON 导出契约：`id`、多态对象的 `$type`、字符串记录引用、数组、对象和字典。
枚举推荐使用符号字符串，例如 `"Rarity.Rare"`；flags 组合值使用 `"Permissions(5)"`。
导入器仍接受旧整数枚举以便迁移。重复属性和未知属性会作为错误报告。

```yaml
sources:
  - path: data/Item.json
    type: json
```

