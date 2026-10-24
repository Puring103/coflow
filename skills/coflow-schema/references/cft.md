# CFT 语言

CFT 使用 `table`、`data`、`singleton` 定义配置结构，支持枚举、常量、别名、默认值、维度和业务校验。项目中的 CFT 文件共享声明。

```cft
enum Quality { Common, Rare = 10, Epic }
sealed data Stats {
  hp: int = 100;
  check PositiveHealth { hp > 0: "生命值必须为正"; }
}
table Item {
  @localized
  name: string;
  stats: Stats;
  quality: Quality = Quality::Common;
  next: Option<&Item> = None;
  @localized
  label: string = f"{self.name}: {self.stats.hp}";
  check NameRequired { name.len() > 0: "名称不能为空"; }
}
singleton Settings { start: &Item; }
```

## 声明与身份

| 声明 | 含义 |
| --- | --- |
| `table` | 多条有 ID、可被引用的记录 |
| `data` | 内联数据对象，可声明自身校验 |
| `singleton` | 唯一记录，ID 为声明名称 |
| `type Name = T;` | 类型别名 |

字段用 `&TableName` 声明记录引用，用 data 名称声明内联值。引用写为 `&Type::id` 或 `&id`。singleton 通过唯一入口访问；table 不能内联构造，data 不能作为顶层记录。

记录的固有身份是 `id`，字典键使用 `key`。table 与 singleton 不能声明 `id` 字段；data 可以声明自己的 `id` 字段。`@idAsEnum(Name)` 可以为记录 ID 生成枚举。

类型采用单继承：table 继承 table，data 继承 data，singleton 可继承 table。abstract 类型由具体子类型实例化，sealed 类型不能派生，singleton 本身封闭。字段在继承链中不得重名。

## 类型与省略

| 类型 | 数据含义 |
| --- | --- |
| `int` / `long` | 有符号 32 位 / 64 位整数 |
| `float` / `double` | 有限 binary32 / binary64 浮点数 |
| `bool` / `string` | 布尔 / 字符串 |
| `&TableName` | 记录引用 |
| data 名称 | 内联对象 |
| `Option<T>` | `None` 或 `Some(T)`，允许嵌套 |
| `[T]` / `{K: V}` | 数组 / 字典 |
| enum 名称 | 命名枚举值 |

数值混合计算按 `int → long → float → double` 提升。数字字面量按目标类型检查范围，float 按 32 位精度舍入；已声明的数值类型不能隐式缩窄。没有目标类型时，整数字面量在 32 位范围内推断为 int，否则为 long；浮点字面量推断为 double。

```cft
data Metrics {
  count: int = 10;
  total: long = 4294967296;
  ratio: float = 0.1;
  precise: double = 0.1;
}
```

**字段只有声明了默认值才可以省略。** `Option<T>` 表示值可以为空；无默认值的 Option 字段也必须填写。

```cft
data Note {
  required: Option<string>;         # 必须填写，可以明确写 None
  optional: Option<string> = None;  # 可以省略
  count: int = 1;                   # 可以省略
}
```

Option 的每层状态独立，`None`、`Some(None)`、`Some(Some(3))` 不同。在已知 Option 位置提供裸值表示一层 `Some`。

```cft
const LIMIT = 100;
const EMPTY: Option<int> = None;
type ItemLink = Option<&Item>;
```

常量可推断无歧义类型，`None` 和空集合需要明确类型。别名与常量依赖必须无环。字典 key 使用 int、long、string 或 enum。

## 字符串与名称

字符串统一声明为 `string`。普通双引号文本保留花括号，只有显式 `f"..."` 执行字段路径插值；连续两个左花括号或右花括号表示文本花括号。

```cft
data Label {
  name: string;
  literal: string = "{self.name}";
  title: string = f"名称：{self.name}";
}
```

模板可以用于字段值、默认值、数组元素和字典值。依赖所属对象的模板不能用于常量。规则消息使用规则作用域中的字段和绑定，详见 [Check 校验](./check.md)。

语法结构词与字面量词受保护。`min`、`max`、`keys`、`values`、`records`、`return` 等可以作为业务名称；内建类型名称不能被类型声明覆盖。`#` 开始行注释。

## 注解与规则

| 注解 | 用途 |
| --- | --- |
| `@label` / `@description` | 类型、枚举、成员、字段的展示信息 |
| `@struct` | 无基类 sealed data 生成目标结构体 |
| `@flag` | 位枚举，自动提供 None 和 Everything |
| `@int64` | enum 使用 64 位 long，默认使用 32 位 int |
| `@idAsEnum(Name)` | 为 table 的 ID 填充已声明的空 enum |
| `@localized` / `@dimension("名称")` | table 或 singleton 字段的维度绑定 |
| `@expand` | 具体 data 字段的展开信息 |

enum 默认使用 32 位 int，需要 64 位时添加 `@int64`。省略成员值时递增，用户成员名称和数值不能重复；超过声明范围时报错，不会自动升级位宽。

```cft
enum Status { Pending = 0, Done = 1 }
@int64 enum LargeCode { Special = 4294967296 }
@flag enum Permission { Read = 1, Write = 2, Execute = 8 }
@flag @int64 enum LargePermission { Manage = 4294967296 }
```

位枚举自动提供 `None = 0` 和 `Everything`。Everything 等于所有实际标志位的按位或，随成员变化重新计算；上例 `Permission::Everything` 为 11。两个内建名称不能自行声明。用户非零成员必须为正的二次幂；32 位支持 31 个独立标志位，64 位支持 63 个。支持同类型成员的 `&`、`^`、`|` 组合。普通枚举不自动添加这两个成员。

data、table、singleton 的字段之后可以声明多个命名或匿名 `check`。规则按父类到子类、同类型声明顺序执行；同一继承链中的规则名不能重复。项目级规则在顶层声明，名称可省略，使用 `records(TypeName)` 检查集合与记录之间的关系。

```cft
table Sample {
  value: int;
  check { value >= 0; }
  check: "数值不能超过上限" { value <= 100; }
}
check { records(Sample).len() > 0; }
```

匿名规则使用根据源文件和位置自动生成的标识，可通过 CLI 列出、查询和单独执行。源码位置变化时标识可能变化，需要长期稳定标识时为规则命名。
