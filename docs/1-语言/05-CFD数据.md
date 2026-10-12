# CFD 数据

## 记录与分组

CFD 使用项目 CFT 声明的类型提供记录、字段值和维度覆盖。

```cfd
sword: Item { name: "Sword", price: 100 }
Item {
  shield { name: "Shield", price: 80 },
  bow: RangedItem { name: "Bow", price: 120, range: 10 },
}
```

独立记录使用 `key: Type { ... }`。分组使用 `Type { key { ... } }`，其中显式提供的实际类型必须等于分组类型或继承自该类型。abstract 类型可以作为分组限定，实际记录必须为具体类型。

记录 key 在整棵继承树内唯一，不同继承树可以有相同 key。同一记录字段只能提供一次；多余字段、类型错误和重复 key 产生诊断。

singleton 必须有且只有一条以声明名称为 key 的记录。data 只用于内联对象，table 与 singleton 提供顶层记录。

## 字段值

```cfd
entry: Entry {
  count: 2147483648,
  ratio: 1.0000000000000002,
  enabled: true,
  text: "{literal}",
  label: f"数量 {self.count}",
  payload: Payload { score: 7 },
  target: &Item::sword,
  optional: Some(None),
  values: [1, 2, 3],
  lookup: { "first": 1, "second": 2 },
}
```

内联对象可以提供实际类型，也可以在字段声明足以确定类型时使用 `{ field: value }`。对象字段与字典项根据 schema 解释。数组、字典和字段允许末尾逗号，相邻字段之间需要逗号。

省略字段按 schema 默认值或 Option 默认空态物化。必填字段缺失属于数据诊断，编辑器可以保留记录及明确的 missing 状态。类型不一致的值不伪装成缺失字段。

数字按 int64 或有限 binary64 验证；enum 使用成员名或 `Enum::Member`。flag 组合使用同类型成员的 `&`、`^`、`|` 和括号。常量通过项目全局名称解析。

## 引用与 Option

`&Type::key` 明确记录类型；`&key` 在字段已知目标类型时按该类型的继承域查找。查找到的实际类型仍必须符合字段声明。所有记录先建立身份，引用可以跨文件或成环。

Option 可以使用 `None`、`Some(value)` 或在明确可选位置提供裸值。每层状态独立保留；嵌套可选用显式构造器表达不同层次。

## 维度覆盖

```cfd
language[ja] {
  sword: Item { name: "剣" },
}
```

覆盖只提供明确写出的字段，不物化该记录的其他字段。目标记录必须已存在，字段必须绑定该维度；同一维度、变体、记录、字段重复覆盖报错。显式 None 覆盖保留为空态。

文件加载顺序不决定覆盖优先级。全部覆盖共同构成变体集合，未覆盖字段使用基础值。维度读取和来源契约见[维度](./09-维度.md)。

## 来源与容错

来源文件与记录、字段和值的跨度用于诊断、源码导航和局部写入。编辑会话接纳可修复的缺失字段或引用错误，拒绝无法由结构化模型表达的记录；严格消费者在存在数据错误时失败。具体行为见 [编辑器容错数据加载](../editor-tolerant-data-loading.zh-CN.md)。
