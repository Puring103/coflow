# 单元格值语法

**依赖文档**：[01-cft.md](01-cft.md)

单元格值语法（Cell Value Syntax）用于在 Excel 单元格中表达复杂数据结构。解析器是 **schema-guided** 的：每个单元格都有一个由 CFT 字段类型提供的目标类型，所有语法歧义都由该目标类型消解。

解析器输入为：

- 目标 CFT 类型，例如 `int`、`Stats`、`[Reward]`、`{string: int}`
- 单元格文本

解析器输出为：

- `Omitted`：字段未提供，后续数据模型按 schema 默认值处理
- `Value`：结构化输入值，后续交给数据模型做字段必填、默认值、重复 key、`@ref` 解析和类型兼容校验

---

## 目录

1. [核心规则](#1-核心规则)
2. [边界省略规则](#2-边界省略规则)
3. [标量与字符串](#3-标量与字符串)
4. [枚举](#4-枚举)
5. [对象](#5-对象)
6. [数组](#6-数组)
7. [字典](#7-字典)
8. [多态对象](#8-多态对象)
9. [skip 与 null](#9-skip-与-null)
10. [完整示例](#10-完整示例)

---

## 1. 核心规则

| 分隔符 | 作用 |
|--------|------|
| `,` | 分隔对象字段、字典条目 |
| `\|` | 分隔数组元素 |
| `{}` | 标记对象或字典边界 |
| `[]` | 标记数组边界 |

切分 `,` 和 `|` 时，只在 **括号深度为 0 且不在字符串内** 的位置切分。引号内的分隔符只是字符串内容。

所有复合语法都由目标类型消歧：

- 目标类型是 `type` 时，`{...}` 按对象解析
- 目标类型是 `{K: V}` 时，`{...}` 按字典解析
- 目标类型是 `string` 时，`true`、`123`、`Rare` 都是字符串内容，不会按 bool、int、enum 解析
- `@ref` 字段按其声明的 id 类型解析为 string 或 int；引用查找由数据模型阶段根据 `@ref(TypeName)` 完成

数组永远使用 `|` 分隔。对象和字典永远使用 `,` 分隔。**不支持逗号数组**。

---

## 2. 边界省略规则

只有单元格根节点可以省略自己的边界。嵌套复合值必须写边界。

| 位置 | 目标类型 | 是否可省略边界 | 示例 |
|------|----------|----------------|------|
| root | `Stats` | 可以省略 `{}` | `100, 50` |
| root | `[int]` | 可以省略 `[]` | `1 | 2 | 3` |
| root | `{string: int}` | 可以省略 `{}` | `alice: 10, bob: 20` |
| nested | `Stats` | 必须写 `{}` | `{100, 50}` |
| nested | `[int]` | 必须写 `[]` | `[1 | 2 | 3]` |
| nested | `{string: int}` | 必须写 `{}` | `{alice: 10, bob: 20}` |

数组元素是嵌套值，所以对象数组必须给每个对象元素写 `{}`：

```
// [Stats]
{100, 50} | {200, 80} | {300, 150}
```

多态对象使用 `TypeName{...}`，它自带对象边界，可以出现在 root 或 nested 位置。

---

## 3. 标量与字符串

标量按目标类型解析：

```
// int
42
-3

// float
3.14
-1.5

// bool
true
false
```

`float` 只接受有限数；`NaN`、`inf`、`-inf` 等非 JSON number 值必须作为类型不匹配处理。

字符串可以裸写，但以下内容必须加双引号：

| 情况 | 示例 |
|------|------|
| 含 `,` | `"hello, world"` |
| 含 `\|` | `"fire\|ice"` |
| 含 `:` | `"key: value"` |
| 含 `{` `}` `[` `]` | `"a{b}"` |
| 空字符串 | `""` |
| 内容为 `_` | `"_"` |
| 内容为 `null` | `"null"` |

转义规则使用 JSON 风格的常用转义：`\"`、`\\`、`\n`、`\r`、`\t`。

合法裸字符串示例：

```
slime_01
hello world
火焰之剑
123abc
true        // 目标类型是 string 时，这是字符串 "true"
```

非法裸字符串示例：

```
hello, world
fire|ice
key: value
_
null
```

---

## 4. 枚举

目标类型是枚举时，可以写裸变体名，也可以写显式枚举前缀：

```
// Rarity
Rare
Rarity.Rare
```

显式前缀必须和目标枚举类型一致。

---

## 5. 对象

对象支持 positional 和 named 两种风格。一个对象内不能混用这两种风格。

### 5.1 Positional

按字段声明顺序填值。缺失字段交给数据模型按默认值规则处理；如果缺失字段没有默认值，数据模型报错。

```
// Stats { hp: int; attack: int; speed: float = 1.0; }
100, 50
100, 50, 1.5
```

`_` 可以跳过当前位置字段：

```
// Drop { item: string? = null; count: int = 1; note: string = ""; }
_, 3
_, _, notice
```

### 5.2 Named

字段名 `:` 值，字段顺序任意。未出现的字段交给数据模型按默认值规则处理。

```
// Stats { hp: int; attack: int = 50; speed: float = 1.0; }
hp: 100, attack: 60
hp: 100, speed: 2.0
speed: 2.0, hp: 100
```

named 风格中 `field: _` 等价于不写该字段，但通常直接省略字段更清楚。

### 5.3 嵌套对象

嵌套对象必须写 `{}`：

```
// Monster { id: string; level: int; stats: Stats; }
slime, 5, {100, 50}
id: slime, level: 5, stats: {hp: 100, attack: 50}
```

---

## 6. 数组

数组元素之间用 `|` 分隔。root 数组可以省略最外层 `[]`。

```
// [int]
1 | 2 | 3
[1 | 2 | 3]

// [string]
warrior | tank | elite

// [Rarity]
Rare | Epic | Common
```

数组元素是对象时，每个对象元素必须写 `{}`：

```
// [Stats]
{100, 50} | {200, 80} | {300, 150}
```

数组嵌套在对象字段、字典值或另一个数组中时，数组本身必须写 `[]`：

```
// Zone { name: string; monsters: [Monster]; }
forest, [{slime, 5, {100, 50}} | {goblin, 10, {200, 80}}]
```

---

## 7. 字典

字典条目之间用 `,` 分隔，key 和 value 之间用 `:`。root 字典可以省略最外层 `{}`。

```
// {DamageType: float}
Fire: 0.5, Ice: 0.2, Physical: 1.0
{Fire: 0.5, Ice: 0.2, Physical: 1.0}

// {string: int}
alice: 10, bob: 20

// {int: string}
1: sword, 2: shield
```

字典 key 由 schema 类型转换：

- `string` key 按字符串内容比较
- `int` key 按整数值比较
- `enum` key 按“枚举类型 + 底层整数值”比较

重复 key 是加载错误，不允许后写覆盖。

字典嵌套在对象字段、数组元素或另一个字典值中时，必须写 `{}`：

```
id: slime, attrs: {hp_bonus: 10, atk_bonus: 5}
```

---

## 8. 多态对象

字段声明类型是父类时，必须用 `TypeName{...}` 标记实际类型。父类包括 `abstract type`，以及有子类的普通 `type`。

```
// Reward 字段
CurrencyReward{r1, 100}
ItemReward{r2, sword_01, 1}
```

`TypeName{...}` 中的 `TypeName` 必须是目标类型的具体可赋值类型。`{...}` 内部仍然按对象 positional 或 named 规则解析。

多态数组：

```
// [Reward]
CurrencyReward{r1, 100} | ItemReward{r2, sword_01, 1} | CurrencyReward{r3, 50}
```

多态数组嵌套在对象字段中时，数组本身必须写 `[]`：

```
rewards: [CurrencyReward{r1, 100} | ItemReward{r2, sword_01, 1}]
```

---

## 9. skip 与 null

| 写法 | 位置 | 语义 |
|------|------|------|
| 空单元格 | root | 整个字段 `Omitted` |
| `_` | root | 整个字段 `Omitted` |
| `_` | 对象字段值 | 跳过当前字段 |
| `null` | 任意值位置 | 显式 null，目标类型必须是 `T?` |

```
// Drop { item: string? = null; count: int = 1; note: string = ""; }
_               // root omitted，整个 Drop 字段使用默认值
_, 3            // item omitted，count=3，note 使用默认值
null, 3         // item 显式 null，count=3
_, _, notice    // item 和 count omitted，note="notice"
```

字符串内容为 `_` 或 `null` 时必须加引号：

```
"_"
"null"
```

---

## 10. 完整示例

以下示例基于 CFT 综合示例中的类型定义：

```
// string
slime_01

// int
42

// Rarity
Rare

// Stats（root object，省略最外层 {}）
100, 50
hp: 100, speed: 2.0

// Item（root object，tags 是嵌套数组，必须写 []）
sword_01, 铁剑, Rare, [weapon | melee]

// [Stats]（root array，省略最外层 []，对象元素必须写 {}）
{100, 50} | {200, 80, 1.5} | {300, 150}

// [Reward]（root polymorphic array）
CurrencyReward{r1, 100} | ItemReward{r2, sword_01, 1} | CurrencyReward{r3, 50}

// {DamageType: float}（root dict，省略最外层 {}）
Fire: 0.5, Ice: 0.2, Physical: 1.0

// Monster（嵌套 object 和 array）
id: slime, level: 5, stats: {100, 50}, tags: [warrior | melee]

// [Monster]
{slime, 5, {100, 50}, [warrior]} | {goblin, 10, {200, 80}, [elite | melee]}

// Drop
null, 3

// DropTable（嵌套数组必须写 []）
rewards: [CurrencyReward{r1, 100} | ItemReward{r2, sword_01, 1}], weights: [60 | 40]
```
