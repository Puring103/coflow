# CFD 文本数据语言

CFD（Coflow Data）是项目唯一的数据输入格式。解析器先生成带 source span 的无 schema 语法树，runtime
再根据 CFT 完成类型转换、默认值、继承、引用、维度 overlay 和业务 check。

```cfd
sword: Item {
  name: "Fire Sword",
  rarity: Rare,
  tags: [weapon, fire],
}
```

空白不参与语义，注释从 `#` 延续到行尾。字段和集合项使用逗号分隔并允许尾逗号。项目统一使用 2 空格
缩进，可通过 `coflow format` 规范化排版。

## 文件与名称

项目中的 CFD 文件共同提供配置记录。文件和目录只用于组织数据，不创建名称作用域。记录声明和内联
对象的类型直接使用 CFT 中项目全局唯一的短名：

```cfd
sword: Item {
  rarity: Rarity::Rare,
}
```

`::` 用于枚举成员和带类型的记录引用，例如 `Rarity::Rare` 与 `&Item::sword`，不用于限定类型名。

## 顶层记录

普通记录由 record key、实际 type 和字段块组成：

```cfd
sword: Item {
  name: "Sword",
}
```

同类型记录可以放入 group，省略每条记录重复的 type：

```cfd
Item {
  sword {
    name: "Sword",
  }

  shield {
    name: "Shield",
  }
}
```

group 内也可用 `key: DerivedType { ... }` 指定具体派生 type。group 只是文本简写，不创建额外的数据
层级。record key 必须是非保留的 CFT 标识符，并在其查询类型域中唯一。

## 字段与省略

字段写作 `name: value`。字段顺序不改变 schema 语义，但来源顺序和 source identity 会稳定保留。省略字段
时，runtime 使用 CFT 默认值；没有默认值的必填字段不能省略。未知字段和重复字段会产生诊断。

```cfd
starter: Item {
  name: "Starter",
  enabled: true,
}
```

CFD 不能声明 type、默认值或 check，也不允许在数据文件中写 `check { ... }`。

## 值语法

### Scalar、字符串和 enum

```cfd
count: 10,
ratio: 0.25,
enabled: true,
name: "line 1\nline 2",
rarity: Rare,
permissions: Read | Write,
```

bool 只接受小写 `true`、`false`。字符串使用双引号，支持 `\"`、`\\`、`\n`、`\r`、`\t`。
普通 enum 使用变体名称；`@flag` enum 可用 `|`、`^`、`&` 和括号组成位表达式。

### 数组、字典与内联对象

```cfd
tags: [weapon, rare],
weights: {
  Fire: 10,
  Ice: 5,
},
stats: {
  hp: 100,
  speed: 1.5,
},
effect: DamageEffect {
  amount: 20,
},
```

`{ key: value }` 由字段的 CFT 类型决定是字典还是内联对象。抽象父 type 字段必须使用
`ConcreteType { ... }` type marker 指定实际子 type。数组保留顺序，字典 key 必须符合声明的 key 类型且
不能重复。

### Option 与 Result

```cfd
subtitle: None,
owner: &sword,
cached: Ok(10),
failure: Err("not found"),
```

Option 的规范写法是 `None` 或不带包装的存在值；解析器也接受显式 `Some(value)`。Coflow 编辑器和
结构化 writer 会把 `Some(value)` 写回为 `value`。Result 使用显式 `Ok(value)` / `Err(value)`。

### 记录引用

```cfd
owner: &sword,
fallback: &Item::default_item,
```

`&key` 根据字段声明的 `&Type` 解析；需要显式写出目标类型时可写 `&Type::key`。
引用可跨 CFD 文件，但目标必须存在且实际 type 可赋给声明的引用类型。引用不是字符串，不能使用引号。

### 格式化字符串

普通字符串包含有效字段引用时会自动识别为格式化字符串，不存在单独的字符串前缀：

```cfd
label: "{name} x {count}",
remote_label: "{&sword.name}",
typed: "{&Item::sword.name}",
```

插值引用形式为 `{field}`、`{&key.field}` 或 `{&Type::key.field}`。`{{` 和 `}}` 表示字面花括号。
解析完成后 runtime 根据记录和字段路径求值，同时保留原始 source。

### 函数值

函数类型字段可以在 CFD 中提供与 CFT 声明一致的签名和函数体：

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

函数体是受静态类型约束的表达式语言。函数签名必须与 CFT 字段类型一致；函数字段也可以在 CFT 中
声明默认实现，CFD 中的显式值会覆盖它。`@Host` 服务函数由宿主配置，不能在 CFT 中声明默认实现，也
不能在 CFD 中实现。C# Runtime 的 `Load` 只保留函数值，`LoadAndCompile` 才类型检查并编译函数体。
Rust runtime 当前不执行函数。当前 C# Runtime 的 VM 尚未提供执行预算，函数只应来自受信任、可控的
配置源。

## 文件发现与检查

`coflow.yaml` 的 `data` 可列出 `.cfd` 文件或目录；目录递归发现小写 `.cfd` 扩展名，其他文件不参与
数据模型。多个来源按稳定路径顺序加载。维度输出仍为 CFD，但其覆盖位置由 CFT 注解和项目 dimensions
配置共同决定。

`coflow check` 会报告语法错误、未知或重复字段、类型不匹配、缺少必填字段、重复 record key、悬空或
错误类型引用以及 check 失败。`coflow format` 只格式化配置的文本，不替代这些检查。
