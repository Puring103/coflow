# CFT 声明

## 文件与声明

项目中的 CFT 文件共同组成全局声明集合。声明包括 type、enum、const、类型别名和命名的顶层 check；名称在整个项目中唯一。类型来源文件用于诊断和源码导航。

```text
cft-file := { annotation | declaration }
type-decl := { annotation } [ "abstract" | "sealed" ] "type" identifier
             [ ":" identifier ] "{" { field } [ check-body ] "}"
field := { annotation } identifier ":" type [ "=" value ] ";"
alias := "type" identifier "=" type ";"
const-decl := "const" identifier [ ":" type ] "=" value ";"
```

## 类型与继承

普通 type 可以用于 CFD 顶层记录和内联值。字段使用 `TypeName` 表示内联对象，使用 `&TypeName` 表示记录引用。

类型采用单继承。abstract 类型通过具体子类型实例化；sealed 类型不能继续派生。继承链中字段不得重名，父子检查规则保留。类型中的匿名 check 位于全部字段之后。

```cft
abstract type Entity { name: string; }
type Item: Entity { price: int = 100; }
@singleton type Settings { start: &Item; }
@struct sealed type Point { x: float; y: float; }
```

`@singleton` 类型恰好提供一条记录，key 为声明名称；单例类型不能用作内联值或记录引用字段。`@struct` 要求 sealed，控制目标代码的内联值表示，记录身份与引用语义保持独立。

必填对象循环、继承循环和不能有限展开的默认值在声明阶段生成诊断。有限的 Option、数组、字典递归和记录引用见[类型与值](./03-类型与值.md)。

## enum 与 flag

```cft
enum Quality { Common, Rare = 10, Epic }
@flag enum Permission { Zero = 0, Read = 1, Write = 2, Execute = 4 }
```

成员值为有符号 64 位整数，未指定值时从前一个成员递增。成员名称、数值不能重复，自动递增越界报错。不同 enum 类型相互独立。

`@flag` 成员允许零或正的二次幂。数据值可以通过同类型成员的 `&`、`^`、`|` 和括号组合，优先级依次为 `&`、`^`、`|`。

## 默认值、常量与别名

默认值可以包含标量、Option、集合、内联对象、枚举、记录引用和 fstring。默认引用在数据加载时链接。常量依赖在声明阶段解析；fstring 作为字段默认值保留所属对象，常量不包含 fstring。

```cft
const LIMIT = 100;
const EMPTY: Option<int> = None;
type ItemLink = Option<&Item>;
type Item {
  count: int = LIMIT;
  links: [ItemLink] = [];
}
```

None、空数组和空字典需要所在位置的明确类型。别名与常量依赖必须无环。CFD 可以使用已声明常量作为字段值。

## 注解

| 注解 | 目标与含义 |
| --- | --- |
| @label("文本") | 类型、枚举、成员、字段的显示名称 |
| @description("文本") | 类型、枚举、成员、字段的说明 |
| @singleton | 单例 type |
| @struct | sealed type 的内联结构体表示 |
| @flag | enum 的位掩码成员约束 |
| @idAsEnum(Name) | 为 type 的记录 key 生成目标语言 enum；Name 为已声明的空 enum |
| @localized | 字段绑定 language 维度 |
| @dimension("名称") | 字段绑定指定维度 |
| @expand | 具体内联对象字段的展示展开信息 |

同一字段只能绑定一个维度。注解名称、参数个数、参数类型和目标位置均参与声明检查。`@singleton` 和 `@idAsEnum` 不能同时应用于同一类型。

检查规则的结构与执行边界见[Check 校验](./07-Check校验.md)。
