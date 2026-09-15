# CFT 声明

> 状态：语言设计
>
> 范围：类型、枚举、字段、默认值、注解和 check 声明。

## 1. 文件结构

```text
cft-file := [ namespace-decl ] { use-decl } { declaration }
declaration := { annotation } (enum-decl | table-decl | singleton-decl | data-decl
                            | alias-decl | const-decl | check-decl)
alias-decl := "type" identifier "=" type ";"
const-decl := "const" identifier ":" type "=" value ";"
annotation := "@" identifier [ "(" [ annotation-arg { "," annotation-arg } [ "," ] ] ")" ]
annotation-arg := string-literal | qualified-name
```

CFT 声明类型与规则，不保存 CFD 记录。
命名空间和导入见[命名空间](./08-命名空间.md)。

## 2. enum 与 flag

```text
enum-decl := "enum" identifier "{" [ variant { "," variant } [ "," ] ] "}"
variant := { annotation } identifier [ "=" int-literal ]
```

普通 enum 使用非负 i32 值。未指定时从 0 开始，按前一个值加一分配。
变体名和值均不能重复，自动分配越界时报错。不同 enum 互不兼容。

`@flag` 使用完整的 32 位无符号位模式：

- 允许 bit 0—31，最多 32 个标志。
- 非零变体值必须是单独一个位；显式 0 可以声明零值变体。
- 自动分配取下一个未使用的 2 的幂。
- 位运算只作用于同一个 flag 类型；结果仍为该类型。
- `~` 只翻转该类型已经声明的标志位。
- 构造值不能包含未声明的位。

```cft
@flag
enum Permission {
  Read = 1,
  Write = 2,
  High = 2147483648,
}
```

最高位通过变体声明和 flag 位运算使用，不改变普通 int 的范围。

## 3. table、singleton、data 与继承

```text
table-decl := [ "abstract" | "sealed" ] "table" identifier [ ":" qualified-name ]
              "{" { record-member } "}"
singleton-decl := "singleton" identifier [ ":" qualified-name ]
                  "{" { record-member } "}"
data-decl := [ "abstract" | "sealed" ] "data" identifier [ ":" qualified-name ]
             "{" { field-decl } "}"
record-member := field-decl | check-block
```

- table 声明多记录类型；singleton 声明单例记录类型；data 声明普通数据类型，用于字段和内联值。
- table 只能继承 table，data 只能继承 data，singleton 可以继承 table。
- 记录类型与 data 之间不能互相继承或转换用途。记录不能内联构造，data 不能作为顶层记录。
- abstract 在记录、默认值和函数体中都不能直接构造；使用其具体子类型。
- sealed table 和 sealed data 不能继续派生；singleton 是具体的封闭记录类型，不能作为父类型。
- 每个类型最多有一个直接父类型，继承关系不能成环。
- 子类型继承父字段，不能重新声明父字段，也不能修改其声明类型或默认值。记录子类型同时继承父 check。
- 同一记录类型及其继承链中，字段和命名 check 共用成员名称，禁止同名；data 字段在继承链中也不能重名。
- 记录父子 check 都保留，按父到子执行。每个记录类型最多有一个匿名 check；data 不声明 check。
- `@struct` 只能用于 sealed data，不参与继承；字段类型不受额外限制。
- data 的内容递归必须经过可选类型；数组和字典本身不能打断递归。记录引用连接身份，允许成环。
- 默认值展开必须有限，可选递归也不能形成无限默认值物化。

singleton 的唯一记录 key 为类型短名。普通 singleton 必须恰好提供一条这样的记录，
其父 table 仍可有其他记录；单例约束只作用于该 singleton。
同一记录继承树中的 key 仍须唯一，详见[命名空间](./08-命名空间.md#5-引用与-record-key)。
`@Host singleton` 的记录由宿主绑定提供，缺失绑定按 Host 规则延迟到实际使用时报告。

```cft
abstract table Entity {
  name: string;
}

table Item : Entity {
  price: int;
}

singleton Settings : Entity {
  version: string;
}

abstract data Effect {
  label: string;
}

sealed data DamageEffect : Effect {
  amount: int;
}

@struct
sealed data Point {
  x: int;
  y: int;
}
```

## 4. 字段与默认值

```text
field-decl := { annotation } identifier ":" type [ "=" value | "=>" block ] ";"
```

提供字段值时使用该值；省略时使用默认值；没有默认值的可选字段为 None，其余字段必填。
显式 None 只适用于可选字段。普通值和 fstring 的差别见[类型与值](./03-类型与值.md)。

```cft
table Item {
  name: string = "Unknown";
  label: fstring = f"名称：{self.name}";
  price: int = 10;
  tags: [string] = [];
  next: Item? = None;

  total: fn(count: int) -> int => {
    self.price * count
  };
}
```

默认值可以包含集合、对象、引用、可选值、函数和 fstring。
函数可以放在任何类型允许的位置，不因嵌套而受限。
fstring 初始化使用 `f"..."` 字面量或已有文本模板；普通数据位置可复用 fstring 常量。
模板传递遵循[类型与值](./03-类型与值.md#54-模板传递与返回)中的期望类型规则。

`&key` 按声明所在的静态本类型查找记录，其他类型写 `&Type::key`。
继承字段的默认值及默认函数保持父类型的查找上下文，不随运行时 self 的实际类型改变。
默认值中的记录引用在加载具体数据时解析，不在契约中保存运行时记录地址。

## 5. 函数字段

`=>` 是函数字段默认实现的简写，参数名取自字段签名。
使用该简写时签名必须为所有参数命名；完整写法为 `= fn(...) -> R { ... }`。

- 默认实现必须符合字段签名，参数名不影响签名相等。
- CFD 提供的实现替代该对象的默认实现，契约本身保持只读。
- 函数中的 self 指向所属对象，只读。
- 取出函数字段后仍记住原对象，不因存入其他位置而重新绑定。
- 父类型默认实现按声明类型检查，执行时使用实际对象的数据和最终函数实现。

字段集合中的函数字面量也绑定字段所属对象；内联对象自身字段的函数字面量绑定该内联对象。
完整绑定规则见[函数与表达式](./06-函数与表达式.md#11-创建位置与绑定)。

## 6. 注解

同一目标不能重复使用同名注解。自定义注解作为契约元数据保存。

| 注解 | 目标 | 作用 |
| --- | --- | --- |
| @label("...") | 对象类型、enum、变体、字段 | 显示名称和生成代码注释 |
| @description("...") | 对象类型、enum、变体、字段 | 编辑说明和生成代码注释 |
| @flag | enum | 32 位标志集合 |
| @struct | sealed data | 目标代码使用值类型表示 |
| @Host | singleton | 声明由宿主整对象绑定的服务，数据和函数由宿主提供 |
| @idAsEnum(Name) | table | 仅在目标代码中为记录 key 生成 enum |
| @localized | 顶层记录字段 | 绑定 language 维度 |
| @dimension("name") | 顶层记录字段 | 绑定指定维度 |

普通 singleton 没有记录、记录数量不为一或 key 不正确都属于加载错误。

@Host 单例由宿主绑定提供，允许暂缺绑定，不要求 CFD 提供记录。
实际调用未绑定服务的函数或读取其宿主数据时报执行错误。
Host 函数字段只声明签名，实现由宿主提供，具体规则见[Host 函数](./12-Host函数.md)。

`@idAsEnum(Name)` 中的 Name 引用空 enum 声明，作为代码生成目标。
生成器根据记录 key 输出目标语言成员，不向 CFT enum 回填成员，不改变契约、记录加载或 id 类型。
该注解用于 table，singleton 和 data 不使用该注解。

```cft
singleton Settings {
  version: string;
}

enum ItemId {}

@idAsEnum(ItemId)
table CatalogItem {
  name: string;
}
```

`@localized` 与 `@dimension` 不能同时修饰同一字段。
维度字段和生成记录见[维度](./09-维度.md)。

## 7. check

```text
check-block := "check" [ identifier ] block
check-decl := "check" identifier block
```

table 和 singleton 内的 check 位于所有字段之后；data 不声明 check。顶层 check 必须命名。
check 是可选的特殊函数，可以调用普通函数。执行和报告规则见[Check 校验](./07-Check校验.md)。
