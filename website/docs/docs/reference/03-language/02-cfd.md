# CFD 文本

CFD 是项目唯一的数据输入格式。文件由顶层记录、记录 key 和 schema-guided 字段组成：

```cfd
Item sword {
    name: "Fire Sword",
    rarity: Rare,
}
```

支持 scalar、字符串、enum、null、数组、字典、内联对象和跨文件引用。解析器先生成带 span 的语法树，runtime 再使用 CFT schema 完成类型转换、默认值、引用和维度 overlay。

目录只发现 `.cfd`；文件顺序和 source identity 稳定排序。未知字段、重复 key、类型不匹配和悬空引用都会在 `check` 中报告。
