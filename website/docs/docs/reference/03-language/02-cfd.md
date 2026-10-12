# CFD 配置文本

CFD 提供 CFT 声明的基础记录与维度覆盖。应用选择互相引用的一组文件并一起加载。

```cfd
sword: Item {
  name: "Sword",
  stats: Stats { hp: 100 },
  quality: Quality::Rare,
  next: Some(&Item::shield),
}
Item {
  shield {
    name: "Shield",
    stats: { hp: 80 },
    next: None,
  },
}
language[ja] { sword: Item { name: "剣" } }
```

独立记录使用 `key: Type { ... }`，分组使用 `Type { key { ... } }`。分组内显式提供的实际类型必须属于该分组类型的继承域。记录 key 使用语言标识符，在整棵类型继承树内唯一。

singleton 的唯一记录 key 为声明名称。字段之间使用逗号，可以保留末尾逗号；`#` 开始行注释。重复 key、重复字段、多余字段、类型错误和未知类型均产生诊断。

## 值

| 值 | 示例 |
| --- | --- |
| 整数、浮点、布尔 | `2147483648`、`1e100`、`true` |
| 普通字符串 | `"{literal}"` |
| 插值文本 | `f"价格：{self.price}"` |
| 对象 | `Stats { hp: 100 }` 或已知类型位置的 `{ hp: 100 }` |
| 引用 | `&Item::sword` 或目标类型已知时的 `&sword` |
| 可选值 | `None`、`Some(3)`、`Some(None)` |
| 数组、字典 | `[1, 2]`、`{ "a": 1 }` |
| 枚举、flag | `Quality::Rare`、`Read | Write` |
| 常量 | CFT 中已声明的全局常量名 |

整数为有符号 64 位，浮点为有限 binary64。Option 可以嵌套并保留每层状态；裸值在可选字段中表示一层 Some。省略字段按默认值或 Option 空态补全，其他必填字段缺失产生诊断。

引用在所有记录身份建立后连接，可跨文件和成环。普通字符串中的花括号保持原文，fstring 在加载时按字段路径生成文本，路径末端必须是可转为文本的基础值。

## 覆盖

`dimension[variant]` 覆盖已有记录的维度字段，只替换明确提供的完整字段值。未覆盖字段使用基础值，显式 None 保持为空。同一维度、变体、记录、字段重复覆盖时报错，文件顺序不提供覆盖优先级。

维度的选择与读取见 [维度与本地化](../10-localization.md)。
