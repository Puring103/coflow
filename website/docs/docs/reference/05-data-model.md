# 数据模型

Coflow 会把 Excel、CSV、CFD 和维度文件中的数据合并成统一的数据模型。无论记录来自哪种数据源，它们都遵循相同的 CFT 类型、默认值、引用和校验规则。

配置作者需要理解合并后的数据语义，因为 `check`、导出、代码生成和编辑器都以这份统一结果为准。

## 记录与类型

每条顶层记录由两部分确定：

- record key，例如 `sword_fire`；
- 实际类型，例如 `Item` 或 `CurrencyReward`。

```cfd
sword_fire: Item {
  name: "Fire Sword",
}
```

record key 相当于记录的业务 ID，不需要也不能再声明为普通 `id` 字段。CFT `check` 中可以通过虚拟字段 `id` 读取它。

实际类型决定记录拥有哪些字段、执行哪些继承校验，以及导出多态对象时使用哪个具体类型。

## 值类型

数据模型支持以下值：

| CFT 类型 | 数据含义 |
| --- | --- |
| `int` | 64 位整数 |
| `float` | 有限浮点数，不允许 `NaN` 或无穷值 |
| `bool` | 布尔值 |
| `string` | 字符串 |
| enum | 指定 enum 的值 |
| `TypeName` | 内联对象 |
| `&TypeName` | 指向顶层记录的引用 |
| `[T]` | 有序数组 |
| `{K: V}` | 字典 |
| `T?` | 可显式取 `null` 的值 |

字典 key 只允许 `string`、`int` 或 enum。同一个字典中不能出现重复 key。

## 内联对象与记录引用

内联对象是所在记录的一部分，没有独立 record key，也不能被其他记录引用：

```cfd
stats: { hp: 100, attack: 20 }
```

记录引用指向另一条顶层记录：

```cfd
featured_item: &sword_fire
```

CFT 字段类型决定必须使用哪一种形态：

| 字段类型 | 合法数据 |
| --- | --- |
| `Item` | 内联 `Item` 对象 |
| `&Item` | `&key` 记录引用 |
| `[&Item]` | 记录引用数组 |
| `{string: &Item}` | value 为记录引用的字典 |

内联对象不能写成裸 record key，记录引用也不能用内联对象代替。

## 继承与多态

子类型的记录或对象可以赋给父类型位置。例如 `ItemReward` 是 `Reward` 的子类型时：

```cft
type Drop {
  reward: Reward;
  saved_reward: &Reward;
}
```

`reward` 可以保存内联 `ItemReward { ... }`，`saved_reward` 可以引用实际类型为 `ItemReward` 的顶层记录。

反方向不成立：父类型值不能赋给要求具体子类型的位置。抽象类型不能直接实例化，必须使用具体子类型。

同一继承体系中的 record key 必须唯一。没有继承关系的类型可以使用相同 key，因为引用的目标类型能够区分它们。

## 缺省字段、默认值与 null

字段没有出现在数据源中时：

- 字段声明了 CFT 默认值：使用默认值；
- 字段没有默认值：报告 `CFD-DATA-006`，即使它是 nullable 也不能省略。

nullable 只表示该字段可以**显式填写** `null`。如果希望字段既可为 null 又可省略，应声明默认值：

```cft
type Node {
  next: &Node? = null;
}
```

默认值会递归应用到内联对象。每一层对象中没有默认值的必填字段都必须由数据源提供。

## 记录顺序

同一类型的记录保持稳定顺序：

1. `coflow.yaml` 中 source 的顺序；
2. 表格 source 中 sheet 的顺序；
3. sheet 中的行顺序；
4. CFD 文件中记录的出现顺序。

这个顺序会反映在数据查询、编辑器列表和导出结果中。

## Spread

CFD 的 `...source` 会先复制来源对象或字典的值，再应用本地覆盖：

```cfd
elite_monster: Monster {
  ...&basic_monster,
  name: "Elite Training Dummy",
}
```

对象 spread 可以使用顶层 record 引用或内联对象，来源类型必须能赋值给当前对象类型。字典 spread 使用内联字典，其中的 key 和 value 必须符合当前字典类型。

多个 spread 按出现顺序合并，后面的 spread 覆盖前面的 spread，本地字段或字典条目拥有最高优先级。循环依赖会被拒绝。

## Singleton

`@singleton` 类型在整个数据集中必须恰好有一条记录：

```cft
@singleton
type GameConfig {
  max_level: int;
}
```

singleton 记录仍然需要 record key。不同 singleton 类型不能使用相同 key。singleton 类型不能作为普通内联对象或记录引用字段使用。

## 维度值

`@localized` 和 `@dimension("name")` 字段可以针对不同维度变体提供值。普通数据源中的字段值是默认值，维度文件保存各个变体的覆盖值。

维度变体缺失和显式 `null` 含义不同：

- 缺失：这个变体没有提供值；
- `null`：这个变体明确提供了空值，字段必须是 nullable。

维度值使用与默认值相同的字段类型、记录引用和校验规则。详见 [本地化与维度](./10-localization.md)。

## 数据源一致性

所有数据源遵循相同规则：

- 字段必须存在于 CFT 类型中；
- 值必须符合字段类型；
- 默认值和必填字段行为一致；
- enum、字典 key 和多态类型按同一规则校验；
- 记录引用可以跨数据源解析；
- record key 唯一性和 `@singleton` 约束对整个项目生效。

因此，一条 CFD 记录可以引用 Excel 中的记录，CSV 和 Excel 中相同类型的记录也会进入同一个逻辑数据集合。
