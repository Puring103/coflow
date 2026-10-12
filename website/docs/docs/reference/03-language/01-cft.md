# CFT 语言

CFT 使用 table、data、singleton 定义配置结构，并提供枚举、常量、别名、默认值、维度和编辑器校验。全部 CFT 文件组成项目全局声明集合。

```cft
enum Quality { Common, Rare = 10, Epic }
@struct sealed data Stats { hp: int = 100; }
table Item {
  @localized
  name: string;
  stats: Stats;
  quality: Quality = Quality::Common;
  next: Option<&Item> = None;
  label: fstring = f"{self.name}: {self.stats.hp}";
  check { stats.hp > 0: "生命值必须为正"; }
}
singleton Settings { start: &Item; }
```

## 声明边界

| 声明 | 语义 |
| --- | --- |
| table | 多条有身份、可被引用的记录 |
| data | 按内容保存的内联数据对象 |
| singleton | 唯一配置记录，key 为声明名称 |
| type Name = T; | 类型别名 |

字段使用 `&TableName` 声明记录引用，使用 data 名称声明内联值。`&` 的目标必须是 table，引用值使用 `&Type::key` 或 `&key`。singleton 通过唯一入口访问，不作为字段类型。table 不能内联构造，data 不作为顶层记录。

类型采用单继承：table 继承 table，data 继承 data，singleton 可继承 table。abstract 声明通过具体子类型实例化，sealed 类型不能派生，singleton 本身封闭。字段在继承链中不得重名。

## 类型与默认值

| 类型 | 数据含义 |
| --- | --- |
| int / float | 有符号 64 位整数 / 有限 binary64 浮点 |
| bool / string | 布尔 / 普通文本 |
| fstring | 加载时生成文本的字段路径模板 |
| `&TableName` | 记录引用 |
| data 名称 | 内联对象 |
| `Option<T>` | None 或 Some(T)，可以嵌套 |
| [T] / {K: V} | 数组 / 字典 |
| enum 名称 | 命名枚举值 |

```cft
const LIMIT = 100;
const EMPTY: Option<int> = None;
type ItemLink = Option<&Item>;
```

常量可推断无歧义类型，None、空集合需要明确类型。别名与常量依赖必须无环，常量不包含依赖所属对象的 fstring。字典 key 使用 int、string 或 enum。

Option 每层独立，None、Some(None)、Some(Some(3)) 不同。明确可选位置可提供裸值，表示一层 Some。省略字段使用默认值；无默认值的 Option 为空，其他必填字段缺失时产生诊断。必填 data 循环与无限默认值展开会产生声明诊断。

## 枚举与注解

enum 的值为有符号 64 位整数，省略时递增，名称和数值不能重复。@flag 的非零成员为正的二次幂，用同类型成员的 &、^、| 和括号组合。

| 注解 | 用途 |
| --- | --- |
| @label / @description | 类型、枚举、成员、字段的展示信息 |
| @struct | 无基类 sealed data 生成目标结构体 |
| @flag | enum 的标志约束 |
| @idAsEnum(Name) | 为 table 的 key 填充已声明的空 enum |
| @localized / @dimension("名称") | table 或 singleton 字段的维度绑定 |
| @expand | 具体 data 字段的展开信息 |

同一字段只能绑定一个维度。table 与 singleton 的匿名 check 位于全部字段之后，data 的校验由所属记录规则表达；项目级 check 必须命名。规则见 [Check 校验](./04-check.md)。

字符串使用双引号，# 开始行注释。fstring 使用显式前缀和字段路径，连续两个同方向花括号表示一个文本花括号。
