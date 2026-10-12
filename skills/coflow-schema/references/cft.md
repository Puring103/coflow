# CFT 语言

CFT 定义配置类型、字段、默认值、枚举、常量、维度和编辑器检查规则。项目中的全部 CFT 文件共同组成全局声明集合。

```cft
enum Quality { Common, Rare = 10, Epic }
@struct sealed type Stats { hp: int = 100; }
type Item {
  @localized
  name: string;
  stats: Stats;
  quality: Quality = Quality::Common;
  next: Option<&Item> = None;
  label: fstring = f"{self.name}: {self.stats.hp}";
  check { stats.hp > 0: "生命值必须为正"; }
}
@singleton type Settings { start: &Item; }
```

## 类型

| 类型 | 数据含义 |
| --- | --- |
| int | 有符号 64 位整数 |
| float | 有限 IEEE 754 binary64 浮点数 |
| bool / string | 布尔 / 文本 |
| fstring | 加载时生成文本的字段路径模板 |
| TypeName | 内联对象 |
| &TypeName | 记录引用 |
| `Option<T>` | None 或 Some(T)，支持嵌套 |
| [T] / {K: V} | 数组 / 字典 |
| EnumName | 命名枚举值 |

普通 type 同时支持顶层记录和内联对象。类型采用单继承，abstract 类型由具体子类型实例化，sealed 类型不能继续派生。字段使用声明类型，继承链中字段不得重名。必填对象循环和无限默认值展开会产生诊断。

```cft
abstract type Entity { name: string; }
type Weapon: Entity { damage: int; }
type Link = Option<&Entity>;
const LIMIT = 100;
const EMPTY: Option<int> = None;
```

常量可以推断无歧义类型；None、空集合需要明确类型。类型别名和常量依赖必须无环。常量不包含依赖所属对象的 fstring。字典 key 使用 int、string 或 enum。

Option 每层状态独立：`None`、`Some(None)` 与 `Some(Some(3))` 不同。明确可选位置也可以提供裸值，表示一层 Some。省略字段优先读取默认值；没有默认值的 Option 字段为空，其他必填字段缺失时产生诊断。

## 枚举与注解

enum 成员数值为有符号 64 位整数，省略值时递增；名称、数值不得重复，自动编号越界时报错。`@flag` 非零成员为正的二次幂，使用同类型成员的 `&`、`^`、`|` 和括号组合。

| 注解 | 用途 |
| --- | --- |
| @label("文本") / @description("文本") | 类型、枚举、成员、字段的展示信息 |
| @singleton | 类型恰好有一条以类型名称为 key 的记录 |
| @struct | sealed type 的内联值生成目标结构体 |
| @flag | enum 的标志成员约束 |
| @idAsEnum(Name) | 以已声明的空 enum 为记录 key 生成目标枚举 |
| @localized | 字段绑定 language 维度 |
| @dimension("名称") | 字段绑定指定维度 |
| @expand | 具体内联对象字段的展开信息 |

单例不能用作内联字段或引用目标，也不能与 @idAsEnum 同时使用。同一字段只能绑定一个维度。

字符串使用双引号，`#` 开始行注释。fstring 使用 `f"..."` 和基础字段路径，连续两个同方向的花括号表示一个文本花括号。类型中的匿名 check 位于所有字段之后，顶层 check 必须命名；规则见 [Check 校验](./check.md)。
