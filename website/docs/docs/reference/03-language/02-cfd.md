# CFD 数据语言

CFD 为 CFT 声明提供记录数据。加载时检查类型、补齐默认值并链接引用；
函数与模板在构建阶段完成类型检查和编译，成功发布 Runtime 后按调用或读取请求执行。

```cfd
use game::Item;
use game::Stats;

sword: Item {
  name: "Sword",
  stats: Stats { hp: 100 },
  next: &Item::shield,
  tags: ["weapon"],
}
shield: Item {
  name: "Shield",
  stats: Stats { hp: 150 },
}
```

文件以可选的 `use` 导入开头，随后逐条写 `key: Type { ... }`。
CFD 不声明 namespace、类型、默认值或 check。记录类型必须为 table 或 singleton，
Host singleton 由宿主提供，不在 CFD 中声明。

字段和集合项使用逗号分隔，允许尾逗号。注释从 `#` 延续到行尾。
数组和字典保留输入顺序，字段与字典 key 不能重复。

## 对象、可选值与引用

内联数据写 `Stats { hp: 100 }`，其中 Stats 必须是 data。`{ "hp": 100 }` 是字典。
抽象 data 字段需要提供具体子类型；table 和 singleton 通过引用使用。

省略字段时使用声明默认值；无默认值的可选字段为 None，其余字段必填。
可选值写 `None` 或直接写非空值；`Some(...)` 不是值语法。

记录 key 在整棵继承树内唯一，父子和兄弟类型不能重复，无关类型可以同 key。
`&Type::key` 查找该类型及其子类型的记录。`&key` 使用源码所在静态本类型，
不根据字段期望类型猜测目标。引用可以跨文件、自引用或形成记录循环。
程序读取记录身份使用 `record.id`。

## 文本与函数源码

普通字符串写 `"text"`，其中的花括号不触发插值。
fstring 字段写 `f"名称：{self.name}"`，函数字段写完整签名：

```cfd
item: Item {
  name: "Sword",
  label: f"名称：{self.name}",
  price: 100,
  total: fn(count: int) -> int { self.price * count },
}
```

该示例要求 Item 声明对应字段。运行时按声明签名编译函数和模板；函数在调用时执行，
fstring 在普通读取时求值。

对象字段及其集合中直接声明的函数和模板绑定字段所属对象；
嵌套 data 自身字段中的字面量绑定嵌套对象。已有值复制后保留原绑定。
维度覆盖使用原业务对象绑定，见 [本地化与维度](../10-localization.md)。
