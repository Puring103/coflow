# 单元格值语法

**依赖文档**：[01-cft.md](01-cft.md)

单元格值语法（Cell Value Syntax）用于在 Excel 单元格中表达复杂数据结构。解析器是完全 **schema-guided** 的——每个单元格的目标类型由对应列的 CFT 字段类型确定，解析器根据类型上下文消解所有歧义。

---

## 目录

1. [分隔符与括号省略规则](#1-分隔符与括号省略规则)
2. [标量值](#2-标量值)
3. [枚举值](#3-枚举值)
4. [对象](#4-对象)
5. [数组](#5-数组)
6. [字典](#6-字典)
7. [多态对象](#7-多态对象)
8. [skip 与 null](#8-skip-与-null)
9. [字符串引号规则](#9-字符串引号规则)
10. [完整示例](#10-完整示例)

---

## 1. 分隔符与括号省略规则

| 分隔符 | 作用 |
|--------|------|
| `,` | 分隔对象字段、字典条目 |
| `\|` | 分隔数组元素 |
| `{}` | 标记嵌套对象边界 |
| `[]` | 标记嵌套数组边界 |

**省略规则：只有最顶层（单元格根节点）可以省略括号；嵌套对象和嵌套数组必须保留括号。**

```
// 字段类型 Stats，顶层省略 {}
100, 50

// 字段类型 Monster，顶层省略 {}，但内部 stats 必须保留 {}
slime, 5, {100, 50}

// 字段类型 [Stats]，顶层省略 []，但嵌套数组必须保留 []
100, 50 | 200, 80

// 字段类型 Zone { monsters: [Monster]; }，顶层省略 {}，monsters 字段的嵌套数组保留 []
forest, [slime, 5, {100, 50} | goblin, 10, {200, 80}]
```

空白字符（空格、换行）可以任意添加或省略。

---

## 2. 标量值

标量值直接裸写，不需要引号。schema 类型决定解析方式：

```
// int 字段
42
-3

// float 字段
3.14
-1.5

// bool 字段
true
false

// string 字段（schema 知道是 string，裸写即可）
slime_01
hello world
火焰之剑
123abc      // schema 说是 string，就是字符串，不是数字
true        // schema 说是 string，就是字符串，不是 bool
```

---

## 3. 枚举值

枚举值可以省略类型前缀，schema 已知类型时直接写变体名：

```
// Rarity 字段
Rare                    // 省略前缀
Rarity.Rare             // 显式写法，任何场景均合法
```

---

## 4. 对象

### 具名风格（named）

字段名 `:` 值，任意顺序，有默认值的字段可以省略：

```
// Stats { hp: int; attack: int; speed: float = 1.0; }
hp: 100, attack: 50
hp: 100, speed: 2.0     // 跳过 attack（attack 必须有默认值才合法）
hp: 100, attack: 50, speed: 1.5
```

### 位置风格（positional）

按字段声明顺序填值，末尾连续的有默认值字段可以省略：

```
// Stats { hp: int; attack: int; speed: float = 1.0; }
100, 50             // speed 使用默认值 1.0
100, 50, 1.5        // 全部字段
```

**具名和位置风格在同一个对象中禁止混用。**

`TypeName{}` 内部同样遵循具名/位置风格规则。

---

## 5. 数组

数组元素之间用 `|` 分隔，最外层 `[]` 可以省略：

```
// [int]
1 | 2 | 3

// [string]
warrior | tank | elite

// [Rarity]
Rare | Epic | Common

// [Stats]（每个元素是对象，字段用 , 分隔，元素用 | 分隔）
100, 50 | 200, 80 | 300, 150
```

嵌套数组必须保留 `[]`：

```
// Zone { name: string; monsters: [Monster]; }
forest, [slime, 5, {100, 50} | goblin, 10, {200, 80}]
```

---

## 6. 字典

字典条目之间用 `,` 分隔，最外层 `{}` 可以省略：

```
// {DamageType: float}，enum key 裸写变体名（schema 已知类型）
Fire: 0.5, Ice: 0.2, Physical: 1.0

// {string: int}
alice: 10, bob: 20

// {int: string}
1: sword, 2: shield
```

字典 string key 遵循与普通字符串相同的引号规则（见第 9 节）。

字典解析完成后按 schema 类型转换 key，并检查重复：

- `string` key 按字符串内容比较
- `int` key 按整数值比较
- `enum` key 按“枚举类型 + 底层整数值”比较

重复 key 是加载错误，不允许后写覆盖。

---

## 7. 多态对象

字段声明类型是父类（包括 `abstract type`，或有子类的普通 `type`）时，必须用 `TypeName{}` 显式标记实际类型。解析器扫描到第一个 `{` 前的标识符即为类型名，`{` 后按该类型的字段 schema 继续解析：

```
// Reward 字段（abstract，ItemReward 和 CurrencyReward 是子类）
CurrencyReward{100}
ItemReward{sword_01, 1}

// [Reward]（多态数组，| 分隔，每个元素带 TypeName{}）
CurrencyReward{100} | ItemReward{sword_01, 1} | CurrencyReward{50}
```

字段类型是具体 `type` 且字段声明类型不是父类时，不需要 `TypeName{}`，直接填对象内容。

---

## 8. skip 与 null

| 写法 | 语义 |
|------|------|
| 空单元格 | 使用默认值（字段必须有默认值，否则报错） |
| `_` | 同空单元格，positional 风格中用于跳过中间字段 |
| `null` | 显式填 null，字段类型必须是 `T?`，否则报错 |

```
// Drop { item: Item? = null; count: int = 1; note: string = ""; }
_               // item=null（默认）, count=1, note=""
_, 3            // item=null（默认）, count=3, note=""
null, 3         // item=null（显式）, count=3, note=""
sword_01        // item=sword_01, count 和 note 用默认值
_, _, notice    // item=null, count=1, note="notice"
```

`_` 在 named 风格中不需要（直接省略有默认值的字段即可）。

字符串字面量 `"_"` 表示内容为下划线的字符串，不是 skip。

---

## 9. 字符串引号规则

绝大多数字符串不需要引号，以下情况**必须**加双引号：

| 情况 | 示例 |
|------|------|
| 含 `,` | `"hello, world"` |
| 含 `\|` | `"fire\|ice"` |
| 含 `:` | `"key: value"` |
| 含 `{` `}` `[` `]` | `"a{b}"` |
| 空字符串 | `""` |
| 内容为 `_` | `"_"` |
| 内容为 `null` | `"null"` |

转义规则遵循标准 JSON 转义：`\"` `\\` `\n` `\r` `\t`。

其他情况裸写即可，包括含空格、中文、数字开头等：

```
hello world         // 合法裸字符串
火焰之剑             // 合法
123abc              // schema 是 string 时合法
```

---

## 10. 完整示例

以下示例基于 CFT 综合示例中的类型定义：

```
// string 字段
slime_01

// int 字段
42

// Rarity 字段（枚举省略前缀）
Rare

// Stats（positional，省略末尾默认值字段）
100, 50

// Stats（named，跳过有默认值的字段）
hp: 100, speed: 2.0

// Item（positional，最外层 {} 省略）
sword_01, 铁剑, Rare, ["weapon", "melee"]

// [Stats]（positional，| 分隔元素）
100, 50 | 200, 80, 1.5 | 300, 150

// [Reward]（多态数组）
CurrencyReward{r1, 100} | ItemReward{r2, sword_01, 1} | CurrencyReward{r3, 50}

// {DamageType: float}（字典，省略最外层 {}）
Fire: 0.5, Ice: 0.2, Physical: 1.0

// Monster（named，嵌套对象和数组）
id: slime, level: 5, stats: {100, 50}, rarity: Common

// [Monster]（| 分隔，嵌套对象用 {}）
slime, 5, {100, 50}, Common | goblin, 10, {200, 80}, Rare

// Drop（_ 跳过使用默认值，null 显式填 null）
null, 3

// DropTable（rewards 是多态数组，weights 是 int 数组）
rewards: CurrencyReward{r1, 100} | ItemReward{r2, sword_01}, weights: 60 | 40
```
