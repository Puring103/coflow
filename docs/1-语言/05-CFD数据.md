# CFD 数据

> 状态：语言设计
>
> 范围：CFD 的记录、字段、值和引用语法。

CFD 是项目唯一的数据输入格式。解析器先生成带 source span 的无 schema 语法树，再依据 CFT 契约完成
类型转换、默认值、继承、引用、维度覆盖和业务 check。

## 1. 文件结构

```text
cfd-file    := { record-decl }
record-decl := record-key ":" qualified-name "{" [ field-list ] "}"
field-list  := field { "," field } [ "," ]
field       := identifier ":" value
```

```cfd
sword: Item {
  name: "Fire Sword",
  rarity: Rare,
  tags: [weapon, fire],
}
```

## 2. 顶层记录

每个记录由 record key、实际 type 和字段块组成：

```cfd
sword: Item {
  name: "Sword",
}
```

```cfd
shield: Equipment {
  name: "Shield",
}
```

```cfd
hit: DamageEffect {
  label: "Hit",
  amount: 10,
}
```

- 类型引用使用与 CFT 相同的命名空间解析规则，允许短名或完整限定名。
- record key 使用普通标识符，必须是其查询类型域中唯一的非保留标识符；命名空间不改变 record key。

## 3. 字段与省略

```text
field := identifier ":" value
```

- 字段顺序不改变 schema 语义，但来源顺序和 source identity 会稳定保留。
- 字段值只能是结构化字面量：scalar、字符串、enum、集合、内联对象、引用、`None` 和格式化字符串；完整文法见 [03-类型与值](./03-类型与值.md) §9。
- 普通数据字段不包含表达式、算术、控制流或函数调用。
- 省略字段时，使用 CFT 默认值。
- 没有默认值的必填字段不能省略。
- CFD 不能声明 type、默认值或 check。

```cfd
starter: Item {
  name: "Starter",
  enabled: true,
}
```

## 4. scalar 与字符串

```text
scalar := int-literal | float-literal | "true" | "false" | enum-variant
```

```cfd
count: 10,
ratio: 0.25,
enabled: true,
name: "line 1\nline 2",
rarity: Rare,
```

- bool 只接受小写 `true`、`false`。
- 字符串使用双引号，支持 `\"`、`\\`、`\n`、`\r`、`\t`。

## 5. flag enum 位表达式

`@flag` enum 可用 `|`、`^`、`&` 和括号组成位表达式：

```text
bit-expr := bit-term { ( "|" | "^" | "&" ) bit-term }
bit-term := enum-variant | "(" bit-expr ")"
```

```cfd
permissions: Read | Write,
mask: (Read | Write) & Execute,
```

## 6. 数组、字典与内联对象

```text
array := "[" [ value { "," value } [ "," ] ] "]"
dict  := "{" [ dict-entry { "," dict-entry } [ "," ] ] "}"
block := qualified-name "{" [ field-list ] "}"
```

```cfd
tags: [weapon, rare],
weights: {
  Fire: 10,
  Ice: 5,
},
stats: Stats {
  hp: 100,
  speed: 1.5,
},
effect: DamageEffect {
  amount: 20,
},
```

- `{ ... }` 总是字典字面量；内联对象必须写类型名 `TypeName { ... }`。
- 抽象父 type 字段必须写具体子 type，即 `ConcreteType { ... }`。
- 数组保留顺序，字典 key 必须符合声明的 key 类型且不能重复。

## 7. 可选值

```text
optional-value := "None" | value
```

可选类型的规范写法是 `None` 或裸存在值：

```cfd
subtitle: None,
owner: &sword,
label: "present",
```

- 省略字段时使用 CFT 默认值。
- 显式 `None` 表示明确为空，即使字段默认值不是 `None`。
- 没有默认值的可选字段省略时为 `None`。
- 结构化 writer 写回裸值。

## 8. 记录引用

```text
reference := "&" [ qualified-name "::" ] record-key
```

```cfd
owner: &sword,
fallback: &Item::default_item,
```

- `&key` 根据字段声明的 `&Type` 解析；需要显式写出目标类型时可写 `&Type::key`。
- `&key` 在声明的 `&Type` 及其子类型的记录域内查找；唯一匹配才有效，多个匹配报引用歧义并要求写 `&Type::key`。
- 引用可跨 CFD 文件，也允许自引用和跨记录循环。
- 目标必须存在，且实际 type 可赋给声明的引用类型。
- 引用不是字符串，不能使用引号。

## 9. 格式化字符串

```text
formatted-string := '"' { string-char | escape | "{{" | "}}" | "{" field-path "}" } '"'
field-path       := [ qualified-name "::" ] [ record-key "." ] identifier { "." identifier }
```

```cfd
label: "{name} x {count}",
remote_label: "{&sword.name}",
typed: "{&Item::sword.name}",
literal: "{{not interpolation}}",
```

插值引用形式为 `{field}`、`{&key.field}` 或 `{&Type::key.field}`；`{{` 和 `}}` 表示字面花括号。
解析完成后根据记录和字段路径求值，同时保留原始 source。

## 10. 函数值

```text
function := "fn" "(" [ param { "," param } [ "," ] ] ")" "->" type block
```

```cfd
calculator: Calculator {
  classify: fn(value: int) -> string {
    if value >= 10 {
      "large"
    } else {
      "small"
    }
  },
}
```

- 函数签名必须与 CFT 字段类型一致。
- CFD 不声明字段类型，函数值的完整签名是其参数名与形状的唯一来源，因此必须写签名；CFT 的 `=>` 能省略签名是因为字段类型已给出签名。
- 函数字段可以在 CFT 中声明默认实现，CFD 中的显式值覆盖它。
- `@Host` 服务函数由宿主配置，CFD 不能实现；CFT 可声明默认实现，被宿主绑定覆盖。
- 函数体是受静态类型约束的表达式语言，见 [06-函数与表达式](./06-函数与表达式.md)。
