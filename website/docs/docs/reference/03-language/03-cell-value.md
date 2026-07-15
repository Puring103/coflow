# 单元格值语法

单元格值语法用于 Excel 和 CSV 表格。它是 schema-guided 的：每个单元格都会按 CFT 字段类型解析。

同一段文本在不同目标类型下含义可能不同。例如 `true` 在 `bool` 字段里是布尔值，在 `string` 字段里是字符串。

## 核心分隔符

| 语法 | 作用 |
| --- | --- |
| `,` | 分隔对象字段或字典条目 |
| `|` | 分隔数组元素 |
| `{}` | 对象或字典边界 |
| `[]` | 数组边界 |
| `:` | 字段名和值、字典 key 和 value 的分隔符 |

分隔符只在括号深度为 0 且不在字符串内时生效。

## 空值、跳过与 null

| 写法 | 位置 | 语义 |
| --- | --- | --- |
| 空单元格 | root | 字段未提供，使用 schema 默认值 |
| `_` | root | 字段未提供，使用 schema 默认值 |
| `_` | 对象字段值 | 跳过当前字段 |
| `null` | 任意值位置 | 显式 null，目标类型必须是 `T?` |

字符串内容如果就是 `_` 或 `null`，需要加双引号：

```text
"_"
"null"
```

## 标量

标量按目标类型解析：

```text
42
3.14
true
false
"Fire Sword"
```

`bool` 字段还接受大小写不敏感的别名：

| true | false |
| --- | --- |
| `1` | `0` |
| `yes` | `no` |
| `y` | `n` |

`float` 只接受有限数，不接受 `NaN`、`inf` 或 `-inf`。

## 字符串

字符串可以裸写：

```text
slime_01
hello world
火焰之剑
```

以下情况建议或必须使用双引号：

| 情况 | 示例 |
| --- | --- |
| 含 `,` | `"hello, world"` |
| 含 `|` | `"fire|ice"` |
| 含 `:` | `"key: value"` |
| 含 `{}` 或 `[]` | `"a{b}"` |
| 空字符串 | `""` |
| 内容是 `_` | `"_"` |
| 内容是 `null` | `"null"` |

转义使用 JSON 风格：`\"`、`\\`、`\n`、`\r`、`\t`。

## 枚举

枚举字段可以写变体名，也可以写完整枚举名：

```text
Rare
Rarity.Rare
```

完整写法的枚举名前缀必须与目标枚举类型一致。

## 对象

对象可以按字段顺序填写：

```text
100, 50
```

也可以按字段名填写：

```text
hp: 100, attack: 50
```

一个对象内不要混用位置写法和字段名写法。

嵌套对象必须写 `{}`：

```text
level: 5, stats: {hp: 100, attack: 50}
```

## 数组

数组元素用 `|` 分隔：

```text
weapon | melee | early
```

root 数组可以省略最外层 `[]`，嵌套数组必须写 `[]`：

```text
tags: [weapon | melee]
```

对象数组中，每个对象元素必须有对象边界：

```text
{hp: 100, attack: 50} | {hp: 200, attack: 80}
```

注意：表格单元格数组使用 `|`，CFD 文件中的数组使用 `,`。

## 字典

字典条目使用 `key: value`，条目之间用 `,` 分隔：

```text
Fire: 1.25, Ice: 1.0
```

root 字典可以省略最外层 `{}`，嵌套字典必须写 `{}`：

```text
weaknesses: {Fire: 1.25, Ice: 1.0}
```

字典 key 类型由 CFT 决定，只支持 `string`、`int` 或 enum key。重复 key 会报错，不以后写覆盖。

## 记录引用

字段类型为 `&Type` 时，单元格写 key-only 记录引用：

```text
&sword_fire
```

目标类型来自 CFT 字段类型，例如 `item: &Item;`、`items: [&Item];` 或 `{string: &Item}`。`&key` 只引用顶层 record，不支持 `.field` 或 `[index]` 路径访问。

目标类型是 `string` 时，`&sword_fire` 只是普通字符串。目标类型是 `&Type` 时，裸 `sword_fire` 不会被当成引用，应写成 `&sword_fire`。

## 多态对象

字段类型是父类时，需要写实际子类型：

```text
CurrencyReward{amount: 100}
ItemReward{item: &sword_fire, count: 1}
```

多态数组：

```text
CurrencyReward{amount: 100} | ItemReward{item: &sword_fire, count: 1}
```

`TypeName{...}` 中的 `TypeName` 必须能赋给目标字段类型。

## 引用与内联对象

CFT 字段类型决定单元格形态：`&Item` 必须写 `&sword_fire`，`Item` 必须写内联对象。数组和字典会递归应用内层类型，例如 `[&Item]` 的元素写 `&key`，`[Item]` 的元素写对象。

## 完整示例

| CFT 字段类型 | 单元格内容 |
| --- | --- |
| `string` | `Fire Sword` |
| `int` | `100` |
| `Rarity` | `Rarity.Rare` |
| `[string]` | `weapon | melee` |
| `{Element: float}` | `Fire: 1.25, Ice: 1.0` |
| `Stats` | `hp: 100, attack: 20` |
| `[Stats]` | `{hp: 100, attack: 20} | {hp: 200, attack: 40}` |
| `&Item` | `&sword_fire` |
| `Reward` | `ItemReward{item: &sword_fire, count: 1}` |

## 常见错误

| 错误写法 | 为什么错 | 推荐做法 |
| --- | --- | --- |
| `1, 2, 3` 写给 `[int]` | 单元格数组不用逗号分隔 | 写 `1 | 2 | 3` |
| 嵌套数组写成 `a | b` | 嵌套数组不能省略 `[]` | 写 `[a | b]` |
| 对象数组写成 `hp:100 | hp:200` | 对象元素缺少 `{}` | 写 `{hp:100} | {hp:200}` |
| `item: sword_fire` | 裸 key 不是对象引用 | 写 `item: &sword_fire` |
| `null` 写给 `string` | `null` 只允许用于 nullable | 改为 `string?` 或写字符串 |
