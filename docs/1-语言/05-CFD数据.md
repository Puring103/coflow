# CFD 数据

> 范围：基础记录、结构化字段值、引用和维度覆盖。

## 1. 文件与记录

```text
cfd-file := { use-decl } { record-decl | dimension-block }
record-decl := record-key ":" qualified-name "{" [ field-list ] "}"
record-key := identifier
field-list := field { "," field } [ "," ]
field := identifier ":" value
dimension-block := identifier "[" identifier "]" "{" { record-override } "}"
record-override := record-key ":" qualified-name "{" [ field-list ] "}"
```

CFD 可以导入名称，基础记录的类型必须为 table 或 singleton，data 用于内联字段值。
CFD 不声明 namespace、类型、字段默认值或 check。
记录 key 在整棵继承树内唯一；无共同父类型的独立类型允许同 key。
类型名使用完整限定名或显式导入的短名。

```cfd
use game::Item;

sword: Item {
  name: "Sword",
  price: 100,
}

language[ja] {
  sword: Item {
    name: "剣",
  }
}
```

`:` 与 `[` 直接区分基础记录和覆盖块，不依赖声明名称或文件名。
字段及集合项以逗号分隔，允许尾逗号；记录和覆盖块之间不使用逗号。
加载时完成类型检查、默认值补齐、维度覆盖与引用链接，check 单独执行。

## 2. 字段值

值文法见[类型与值](./03-类型与值.md)。数据值包括数字、布尔、字符串、模板、
enum、集合、内联对象、记录引用、常量和 None。

```cfd
item: Item {
  name: "Sword",
  label: f"名称：{self.name}",
  price: 100,
  tags: ["weapon", "fire"],
  next: None,
}
```

省略基础字段时使用声明默认值；无默认值的可选字段为 None，其余字段必填。
显式 None 仅用于可选字段。字段与字典 key 不能重复。
fstring 使用模板字面量或可访问的模板常量初始化，插值仅使用字段路径。

## 3. 集合与对象

```cfd
tags: ["weapon", "rare"],
weights: { "fire": 10, "ice": 5 },
stats: Stats { hp: 100, speed: 1.5 },
effect: DamageEffect { label: "Hit", amount: 20 },
```

数组保留元素顺序，字典保留原始 CFT/CFD 的键值项顺序。
`{ ... }` 是字典；内联对象必须写 `DataName { ... }`，类型必须是具体 data。
table 和 singleton 通过记录引用使用。读取集合中的具体 fstring 元素时生成文本。

## 4. enum 与 flag

enum 使用变体名或限定名。flag 支持 `&`、`^`、`|` 和括号，
优先级为 `&` 高于 `^` 高于 `|`，操作数必须属于同一 flag 类型。

```cfd
permissions: Permission::Read | Permission::Write,
```

## 5. 记录引用

```text
reference-value := "&" [ qualified-name "::" ] record-key
```

- `&key` 按源码所在的静态本类型查找记录；其他类型使用 `&Type::key`。
- 不按字段期望类型猜测目标；无本类型上下文时使用带类型写法。
- 显式类型采用普通名称解析，查找该类型及其子类型。
- 目标记录必须存在，实际类型必须可赋给字段声明类型。
- 引用可以跨文件、自引用或形成跨记录循环，不是字符串。

```cfd
sword: Item {
  name: "Sword",
  next: &shield,
  owner: &Character::hero,
}
```

覆盖记录体使用头部指定的记录类型作为静态本类型，基础与覆盖中的引用遵循相同规则。
字段路径插值也可使用记录引用，详见[类型与值](./03-类型与值.md)。

## 6. 模板与维度覆盖

普通 string 中的花括号是文本。fstring 的插值读取 self 或指定记录的字段路径，
基础值直接转为文本；源码保存原模板，读取与预览生成 string。

维度覆盖只写需要替换的字段，省略字段使用基础值，对象和集合按整个字段替换。
基础记录与覆盖块可分文件管理；维度声明、重复覆盖检查、回退和编辑来源见[维度](./09-维度.md)。
