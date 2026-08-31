# CFT Schema

CFT 描述类型、字段、默认值、枚举、引用、继承、多态、维度注解和 `check`。它不读文件，也不包含记录数据。

```cft
enum Rarity { Common; Rare; }

type Item {
  name: string;
  rarity: Rarity = Common;
}

check Item {
  name != "";
}
```

CFT 使用空格缩进，每一级固定为 2 个空格，不使用制表符。编辑器格式化和 `Tab` 键遵循这一约定。

`Type` 表示内联对象，`&Type` 表示按 record key 引用，`T?` 表示 nullable。`@localized` 和 `@dimension(name)` 将字段绑定到项目配置中的 CFD overlay 维度。
