# 为类型编写 CFD

文件使用 UTF-8，`#` 为行注释。记录 ID 是标识符，字段和集合项用逗号，允许末尾逗号。配套完整类型见[示例 Schema](../examples/minimal-project/schema/main.cft)。

## 记录与分组

```cfd
sword: Item {
  name: "铁剑",
  price: 90,
  next: Some(&Item::shield),
}
Item {
  shield { name: "盾牌", price: 80 },
}
Settings: Settings { start: &Item::sword }
language[ja] { sword: Item { name: "剣" } }
```

独立形式为 `id: Type { ... }`，分组为 `Type { id { ... } }`。分组内指定派生类型可写 `bow: RangedItem { ... }`，前提是声明了相应继承关系及全部字段。记录实际类型必须可实例化。

singleton 记录 ID 必须等于类型名。data 仅出现在字段值中，不能成为顶层记录。ID 在整棵继承树内唯一；重复记录、重复字段或未知字段均是错误。

## 值表达式

| 值 | CFD 写法 |
| --- | --- |
| 数值、布尔 | `100`、`1.5`、`1e3`、`true` |
| 字符串 | `"铁剑"` |
| 模板 | `f"{self.name}：{self.price}"` |
| 枚举、位组合 | `Quality::Rare`、`Permission::Read \| Permission::Write` |
| 引用 | `&Item::sword`；字段目标类型已知时 `&sword` |
| Option | `None`、`Some(3)`、`Some(None)` |
| 数组 | `[1, 2]` |
| 字典 | `{ "title": "物品" }` |
| data 对象 | `Stats { hp: 100 }`，类型可推断时 `{ hp: 100 }` |
| 常量 | 已声明的 CFT 常量名 |

值按 CFT 类型检查范围，Option 层级和内联对象的必填规则同样适用。声明类型为抽象父类时写出具体子类型；不能用普通对象内联构造 table。

## 省略与引用

只有显式 CFT 默认值允许省略字段，包括对象内部字段。None、空字符串、空集合与省略不同；裸值在 Option 位置表示一层 Some，嵌套状态宜显式书写。

所有记录身份建立后解析引用，允许跨文件和循环引用，但必须一起提供所有依赖的基础记录。引用按记录身份比较，内联对象按实际类型和字段内容比较。

## 模板

普通字符串中的花括号保持文本。f 字符串的 `{{` 和 `}}` 表示文字花括号，self 指模板所属对象；嵌套 data 中的 self 指该内联对象。

模板读取字段路径，如 `f"{self.name}"` 或 `f"搭配 {&Item::sword.name}"`，末端必须为可转文本的标量：string、数值、bool、enum。不能直接把 Option、对象、集合转成文本，模板也不执行任意算术和方法调用。

模板按当前维度上下文求值，缺失路径与循环依赖会报错。修改模板时保留 f 源表达式，不能把渲染结果当成等价源码。

## 维度覆盖

`language[ja] { ... }` 覆盖已存在记录的绑定字段。只填写待覆盖的顶层字段，对象、数组和字典按整个字段替换；对象内部仍补齐 CFT 默认值。空块也声明变体，重复覆盖不会按文件顺序覆盖而是报错。

组织覆盖和检查回退见[维度建模](dimensions.md)。完成写入后用 check --rules 验证；需要规范源码时使用 coflow format。
