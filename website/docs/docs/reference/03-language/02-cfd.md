# CFD 文本

CFD 是项目唯一的数据输入格式。文件由顶层记录、记录 key 和 schema-guided 字段组成：

```cfd
sword: Item {
  name: "Fire Sword",
  rarity: Rare,
}
```

CFD 使用空格缩进，每一级固定为 2 个空格，不使用制表符。编辑器格式化、`Tab` 键以及结构化编辑器写回的文本遵循这一约定。

支持 scalar、字符串、enum、`None`、数组、字典、内联对象和跨文件引用。解析器先生成带 span 的语法树，runtime 再使用 CFT schema 完成类型转换、默认值、引用和维度 overlay。

`bool` 只接受小写 `true` 和 `false`。编辑器复制单个值时写入该值的 CFD 文本；复制、剪切或粘贴矩形区域时使用 CFD 二维数组，例如 `[["sword", 10], ["shield", 20]]`。

目录只发现 `.cfd`；文件顺序和 source identity 稳定排序。未知字段、重复 key、类型不匹配和悬空引用都会在 `check` 中报告。
