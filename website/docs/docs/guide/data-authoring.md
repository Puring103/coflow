# 编写 CFD

CFD 是唯一数据格式。顶层记录使用声明类型和 ID，字段值遵循 CFT 类型：

```cfd
sword: Item {
  name: "Fire Sword",
  tags: ["weapon", "fire"],
}
```

记录 ID 在同一类型继承树内唯一。字段之间用逗号分隔，`#` 开始行注释；省略字段使用 CFT 默认值，没有显式默认值的字段必须填写，Option 字段也一样。引用使用 `&Item::sword`，也可以在已知目标类型时写 `&sword`。

编辑 `.cfd` 后运行 `coflow check --rules --project <project>`，同时检查结构和业务规则。需要更新生成代码时运行 `coflow build --project <project>`。生成目录专门保存代码产物。

维度覆盖使用 `language[ja] { sword: Item { name: "鉄の剣" } }`，被覆盖字段须在 CFT 中声明对应维度。完整语法见 [CFD 参考](../reference/03-language/02-cfd.md)，文件组织见[本地化与维度](../reference/10-localization.md)。
