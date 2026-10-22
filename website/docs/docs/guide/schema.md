# Schema 建模

CFT 定义类型、默认值、引用、继承、多态、维度和 check。先确定哪些对象拥有独立身份，再决定字段如何组合。

- `table` 表示可按 ID 查询和引用的记录，例如物品和技能。
- `data` 表示属于某个字段的内联值，例如坐标和属性集合。
- `singleton` 表示项目中的唯一配置，例如全局参数。

```cft
data Stats { hp: int; attack: int = 0; }
table Item {
  name: string;
  stats: Stats;
  next: Option<&Item> = None;
  check Valid { stats.hp > 0: "生命值必须为正"; }
}
singleton Settings { startingItem: &Item; }
```

需要复用结构时使用 data；需要多个记录指向同一对象时使用 `&TableName`。默认值表达可省略字段的常用取值，check 表达业务约束。本地化字段使用 `@localized`，其他维度使用 `@dimension("名称")`。

全部 CFT 文件共享项目名称空间，文件划分不改变类型身份。修改后运行 `coflow schema check --project <项目目录>`，再用 `coflow check --rules` 验证现有数据。完整规则见 [CFT 参考](../reference/03-language/01-cft.md)。
