# CFD

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

独立记录使用 `id: Type { ... }`，分组使用 `Type { id { ... } }`。分组内显式提供的实际类型必须属于该分组类型的继承域。记录 ID 使用语言标识符，在整棵类型继承树内唯一。

singleton 的唯一记录 ID 为声明名称。字段之间使用逗号，可以保留末尾逗号；`#` 开始行注释。重复记录 ID、重复字段、多余字段、类型错误和未知类型均产生诊断。

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
| 枚举、flag | `Quality::Rare`、`Read \| Write` |
| 常量 | CFT 中已声明的全局常量名 |

数字按声明类型验证：`int` / `long` 为有符号 32 / 64 位整数，`float` / `double` 为有限 binary32 / binary64 浮点数；`float` 按 32 位精度舍入。Option 可以嵌套并保留每层状态；裸值在可选字段中表示一层 Some。省略字段仅按显式默认值补全；没有默认值的字段缺失时产生诊断，包括 Option 字段。

引用在所有记录身份建立后连接，可跨文件和成环。普通字符串中的花括号保持原文，字符串模板 在加载时按字段路径生成文本，路径末端必须是可转为文本的基础值。

## 覆盖

`dimension[variant]` 覆盖已有记录的维度字段，只替换明确提供的完整字段值。未覆盖字段使用基础值，显式 None 保持为空。同一维度、变体、记录、字段重复覆盖时报错，文件顺序不提供覆盖优先级。

维度的选择与读取见 [维度与本地化](../10-localization.md)。

## 记录类型与身份

| CFT 声明 | CFD 写法 | 身份与使用方式 |
| --- | --- | --- |
| `table Item` | `sword: Item { ... }` | 一张表可以包含多条记录，通过 ID 查询或引用 |
| `singleton Settings` | `Settings: Settings { ... }` | 恰好一条记录，ID 与类型名相同 |
| `data Stats` | 字段值 `Stats { ... }` | 内联对象，通过所属字段访问 |

独立记录和类型分组可以混用，分组中也可以指定派生类型：

```cfd
Item {
  sword { name: "铁剑", price: 100 },
  bow: RangedItem { name: "长弓", price: 120, range: 10 },
}
Settings: Settings { start: &Item::sword }
```

这里要求 CFT 声明 `Item`、继承自 Item 的 `RangedItem` 和带有 `start: &Item` 字段的 Settings。抽象类型可用作分组类型，但记录实际类型必须是可实例化的具体类型。ID 在同一继承树内唯一；互不相关的类型树可以使用相同 ID。

## 数值、集合与对象

| 声明类型 | 范围或约束 |
| --- | --- |
| `int` | -2147483648 至 2147483647 |
| `long` | -9223372036854775808 至 9223372036854775807 |
| `float` | 有限 32 位浮点数，写入时按 binary32 精度舍入 |
| `double` | 有限 64 位浮点数 |
| `bool` | `true` 或 `false` |
| `string` | 普通字符串或显式插值模板 |
| `[T]` | 元素按 T 验证，保留顺序，可为空 |
| `{K: V}` | K 为 int、long、string 或 enum，键不能重复 |

数字支持小数和指数形式，如 `1.5`、`-2.5e-3`，范围按目标类型检查。枚举使用成员名或 `Enum::Member`，位枚举可以用同类型成员的 `&`、`^`、`|` 与括号组合，优先级从高到低为 `&`、`^`、`|`。

```cfd
example: Example {
  scores: [10, 20, 30],
  labels: { "title": "物品", "action": "购买" },
  stats: Stats { hp: 100 },
  permissions: Permission::Read | Permission::Write,
}
```

对象实际类型可为字段声明类型或其具体子类型。声明类型足以确定对象类型时可省略 `Stats`；需要实例化子类型时写出实际类型。每个对象内部也遵循必填字段与默认值规则。

## 默认值与可选值

| 字段声明 | 可否省略 | CFD 可写值 |
| --- | --- | --- |
| `count: int;` | 否 | 例如 `3` |
| `count: int = 0;` | 是，补为 0 | 例如 `3` |
| `note: Option<string>;` | 否 | `None`、`Some("备注")` 或 `"备注"` |
| `note: Option<string> = None;` | 是，补为 None | 同上 |
| `nested: Option<Option<int>>;` | 否 | `None`、`Some(None)`、`Some(Some(3))` |

省略、显式 None 和空字符串有不同含义。对象、集合及 Option 中的默认值递归补全；可选值的每一层状态独立保留。只有默认值可以让字段省略。

## 引用与数据模型

`&Item::sword` 按类型与 ID 定位记录，`&sword` 根据所在字段的目标类型查找。引用目标必须存在，并满足字段的类型约束；父类型引用可以连接其子类型记录。

记录身份先建立，再连接引用，因此文件之间可以互相引用，记录引用也可以成环。加载时一起提供所有依赖的 CFD。引用共享同一个记录身份，内联 data 则属于当前对象的字段值。

数组按顺序比较，字典按键和值比较，内联对象按实际类型与字段内容比较，Option 按每层状态与内容比较。引用按记录身份比较，不展开目标记录。

## 字符串与模板

普通字符串中的花括号保留原文；只有 `f"..."` 执行插值。<code v-pre>{{</code> 和 <code v-pre>}}</code> 在模板中表示文本花括号。

```cfd
sword: Item {
  name: "铁剑",
  price: 100,
  label: f"{self.name}：{self.price}",
}
shield: Item {
  name: "盾牌",
  price: 80,
  label: f"搭配 {&Item::sword.name}",
}
```

示例要求 Item 声明 `label: string`。`self` 指模板所属对象，嵌套 data 内的模板使用该内联对象。模板读取字段路径，末端为 string、int、long、float、double、bool 或 enum；不能直接把对象、集合或 Option 转为文本。计算与方法调用写在 CFT 校验表达式中。

模板按当前选择的维度读取依赖，缺少覆盖时重新求值基础模板。路径错误、读取错误和循环依赖产生来源诊断。应用读取到的是求值后的字符串。

## 维度块与替换范围

```cfd
language[ja] {
  sword: Item { name: "剣" },
}
```

目标记录必须已经存在，且 `name` 必须通过 CFT 绑定到 language 维度。只填写需要覆盖的顶层字段；其他顶层字段使用基础表达式。覆盖对象、数组或字典时替换整个字段，不逐项合并；覆盖内联对象内部仍补全其声明默认值。

空覆盖块也声明变体。全部输入文件中的块共同决定已知变体，同一字段不能在同一变体中重复赋值。完整的文件组织和读取方式见[本地化与维度](../10-localization.md)。

## 文件语法

CFD 使用 UTF-8。标识符采用 Unicode XID，首字符为 `_` 或 XID_Start，后续为 `_` 或 XID_Continue，区分大小写；保留名称见 [CFT 文件与词法](./01-cft.md#文件与词法)。字符串支持 `\n`、`\r`、`\t`、`\"`、`\\` 转义，Unicode 字符直接输入。

```text
独立记录： ID: 类型 { 字段: 值, … }
类型分组： 类型 { ID [: 实际类型] { 字段: 值, … }, … }
维度覆盖： 维度[变体] { 记录或类型分组… }
内联对象： [实际类型] { 字段: 值, … }
数组：     [值, …]
字典：     {键: 值, …}
可选值：   None | Some(值)
引用：     &ID | &类型::ID
```

字段、集合元素和字典项使用逗号分隔，允许末尾逗号，`#` 开始行注释。通过 `coflow format` 统一格式，使用 `coflow check` 验证声明、值和引用，使用 `coflow check --rules` 同时执行业务校验。错误结构与完整错误码见[诊断](../09-diagnostics/01-diagnostics.md)。
