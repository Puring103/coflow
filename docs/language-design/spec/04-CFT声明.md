# CFT 声明

## 文件与声明

项目中的 CFT 文件共同组成全局声明集合。声明包括 table、data、singleton、enum、const、类型别名和命名的顶层 check；名称在整个项目中唯一。类型来源文件用于诊断和源码导航。

```text
cft-file := { declaration }
type-decl := { annotation } [ "abstract" | "sealed" ] ("table" | "data" | "singleton") identifier
             [ ":" identifier ] "{" { field } { "check" identifier check-body } "}"
field := { annotation } identifier ":" type [ "=" value ] ";"
alias := "type" identifier "=" type ";"
const-decl := "const" identifier [ ":" type ] "=" value ";"
```

## 类型与继承

table 是多记录类型，singleton 是唯一配置，data 是内联值类型。字段使用 `&TableName` 声明记录引用，使用 data 名称声明内联值。

类型采用单继承：data 继承 data，table 与 singleton 继承 table。abstract 声明通过具体子类型实例化，sealed 与 singleton 不可继续派生。字段在继承链中不得重名。table、singleton 和 data 可以在字段后声明多个具名 check。规则在类型及继承链中不得重名，先执行基类规则，再按派生类型源码声明顺序执行。data 规则绑定实际内联实例，集合、Option 和嵌套对象中的实例分别定位，引用读取不重复创建实例任务。

```cft
abstract table Entity { name: string; }
table Item: Entity { price: int = 100; }
singleton Settings { start: &Item; }
@struct sealed data Point { x: float; y: float; }
```

singleton 恰好提供一条以声明名称为 id 的记录，不能用于字段。@struct 只适用于没有基类的 sealed data，目标使用结构体；table 与 singleton 使用 class。

必填对象循环、继承循环和不能有限展开的默认值在声明阶段生成诊断。有限的 Option、数组、字典递归和记录引用见[类型与值](03-类型与值.md)。

## enum 与 flag

```cft
enum Quality { Common, Rare = 10, Epic }
@flag enum Permission { Zero = 0, Read = 1, Write = 2, Execute = 4 }
```

成员值为有符号 64 位整数，未指定值时从前一个成员递增。成员名称、数值不能重复，自动递增越界报错。不同 enum 类型相互独立。

`@flag` 成员允许零或正的二次幂。数据值可以通过同类型成员的 `&`、`^`、`|` 和括号组合，优先级依次为 `&`、`^`、`|`。

## 默认值、常量与别名

默认值可以包含标量、Option、集合、内联对象、枚举、记录引用和插值模板。默认引用在数据加载时链接。常量依赖在声明阶段解析；插值模板作为字段默认值保留所属对象，常量不包含插值模板。

```cft
const LIMIT = 100;
const EMPTY: Option<int> = None;
type ItemLink = Option<&Item>;
table Item {
  count: int = LIMIT;
  links: [ItemLink] = [];
}
```

None、空数组和空字典需要所在位置的明确类型。别名与常量依赖必须无环。CFD 可以使用已声明常量作为字段值。字段缺省物化规则见[类型与值](03-类型与值.md)，数据中的显式写值与 CFT 默认值来源分别保留。

## 注解

| 注解 | 目标与含义 |
| --- | --- |
| @label("文本") | 类型、枚举、成员、字段的显示名称 |
| @description("文本") | 类型、枚举、成员、字段的说明 |
| @struct | 无基类 sealed data 的结构体表示 |
| @flag | enum 的位掩码成员约束 |
| @idAsEnum(Name) | 为 table 的记录 id 生成目标语言 enum；Name 为已声明的空 enum |
| @localized | 字段绑定 language 维度 |
| @dimension("名称") | 字段绑定指定维度 |
| @expand | 具体内联对象字段的展示展开信息 |

同一字段只能绑定一个维度。注解名称、参数个数、参数类型和目标位置均参与声明检查。@idAsEnum 只用于 table，维度字段只属于 table 和 singleton。

检查规则的结构与执行边界见[Check 校验](07-Check校验.md)。
