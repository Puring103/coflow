# CFT

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

### 继承写法

```cft
abstract table Entity { name: string; }
table Item: Entity { price: int = 0; }
singleton StartingItem: Entity { count: int = 1; }
abstract data Shape { color: string; }
sealed data Circle: Shape { radius: float; }
@struct sealed data Point { x: float; y: float; }
```

基类写在冒号后，字段继承后共同构成实际类型。`singleton` 恰好对应一条同名记录；它不能用作字段类型或引用目标。`&` 的目标为 table，内联字段使用 data 类型。

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

模板可以用于字段值、默认值、数组元素和字典值。依赖所属对象的模板不能用于常量。规则消息使用规则作用域中的字段和绑定，详见 [业务校验](#业务校验)。

语法结构词与字面量词受保护。`min`、`max`、`keys`、`values`、`records`、`return` 等可以作为业务名称；内建类型名称不能被类型声明覆盖。`#` 开始行注释。

## 注解与规则

| 注解 | 用途 |
| --- | --- |
| `@label("文本")` / `@description("文本")` | 类型、枚举、成员、字段的展示信息 |
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

## 业务校验

check 为配置定义只读业务规则。`data`、`table` 和 `singleton` 可以在字段之后声明多个命名或匿名规则，项目规则可以独立声明在顶层。

```cft
data Reward {
  count: int;
  check PositiveCount: "奖励数量无效" {
    count > 0: f"数量 {count} 必须大于 0";
  }
}

table Item {
  price: int;
  tags: [string];
  reward: Reward;
  check Price { price >= 0: "价格不能为负数"; }
  check Tags { tags.isUnique(): "标签不能重复"; }
}
```

规则名称可省略，每条规则至少包含一条语句。匿名规则可以重复声明；命名规则在同一类型及其继承链内不能重名，无继承关系的类型可以使用相同短名。父类型规则先执行，再按声明顺序执行本类型规则。data 规则会自动检查每个内联实例，包括数组、字典和可选值内的对象。

```cft
table Sample {
  value: int;
  check { value >= 0; }
  check: "数值不能超过上限" { value <= 100; }
}
check { records(Sample).len() > 0; }
```

匿名规则使用根据源文件和位置自动生成的标识，可通过 CLI 列出、查询和单独执行。源码位置变化时标识可能变化，需要长期稳定标识时为规则命名。


每条断言返回 bool，可在冒号后提供普通字符串或 `f"..."` 消息。规则、when 和量词也可以在左花括号前提供消息；外层消息补充具体断言的错误内容和位置。

### 条件、集合与关联

顶层规则使用 `records(Type)` 获取 table 或 singleton 的记录集合。字段引用可以直接访问目标字段，例如 `reward.item.enabled`。

```cft
check AvailableItems {
  any item in records(Item): "至少需要一个可用物品" {
    item.price > 0 && item.tags.contains("available");
  }
}
```

| 语法 | 要求 |
| --- | --- |
| `when condition { ... }` | 条件成立时执行主体 |
| `all item in collection { ... }` | 全部元素满足主体 |
| `any item in collection { ... }` | 至少一个元素满足主体 |
| `none item in collection { ... }` | 没有元素满足主体 |

空集合满足 all 和 none，不满足 any。数组双绑定为 `item, index`，索引从零开始；字典双绑定为 `key, value`。量词绑定只在主体内可用，块前消息使用外层作用域。

`when` 条件不成立时主体不执行，该语句正常完成。在 any/none 中同时筛选和检查元素时，使用 `item.enabled && item.price > 0` 这样的布尔组合。

`is Some(value)` 解包一层 Option，`is None` 判断空态，`is TypeName` 判断实际类型。可以使用字段路径、索引、算术、比较、逻辑和位运算，以及 `len`、`contains`、`isUnique`、`matches`、`sum` 等操作。逻辑条件短路求值。

### 检查维度

直接读取字段取得基础值；`for` 选择变体；`hasVariant` 判断字段是否显式填写该变体；`variants` 遍历所属维度全部已发现变体的有效值。

```cft
table Text {
  @localized name: string;
  check TranslationComplete {
    !name.isBlank(): "基础名称不能为空";
    name.hasVariant("ja"): "必须填写日语名称";
    all variant, text in name.variants(): "翻译不完整" {
      name.hasVariant(variant): f"缺少 {variant} 的名称";
      !text.isBlank(): f"{variant} 名称不能为空";
    }
    !name.for("ja").isBlank(): "日语名称不能为空";
  }
}
```

显式 `None`、空字符串或与基础值相同的覆盖也算存在。缺少覆盖时 for 使用基础表达式；其中的模板引用继续读取所选变体。variants 包含回退值，hasVariant 可用于区分已填写和回退。尚未在项目中出现的变体应通过 `hasVariant("ja")` 这样的显式规则检查。

### 执行和诊断

```sh
coflow rule list
coflow check --rules
coflow check --rule Reward::PositiveCount --explain
```

类型规则以 `类型::规则名` 选择，顶层规则直接使用名称。`coflow check` 检查声明与 CFD 数据，`--rules` 执行全部业务规则，`--rule` 选择规则，`--explain` 展示执行解释。`coflow build` 在全部校验通过后生成代码。编辑器在编辑过程中执行规则并显示诊断。

普通断言失败会继续收集错误；除零、越界等求值错误停止当前规则，其余规则继续。报告区分通过、条件未触发、业务失败、求值错误、预算耗尽和未执行，并包含规则名、CFT 位置、CFD 字段位置及相关元素或变体。预算耗尽表示检查未完成。CLI 默认返回计数与有界诊断，`--details` 展开逐目标规则和语句状态，`--explain` 提供实际值与执行解释。

CLI 的数据、统计、引用和维度查询可以辅助查找规则涉及的记录。生成的 C# 类型与 Coflow.Loader 负责加载数据，业务规则在交付前通过 CLI 或编辑器验证。

### 运算与内建方法

类型规则直接读取当前对象字段，引用路径可以继续读取目标字段；顶层规则用 `records(Type)` 获取该类型及子类型的记录。字典单绑定可读取 `.key` 和 `.value`，双绑定为 `key, value`。

| 运算 | 写法与行为 |
| --- | --- |
| 读取 | `stats.hp`、`items[0]`、`labels["name"]` |
| 算术 | `+`、`-`、`*`、`/`、`//`、`%`、`**`、一元 `-` |
| 比较 | `==`、`!=`、`<`、`<=`、`>`、`>=`；链式比较要求方向一致 |
| 逻辑 | `!`、`&&`、`\|\|`；后两者短路求值 |
| 位运算 | `~`、`&`、`^`、`\|`、`<<`、`>>`；接受适用的整数或位枚举 |
| 类型判断 | `value is TypeName`、`value is Some(item)`、`value is None` |
| 分组 | 用括号明确运算顺序 |

`is Some(item)` 的绑定可用于同一条件后续的 `&&` 和成立时的 `when` 主体。实际类型判断成立后可读取子类型字段。普通操作数从左到右求值；整数溢出、除零、无效移位、越界或不存在的字典键产生求值诊断。

| 接收值 | 方法 | 含义 |
| --- | --- | --- |
| 字符串 | `len()`、`isBlank()` | 长度、是否为空白 |
| 字符串 | `contains(text)`、`startsWith(text)`、`endsWith(text)` | 包含、前缀、后缀 |
| 字符串 | `matches("正则")` | 用静态正则匹配，模式必须为字符串字面量 |
| 数组 | `len()`、`contains(value)`、`isUnique()` | 长度、包含、唯一性 |
| 数组 | `min()`、`max()`、`sum()` | 最小、最大、总和；空集合不能求极值 |
| 数组 | `isSorted()`、`isStrictlySorted()` | 非递减、严格递增 |
| 字典 | `len()`、`contains(key)`、`containsKey(key)`、`containsValue(value)` | 大小、键或值是否存在 |
| 字典 | `keys()`、`values()` | 键集合、值集合 |
| 支持标量元素的集合 | `intersects(other)`、`isDisjoint(other)` | 存在交集、互不相交 |
| 支持标量元素的集合 | `isSubsetOf(other)`、`isSupersetOf(other)` | 子集、超集 |
| 数值 | `abs()` | 绝对值 |
| 浮点数 | `isFinite()`、`approxEqual(other, tolerance)` | 有限值、指定容差内近似相等 |
| 维度字段 | `for(variant)`、`hasVariant(variant)`、`variants()` | 有效值、显式覆盖、全部已发现变体 |

`//` 为整数除法，`%` 为整数取余，`**` 为乘方。校验表达式的运算优先级从高到低为：字段、索引与调用 → 一元 `!` / `~` / `-` → `**` → `*` / `/` / `//` / `%` → `+` / `-` / `<<` / `>>` → `&` / `^` / `|` → 比较 → `is` → `&&` → `||`。同一层从左到右结合，乘方从右到左结合；括号可明确覆盖顺序。

校验中可以用 `EnumName(integer)` 从 int 或 long 表达式构造枚举值，例如 `quality == Quality(10)`。记录集合通过 `records(TypeName)` 获取，其他内建操作使用上表的方法形式。

方法的接收类型、元素类型和参数都会进行检查。数值聚合保持声明类型并检查溢出；集合比较与唯一性使用值的相等规则。

## 数据模型与递归

记录具有类型与 ID，引用连接记录身份并可形成环；内联 data 按实际类型与字段内容表示。数组保持元素顺序，字典键唯一；数组逐项比较，字典按键和值比较，Option 按每层状态和内容比较，记录引用按身份比较。

必填内联对象不能构成无终点的递归。需要树结构时，通过 Option 或集合提供终止条件：

```cft
data Node {
  name: string;
  children: [Node] = [];
}
```

默认值也必须能有限展开。`None`、空集合或显式填写的终止字段可结束递归；别名、常量和继承关系不能成环。

## 文件与词法

CFT 使用 UTF-8 文本。标识符首字符为 `_` 或 Unicode XID_Start，后续为 `_` 或 XID_Continue；区分大小写，不进行 Unicode 规范化。项目中的所有文件共享声明名称。

字段以分号结束，枚举成员以逗号分隔，`#` 开始行注释。字符串支持 `\n`、`\r`、`\t`、`\"`、`\\` 五种转义，其他 Unicode 字符直接输入。

全局保留名称为 `_`、`const`、`enum`、`type`、`table`、`data`、`singleton`、`abstract`、`sealed`、`check`、`when`、`all`、`any`、`none`、`in`、`is`、`true`、`false`、`None`、`Some`。内建类型名在类型位置有固定含义。

声明的基本形式如下：

```text
类型： [注解] [abstract | sealed] table|data|singleton 名称 [: 基类] { 字段… 校验… }
字段： [注解] 名称: 类型 [= 默认值];
枚举： [注解] enum 名称 { 成员 [= 整数], … }
别名： type 名称 = 类型;
常量： const 名称 [: 类型] = 值;
校验： check [名称] [: 消息] { 校验语句… }
类型： int | long | float | double | bool | string | 名称 | &名称
     | Option<类型> | [类型] | {键类型: 值类型} | (类型)
```

插值只读取字段路径，末端需为数值、bool、enum 或字符串；对象、集合和 Option 需先选择可读取的标量字段。嵌套 data 中的模板以该对象为 `self`。常规默认值使用值表达式，业务计算与条件写在校验中。更多写值示例见 [CFD](./cfd.md)。
