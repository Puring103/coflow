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
| `int` / `float` | 有符号 64 位整数 / 有限 binary64 浮点 |
| `bool` / `string` | 布尔 / 字符串 |
| `&TableName` | 记录引用 |
| data 名称 | 内联对象 |
| `Option<T>` | `None` 或 `Some(T)`，允许嵌套 |
| `[T]` / `{K: V}` | 数组 / 字典 |
| enum 名称 | 命名枚举值 |

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

常量可推断无歧义类型，`None` 和空集合需要明确类型。别名与常量依赖必须无环。字典 key 使用 int、string 或 enum。

## 字符串与名称

字符串统一声明为 `string`。普通双引号文本保留花括号，只有显式 `f"..."` 执行字段路径插值；连续两个左花括号或右花括号表示文本花括号。

```cft
data Label {
  name: string;
  literal: string = "{self.name}";
  title: string = f"名称：{self.name}";
}
```

模板可以用于字段值、默认值、数组元素和字典值。依赖所属对象的模板不能用于常量。规则消息使用规则作用域中的字段和绑定，详见 [Check 校验](./04-check.md)。

语法结构词与字面量词受保护。`min`、`max`、`keys`、`values`、`records`、`return` 等可以作为业务名称；内建类型名称不能被类型声明覆盖。`#` 开始行注释。

## 注解与规则

| 注解 | 用途 |
| --- | --- |
| `@label` / `@description` | 类型、枚举、成员、字段的展示信息 |
| `@struct` | 无基类 sealed data 生成目标结构体 |
| `@flag` | enum 的标志约束 |
| `@idAsEnum(Name)` | 为 table 的 ID 填充已声明的空 enum |
| `@localized` / `@dimension("名称")` | table 或 singleton 字段的维度绑定 |
| `@expand` | 具体 data 字段的展开信息 |

enum 的值为有符号 64 位整数，省略时递增，名称和数值不能重复。`@flag` 的非零成员为正的二次幂，支持同类型成员的 `&`、`^`、`|` 组合。

data、table、singleton 的字段之后可以声明多个命名 `check`。规则按父类到子类、同类型声明顺序执行；同一继承链中的规则名不能重复。项目级规则独立命名，使用 `records(TypeName)` 检查集合与记录之间的关系。
