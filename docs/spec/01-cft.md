# CFT 语言规格

CFT（Coflow Type File，`.cft`）是一种自校验的类型定义语言，用于声明数据结构的 schema。`.cft` 文件只包含 `const`、`enum`、`type` 定义，不含数据、不执行代码、不做 I/O。

---

## 目录

1. [基本语法](#1-基本语法)
2. [常量](#2-常量)
3. [枚举](#3-枚举)
4. [类型](#4-类型)
5. [check 块](#5-check-块)
6. [注解](#6-注解)
7. [模块系统与 Loader 接口](#7-模块系统与-loader-接口)
8. [错误阶段](#8-错误阶段)
9. [综合示例](#9-综合示例)

---

## 1. 基本语法

注释使用 `#`，可单独成行也可放在行尾：

```cft
# 这是注释
type Item { id: string; }  # 行尾注释
```

所有顶层定义（`const`、`enum`、`type`）共享同一个**全局命名空间**，名称在整个项目中唯一，支持前向引用，无需按声明顺序排列。

标识符遵循 Unicode XID 规则（`unicode-ident`），允许中文、Emoji 之外的合法字符。Reserved identifiers 包括当前关键字和字面量、primitive 类型名、当前内建函数名、为未来语法保留的名称，以及 `_`。这些名称不能用作 `const`、`enum`、`type`、字段、枚举变体或量词变量名称。

当前保留名至少包括：

- 关键字和字面量：`const`、`enum`、`type`、`abstract`、`sealed`、`check`、`when`、`all`、`any`、`none`、`in`、`is`、`true`、`false`、`null`
- primitive 类型名：`int`、`float`、`bool`、`string`
- 当前内建函数名：`len`、`contains`、`unique`、`min`、`max`、`sum`、`keys`、`values`、`matches`
- 未来语法保留名：`if`、`else`、`match`、`case`、`for`、`while`、`let`、`module`、`import`、`export`、`from`、`as`、`use`
- `_`

---

## 2. 常量

`const` 定义编译期常量，可用于字段默认值和 `check` 表达式：

```cft
const MAX_LEVEL  = 100;
const MIN_SPEED  = 0.1;
const EMPTY_NAME = "unknown";
```

- 值只允许整数、浮点、布尔、字符串字面量
- 浮点字面量必须能解析为有限 `f64`；`NaN` 和 `+/-inf` 不是合法 CFT 字面量
- 类型从值自动推断，无需显式标注
- 也可以显式标注类型（仅限 `int` / `float` / `bool` / `string`），编译期会校验值与类型一致：

```cft
const MAX_LEVEL: int    = 100;
const MIN_SPEED: float  = 0.1;
const NAME: string      = "hero";
```

- 标注 named type（如 `const X: Foo = 1;`）会以 `CFT-SCHEMA-030` 报错
- 不允许 `null`、数组、对象等非字面量值
- `const` 不接受任何注解

---

## 3. 枚举

`enum` 定义有限的命名整数集合，变体之间用 `,` 分隔，允许末尾 `,`：

```cft
enum Rarity {
  Common,
  Rare,
  Epic,
}
```

变体默认从 `0` 开始自动编号，依次递增；可以显式指定整数值，未指定的变体从前一个值 +1 继续：

```cft
enum Status {
  None   = 0,
  Active = 10,
  Dead   = 20,
  Ghost,          # 自动为 21
}
```

- 同一枚举内禁止重复整数值
- 枚举值通过 `EnumName.Variant` 使用；裸写 `EnumName` 作为运算数会报 `CFT-TYPE-005`，提示需要 `EnumName.Variant` 或 `EnumName(0)`
- 枚举类型与 `int` 不隐式互转；枚举只能与同类型枚举比较，`rarity > 5` 报类型错误
- 枚举类型支持六种比较运算符（`==` `!=` `<` `<=` `>` `>=`），按底层整数值比较
- 枚举变体允许携带 `@display("text")` 和 `@deprecated`；其他注解用于枚举变体均无效

**位标志枚举**使用 `@flag` 注解，所有变体值必须为 2 的幂（0 除外）：

```cft
@flag
enum Permission {
  Read    = 1,
  Write   = 2,
  Execute = 4,
}
```

`@flag` 枚举支持按位运算（`&` `|` `^` `~`），运算结果仍为同一枚举类型：

```cft
# check 块中
(flags & Permission.Read) != Permission(0)   # 合法
(flags & Permission.Read) != 0               # 错误：不能与 int 比较
```

`Permission(0)` 表示该枚举的整数零值。

---

## 4. 类型

### 4.1 基本结构

```cft
type Weapon {
  id:       string;
  damage:   int;
  cooldown: float = 1.0;
}
```

字段之间用 `;` 分隔，允许末尾 `;`。

- 无默认值的字段**必须**填写
- 有默认值的字段可以省略
- 默认值必须是编译期常量（字面量、`const` 常量或枚举值，包括空数组 `[]`、空对象 `{}`）
- 默认值不能引用其他字段
- 子类不能声明与父类（任意层级）同名的字段

### 4.2 字段类型

| 类型 | 说明 |
|------|------|
| `int` | 64 位整数 |
| `float` | 64 位浮点 |
| `bool` | 布尔值 |
| `string` | 字符串 |
| `[T]` | 数组，T 为任意合法字段类型 |
| `{K: V}` | 字典，K 只允许 `string`、`int` 或 `enum` 类型名 |
| `T?` | nullable，等价于 `T \| null`；`null` 是显式值，不等于字段缺失 |
| `TypeName` | 引用已定义的 `type`（含父类及子类） |
| `EnumName` | 引用已定义的 `enum` |

`type` 使用**名义类型**（nominal typing），不使用结构类型：两个字段完全相同的 `type` 不能互相替换。

字典 key 在 schema-guided 解析后必须唯一。重复 key 是加载错误，不允许后写覆盖。枚举 key 的等价性按“枚举类型 + 底层整数值”判断，不同 enum 即使底层值相同也不是同一个 key。

### 4.3 修饰符

| 关键词 | 语义 |
|--------|------|
| `abstract` | 禁止直接实例化，只能通过子类使用 |
| `sealed` | 禁止被继承；可以直接实例化 |
| （无修饰符） | 可以实例化，可以被继承（默认） |

`abstract` 和 `sealed` 互斥，同时使用报错。

### 4.4 继承

使用 `:` 声明父类，支持单继承和多层继承：

```cft
abstract type Reward {
  id: string;
}

type ItemReward : Reward {
  item:  Item;
  count: int = 1;
}

type CurrencyReward : Reward {
  amount: int;
}
```

规则：
- 每个 `type` 最多一个父类
- `sealed type` 不能被继承
- 子类继承父类所有字段
- 子类不能声明与父类（任意层级）同名的字段
- 子类实例可以赋值给父类类型的字段
- 如果父类声明了 `@id` 字段，子类继承同一个主键字段；子类不能重新声明另一个 `@id` 字段

字段类型为 `abstract type` 时，只能填入其子类实例：

```cft
type Quest {
  reward: Reward;    # Reward 是 abstract，只能填 ItemReward 或 CurrencyReward
}
```

字段类型为普通 `type` 时，可以填入该类型本身或任意子类实例。

`@ref` 的目标类型可以是 `abstract type` 或有子类的普通 `type`，表示持有该类型赋值兼容范围内任意实例的 ID。该范围内的所有记录共享一个继承树索引，`@id` 字段值必须唯一。

### 4.5 nullable

`T?` 是 `T | null` 的简写：

```cft
type Drop {
  item:   Item?;           # 必须显式填写，可以填 null 或 Item 实例
  backup: Item? = null;    # 有默认值，可以省略
}
```

对 `null` 做字段访问、索引访问、大小比较或算术，会在 check 执行时报错；编译期不报错，以避免误伤 `item != null && item.id != ""` 这类安全访问写法。安全访问惯用法：

```cft
item != null && item.id != ""
```

静态类型推断时，`T?` 与 `T` 在算术、比较、索引等位置等价处理（即 nullable 包装会被脱去），结果类型是 `T`。`null` 值在运行期触发 eval error。

### 4.6 前向引用与自引用

无需按声明顺序排列，支持前向引用和自引用：

```cft
type Node {
  value:    int;
  children: [Node];    # 自引用
}

type A { b: B; }       # 前向引用
type B { value: int; }
```

---

## 5. check 块

`check` 是 `type` 内部的可选校验块，必须位于所有字段声明之后。`check` 块在对象构建完成（含 `@ref` 解析）后由宿主显式调用执行，执行期间对象图不可变，不影响对象图构建。

`check` 内可以访问当前对象的所有字段（含继承字段）、`const` 常量和枚举值，不能引用外部节点。check 内的 `@ref` 字段已是解析后的引用，可以直接访问目标对象的字段。

**继承与 check**：子类实例依次执行从根类到当前类的所有 `check` 块：

```cft
abstract type Reward {
  id: string;
  check { id != ""; }                   # 对所有子类实例执行
}

type CurrencyReward : Reward {
  amount: int;
  check { amount > 0; }                 # 只在 CurrencyReward 实例上执行
}
# 执行顺序：Reward.check → CurrencyReward.check
```

### 5.1 条件语句

一个表达式加分号，求值结果必须为 `bool`。多条语句相互独立，不能依赖前面语句的结果；条件为假时继续执行后续语句，收集全部错误：

```cft
check {
  damage > 0;
  cooldown >= MIN_COOLDOWN;
  id != "";
  0 < damage <= MAX_DAMAGE;    # 链式比较（方向一致）
}
```

**链式比较**：所有比较运算符方向相同，即全部是 `<`/`<=`（递增）或全部是 `>`/`>=`（递减）。`a < b > c` 方向不一致，报语法错误（`CFT-SYN-006`）。`!=` 不允许出现在链中。从左到右短路求值，某步为 false 时立即停止。链中相邻每一对操作数独立做静态类型检查；`0 < x < y` 中若 `x: int`、`y: float`，则在第二对触发 `CFT-TYPE-006 ComparisonTypeMismatch`。

### 5.2 量词块

对集合中每个元素执行条件：

```cft
# Array：绑定变量直接是元素
all drop in drops {
  drop.value > 0;
}

# Dict：绑定变量是 entry 对象，具有 .key 和 .value 字段
all entry in scores {
  entry.value >= 0;
}
```

| 量词 | 语义 | 空集合行为 |
|------|------|-----------|
| `all x in col { ... }` | 全部元素通过 | 通过 |
| `any x in col { ... }` | 至少一个元素通过 | 失败 |
| `none x in col { ... }` | 没有元素通过 | 通过 |

量词块中某个元素的条件为假时，继续处理后续元素，收集全部失败。只有类型错误或越界才立即停止当前量词块。

多态数组中每个元素独立执行自己完整继承链上的 check，元素之间互不影响。

如果需要表达"可以为空或至少满足一个"，惯用法为：

```cft
when len(rewards) > 0 {
  any reward in rewards { reward is CurrencyReward; }
}
```

量词块是 check 语句，不是表达式，不能嵌入 `&&`、`||` 或其他表达式内部。

### 5.3 when 块

条件成立时，块内所有语句必须通过；条件不成立时整块直接通过：

```cft
type Skill {
  is_passive: bool;
  cooldown:   float? = null;
  range:      float? = null;

  check {
    when !is_passive {
      cooldown != null;
      cooldown > 0.0;
    }
    when is_passive {
      range != null;
    }
  }
}
```

等价语义：`when cond { s1; s2; }` = `!cond || (s1 && s2)`。

`when` 支持嵌套，也可以与量词块组合：

```cft
all item in items {
  when item.is_rare {
    item.price > 100;
  }
}
```

### 5.4 运算符

优先级从低到高：

| 优先级 | 运算符 | 说明 |
|--------|--------|------|
| 1（最低）| `\|\|` `&&` | 逻辑或/与，短路求值 |
| 2 | `is` | 类型判断 |
| 3 | `==` `!=` `<` `<=` `>` `>=` | 比较，支持链式（如 `0 < x <= 100`） |
| 4 | `\|` `^` `&` | 按位或/异或/与 |
| 5 | `+` `-` `<<` `>>` | 加减、位移（`<<` `>>` 仅支持 `int`） |
| 6 | `*` `/` `//` `%` | 乘、除、整除、取模 |
| 7 | `**` | 幂（右结合） |
| 8 | `!` `~` `-` | 一元逻辑非、按位非、取负 |
| 9（最高）| `.field` `[index]` `fn()` | 字段访问、索引、函数调用 |

### 5.5 `is` 类型判断

`is TypeName` 是可赋值动态类型谓词：对象的实际类型等于 TypeName 或任意子类时返回 `true`。`is null` 对任意 nullable 操作数有效，对非 nullable 操作数无效：

```cft
reward is Reward          # Reward 或任意子类均为 true
reward is CurrencyReward  # CurrencyReward 或任意子类均为 true
item is null              # null 判断
```

`is` 的右侧只能是已定义的 `type` 名或 `null`。primitive 类型（`int`、`float`、`bool`、`string`）和 `enum` 名不允许作为目标，会以 `CFT-TYPE-014 InvalidIsPredicate` 报错。`is TypeName` 的左侧必须是对象或可空对象；`is null` 的左侧必须是 nullable 类型。否则触发 `CFT-TYPE-005 OperatorTypeMismatch`。

### 5.6 内建函数

| 函数 | 参数 | 返回值 | 说明 |
|------|------|--------|------|
| `len(col)` | array 或 dict | int | 元素数量；数组中 `null` 元素照常计数 |
| `contains(col, val)` | array + 元素，或 dict + key | bool | 存在性判断 |
| `unique(array)` | array | bool | 元素是否唯一（支持 int、bool、string、enum 及其 nullable 形式） |
| `min(array)` | 非空 int / float / enum array | 同元素类型 | 最小值 |
| `max(array)` | 非空 int / float / enum array | 同元素类型 | 最大值 |
| `sum(array)` | int 或 float array | 同元素类型 | 求和 |
| `keys(dict)` | dict | array | key 数组 |
| `values(dict)` | dict | array | value 数组 |
| `matches(str, pat)` | string + 正则字符串字面量 | bool | 正则匹配，pattern 使用标准双引号字符串，Unicode 感知 |

注意：
- `unique` 将 `null` 当作可比较值处理；除 nullable 元素外，不支持 float、object、array、dict
- `min` / `max` 跳过 `null`，没有任何非 `null` 值时报 check eval error
- `sum` 跳过 `null`，没有任何非 `null` 值时返回 `0`
- `contains([T?], null)` 检查数组中是否存在 `null` 元素
- `contains(dict, val)` 只检查 key，不检查 value
- `<<` `>>` 两个操作数均必须是 `int`

### 5.7 执行规则

- 多条语句顺序求值，条件为假时继续收集后续错误
- 求值中出现类型错误或越界时，立即停止当前对象的校验
- 同一对象被多处引用时，其 check 只执行一次（按 identity 去重）
- check 在所有数据加载完成（含 `@ref` 解析）后执行，执行期间对象图不可变

---

## 6. 注解

注解附加在 `type`、`enum`、字段声明之前，驱动代码生成和加载器行为，不影响语言语义。每个注解有明确的适用范围，范围外使用立即报错。多个注解可以叠加，每行一个。

| 注解 | 适用目标 | 字段类型限制 | 额外约束 | 说明 |
|------|---------|------------|---------|------|
| `@struct` | `type` | — | 必须是 `sealed type` | codegen 生成值类型（C# struct） |
| `@flag` | `enum` | — | 变体值必须为 2 的幂（0 除外） | 位标志枚举（C# [Flags]） |
| `@id` | `field` | `string`、`int` | 每个继承树最多一个 `@id` 字段；子类继承父类的 `@id` | 主键，加载器用于跨表引用解析，隐含唯一性 |
| `@ref(TypeName)` | `field` | `string`、`int` | TypeName 必须是已定义的 `type`（含 abstract） | 字符串/整数外键，加载器校验目标记录存在 |
| `@index` | `field` | `string`、`int`、`enum` | 字段必须非 nullable | codegen 生成按此字段查询的索引 API；索引始终返回列表 |
| `@display("text")` | `type`、`enum`、`field`、`enum variant` | 任意 | — | 可读名称，codegen 生成 XML 注释，用于编辑器显示 |
| `@deprecated` | `type`、`enum`、`field`、`enum variant` | 任意 | — | 标记废弃，codegen 输出对应语言的废弃标记；子类不自动继承父类的 `@deprecated` |

枚举变体只允许 `@display("text")` 和 `@deprecated`；其他注解用于枚举变体时以 `CFT-SCHEMA-023 InvalidAnnotationTarget` 报错。

示例：

```cft
@display("物品")
type Item {
  @id
  id: string;

  @index
  @display("稀有度")
  rarity: Rarity;

  @ref(Item)
  @display("升级目标")
  next_tier_id: string? = null;

  @deprecated
  @display("旧价格")
  old_price: int = 0;
}
```

---

## 7. 模块系统与 Loader 接口

`.cft` 没有 `use` 导入语句。所有已注册模块共享同一个全局命名空间，宿主负责将文件批量注册到 `CftContainer`。

**ModuleId** 由宿主指定，推荐使用相对项目根的路径去掉扩展名：

```
schema/item.cft   →  "schema/item"
schema/enemy.cft  →  "schema/enemy"
```

**Loader 本身无 I/O**，路径解析和文件读取由宿主负责：

```rust
impl CftContainer {
    pub fn new() -> Self;

    // 注册并解析一个 .cft 源文本。重复 ModuleId、词法错误或语法错误会立即报错。
    pub fn add_module(
        &mut self,
        id: ModuleId,
        source: impl Into<String>,
    ) -> Result<(), CftDiagnostics>;

    // 编译所有已注册模块，统一校验全局命名空间、字段类型、继承、注解、默认值和 check 静态类型。
    pub fn compile(&mut self) -> Result<(), CftDiagnostics>;

    // schema 反射，用于代码生成和 Excel 加载器字段映射；返回的引用在
    // 下次 add_module 或下次成功调用 compile 之前保持稳定。add_module
    // 会立即使已发布 schema 失效；失败的 compile 不发布新 schema。
    pub fn schema(&self, id: &ModuleId) -> Option<&CftSchemaModule>;
    pub fn resolve_type(&self, name: &str) -> Option<&CftSchemaType>;
    pub fn resolve_enum(&self, name: &str) -> Option<&CftSchemaEnum>;
    pub fn resolve_const(&self, name: &str) -> Option<&CftSchemaConst>;

    // 遍历
    pub fn module_ids(&self) -> impl Iterator<Item = &ModuleId>;
    pub fn all_types(&self) -> impl Iterator<Item = &CftSchemaType>;
    pub fn all_enums(&self) -> impl Iterator<Item = &CftSchemaEnum>;
    pub fn has_type(&self, name: &str) -> bool;
    pub fn has_enum(&self, name: &str) -> bool;
}
```

**Schema 结构：**

```rust
pub struct CftSchemaModule {
    pub consts: Vec<CftSchemaConst>,
    pub types:  Vec<CftSchemaType>,
    pub enums:  Vec<CftSchemaEnum>,
}

pub struct CftSchemaType {
    pub name:        String,
    pub parent:      Option<String>,
    pub is_abstract: bool,
    pub is_sealed:   bool,
    pub fields:      Vec<CftSchemaField>,
    pub annotations: Vec<CftAnnotation>,
}

pub struct CftSchemaField {
    pub name:        String,
    pub ty:          String,
    pub has_default: bool,
    pub annotations: Vec<CftAnnotation>,
}

pub struct CftSchemaEnum {
    pub name:     String,
    pub variants: Vec<CftSchemaEnumVariant>,
    pub annotations: Vec<CftAnnotation>,
}

pub struct CftSchemaEnumVariant {
    pub name:  String,
    pub value: i64,
    pub annotations: Vec<CftAnnotation>,
}

pub struct CftSchemaConst {
    pub name:  String,
    pub value: CftConstValue,
}
```

---

## 8. 错误阶段

编译阶段错误码用于 CLI、编辑器诊断和宿主集成。错误码必须稳定，机器逻辑不得依赖错误消息文本。

编译阶段分为四类：

| 阶段 | 说明 |
|------|------|
| `LEX` | 字符流到 token |
| `SYN` | token 到 AST |
| `SCHEMA` | 顶层符号、类型定义、继承、注解、默认值 |
| `TYPE` | `check` 表达式的静态名称解析和类型检查 |

`check` 的实际执行错误不属于编译阶段。特别是对 `null` 做字段访问、索引访问、大小比较或算术，只在 check 执行阶段报错；编译期不能因为 nullable 字段访问直接判错，否则会误伤 `item != null && item.id != ""` 这类安全访问写法。

### 8.1 编译阶段错误码

#### LEX

| 错误码 | 名称 | 含义 |
|--------|------|------|
| `CFT-LEX-001` | `UnexpectedCharacter` | 非法字符 |
| `CFT-LEX-002` | `InvalidStringEscape` | 非法字符串转义 |
| `CFT-LEX-003` | `UnterminatedString` | 字符串未闭合 |
| `CFT-LEX-004` | `InvalidIntLiteral` | 整数字面量非法或溢出 |
| `CFT-LEX-005` | `InvalidFloatLiteral` | 浮点字面量非法、溢出或非有限 |

#### SYN

| 错误码 | 名称 | 含义 |
|--------|------|------|
| `CFT-SYN-001` | `UnexpectedToken` | 遇到不期望的 token |
| `CFT-SYN-002` | `UnexpectedEof` | 文件意外结束 |
| `CFT-SYN-003` | `ExpectedIdentifier` | 需要标识符 |
| `CFT-SYN-004` | `ExpectedToken` | 缺少固定 token，如 `;`、`}` |
| `CFT-SYN-005` | `InvalidTopLevelItem` | 顶层只能出现 `const`、`enum`、`type` |
| `CFT-SYN-006` | `InvalidChainComparison` | 链式比较方向不一致或使用 `!=` |
| `CFT-SYN-007` | `CheckBlockMustBeLast` | `check` 块后又出现字段声明 |
| `CFT-SYN-008` | `InvalidAnnotationSyntax` | 注解语法非法 |
| `CFT-SYN-009` | `InvalidCheckStatement` | `check` 块内不是合法条件语句、量词块或 `when` 块 |
| `CFT-SYN-010` | `DuplicateCheckBlock` | 同一个 `type` 内声明了多个 `check` 块 |

#### SCHEMA

| 错误码 | 名称 | 含义 |
|--------|------|------|
| `CFT-SCHEMA-001` | `DuplicateModule` | 重复注册同一 `ModuleId` |
| `CFT-SCHEMA-002` | `DuplicateGlobalName` | `const`、`enum`、`type` 全局重名 |
| `CFT-SCHEMA-003` | `DuplicateFieldName` | 同一 `type` 内字段重名 |
| `CFT-SCHEMA-004` | `DuplicateEnumVariant` | 同一 `enum` 内变体名重名 |
| `CFT-SCHEMA-005` | `DuplicateEnumValue` | 同一 `enum` 内整数值重名 |
| `CFT-SCHEMA-006` | `UnknownNamedType` | 字段类型引用未知的 `type` 或 `enum` |
| `CFT-SCHEMA-007` | `ParentMustBeType` | 父类引用的名称不是 `type` |
| `CFT-SCHEMA-008` | `UnknownConst` | 默认值引用未知 `const` |
| `CFT-SCHEMA-009` | `InheritanceCycle` | 继承循环 |
| `CFT-SCHEMA-010` | `InheritSealedType` | 继承 `sealed type` |
| `CFT-SCHEMA-011` | `DuplicateInheritedField` | 子类声明了父类任意层级已有字段 |
| `CFT-SCHEMA-012` | `ConflictingTypeModifiers` | `abstract` 和 `sealed` 同时使用 |
| `CFT-SCHEMA-013` | `MultipleIdFieldsInTree` | 同一继承树内存在多个 `@id` 字段 |
| `CFT-SCHEMA-014` | `InvalidDictKeyType` | 字典 key 不是 `string`、`int` 或 `enum` 类型 |
| `CFT-SCHEMA-015` | `InvalidDefaultExpression` | 默认值不是编译期常量 |
| `CFT-SCHEMA-016` | `DefaultTypeMismatch` | 默认值类型与字段类型不匹配 |
| `CFT-SCHEMA-017` | `DefaultReferencesField` | 默认值引用了字段或对象运行期值 |
| `CFT-SCHEMA-018` | `InvalidEnumValueSequence` | 枚举自动编号溢出或无法继续编号 |
| `CFT-SCHEMA-019` | `InvalidFlagEnumValue` | `@flag` 变体值不是 2 的幂 |
| `CFT-SCHEMA-020` | `UnknownAnnotation` | 未知注解名称 |
| `CFT-SCHEMA-021` | `DuplicateAnnotation` | 同一目标重复使用不允许重复的注解 |
| `CFT-SCHEMA-022` | `AnnotationWithoutTarget` | 注解后没有可附加的 `type`、`enum` 或字段 |
| `CFT-SCHEMA-023` | `InvalidAnnotationTarget` | 注解用在不支持的目标上 |
| `CFT-SCHEMA-024` | `InvalidAnnotationArgument` | 注解参数数量或类型错误 |
| `CFT-SCHEMA-025` | `InvalidAnnotatedFieldType` | `@id`、`@ref`、`@index` 字段类型不合法 |
| `CFT-SCHEMA-026` | `StructRequiresSealedType` | `@struct` 标注的 `type` 不是 `sealed type` |
| `CFT-SCHEMA-027` | `RefTargetMustBeType` | `@ref(TypeName)` 的目标不是已定义的 `type` |
| `CFT-SCHEMA-028` | `EnumVariantOnNonEnum` | 默认值使用 `Name.Variant`，但 `Name` 不是 `enum` |
| `CFT-SCHEMA-029` | `UnknownEnumVariant` | 默认值引用未知枚举变体 |
| `CFT-SCHEMA-030` | `InvalidConstValue` | `const` 值不是允许的字面量类型 |

`@ref` 字段类型允许 `string`、`int` 以及对应 nullable 形式；`@index` 字段类型只允许非 nullable 的 `string`、`int`、`enum`。

#### TYPE

| 错误码 | 名称 | 含义 |
|--------|------|------|
| `CFT-TYPE-001` | `UnknownValueName` | `check` 表达式引用未知字段、量词变量、`const` 或枚举名称 |
| `CFT-TYPE-002` | `UnknownField` | 字段访问的目标类型中不存在该字段 |
| `CFT-TYPE-003` | `UnknownEnumVariant` | `check` 表达式引用未知枚举变体 |
| `CFT-TYPE-004` | `EnumVariantOnNonEnum` | `check` 表达式使用 `Name.Variant`，但 `Name` 不是 `enum` |
| `CFT-TYPE-005` | `OperatorTypeMismatch` | 运算符不支持操作数类型 |
| `CFT-TYPE-006` | `ComparisonTypeMismatch` | 不可比较类型，如 `enum` 与 `int` |
| `CFT-TYPE-007` | `ConditionMustBeBool` | `check` 条件、`when` 条件或量词块条件结果不是 `bool` |
| `CFT-TYPE-008` | `UnknownFunction` | 未知内建函数 |
| `CFT-TYPE-009` | `FunctionArityMismatch` | 函数参数数量错误 |
| `CFT-TYPE-010` | `FunctionArgTypeMismatch` | 函数参数类型错误 |
| `CFT-TYPE-011` | `FieldAccessOnNonObject` | 对非对象做字段访问 |
| `CFT-TYPE-012` | `IndexOnNonIndexable` | 对非 array/dict 做索引访问 |
| `CFT-TYPE-013` | `IndexTypeMismatch` | array index 不是 `int`，或 dict key 类型不匹配 |
| `CFT-TYPE-014` | `InvalidIsPredicate` | `is` 目标不是 `type` 或 `null` |
| `CFT-TYPE-015` | `QuantifierRequiresCollection` | `all`、`any`、`none` 的目标不是 array/dict |
| `CFT-TYPE-016` | `UniqueUnsupportedElementType` | `unique` 的元素类型不支持 |
| `CFT-TYPE-017` | `BitwiseRequiresIntOrFlagEnum` | 位运算类型非法 |
| `CFT-TYPE-018` | `ShiftRequiresInt` | `<<`、`>>` 操作数不是 `int` |
| `CFT-TYPE-019` | `RegexPatternMustBeLiteral` | `matches` 的 pattern 不是字符串字面量 |
| `CFT-TYPE-020` | `InvalidRegexPattern` | `matches` 的正则 pattern 无法编译 |

编译诊断应包含错误码、阶段、消息、主位置和相关位置。重复定义、继承冲突、`@id` 冲突等错误必须用相关位置指向首次定义或冲突来源。

**`add_module` 阶段（注册时立即报错）：**

| 错误 | 原因 |
|------|------|
| 词法错误、语法错误 | 源文件格式非法 |
| 重复模块 | 已注册同一 `ModuleId` |

**`compile` 阶段（所有模块注册完成后统一报错）：**

| 错误 | 原因 |
|------|------|
| 全局名称重复 | `const`、`enum`、`type` 重名 |
| 子类字段与父类重名 | 子类声明了与任意父类同名的字段 |
| 继承循环 | `A : B`，`B : A` |
| `abstract` + `sealed` 同时使用 | 修饰符互斥 |
| `@struct` 标注在非 `sealed type` 上 | 注解范围违反 |
| `@flag` 变体值不是 2 的幂 | 注解约束违反 |
| `@ref(TypeName)` 引用未定义的类型 | 注解参数非法 |
| 同一继承树标注多个 `@id` 字段 | 注解约束违反 |
| 注解使用范围或字段类型不匹配 | 注解范围违反 |

**check 执行阶段：**

| 错误码 | 名称 | 原因 |
|--------|------|------|
| `CFD-CHECK-001` | `CheckFailed` | check 表达式求值结果为 false |
| `CFD-CHECK-002` | `CheckEvalTypeError` | 执行期类型错误，例如对不支持的类型使用运算符或函数 |
| `CFD-CHECK-003` | `CheckNullAccess` | 对 `null` 做字段访问、索引访问、大小比较或算术 |
| `CFD-CHECK-004` | `CheckIndexOutOfBounds` | 数组索引越界 |
| `CFD-CHECK-005` | `CheckMissingDictKey` | 字典 key 不存在 |
| `CFD-CHECK-006` | `CheckEmptyMinMax` | `min` / `max` 对空数组或无非 `null` 值的数组调用 |
| `CFD-CHECK-007` | `CheckInvalidRegex` | `matches` 的正则 pattern 在执行期无法编译 |

---

## 9. 综合示例

```cft
const MAX_LEVEL  = 100;
const MAX_ATTACK = 999;
const MIN_SPEED  = 0.1;

@flag
enum Permission {
  Read    = 1,
  Write   = 2,
  Execute = 4,
}

enum Rarity {
  Common = 0,
  Rare   = 10,
  Epic   = 20,
}

enum DamageType {
  Physical,
  Fire,
  Ice,
}

@struct
sealed type Vector2 {
  x: float;
  y: float;
}

type Stats {
  hp:     int;
  attack: int;
  speed:  float = 1.0;

  check {
    hp > 0;
    0 <= attack <= MAX_ATTACK;
    speed >= MIN_SPEED;
  }
}

@display("物品")
type Item {
  @id
  id: string;

  @display("名称")
  name: string;

  rarity: Rarity = Rarity.Common;
  tags:   [string] = [];

  check {
    id != "";
    name != "";
    matches(id, "^[a-z][a-z0-9_]*$");
    none tag in tags { tag == ""; }
  }
}

abstract type Reward {
  @id
  id: string;

  check { id != ""; }
}

type ItemReward : Reward {
  @ref(Item)
  item_id: string;

  count: int = 1;

  check { count > 0; }
}

type CurrencyReward : Reward {
  amount: int;

  check { amount > 0; }
}

type DropTable {
  rewards: [Reward];
  weights: [int];

  check {
    len(rewards) == len(weights);
    len(rewards) > 0;
    sum(weights) == 100;
    min(weights) >= 0;
    any reward in rewards { reward is CurrencyReward; }
  }
}

@display("怪物")
type Monster {
  @id
  id: string;

  @display("名称")
  name: string;

  @index
  rarity: Rarity;

  level:       int;
  stats:       Stats;
  drops:       DropTable;
  boss_drop:   Item? = null;
  resistances: {DamageType: float};
  skill:       Skill? = null;

  check {
    id != "";
    name != "";
    1 <= level <= MAX_LEVEL;
    stats.hp > 0;
    rarity >= Rarity.Common;
    contains(resistances, DamageType.Fire);

    when boss_drop != null {
      boss_drop.rarity >= Rarity.Rare;
    }

    all entry in resistances {
      0.0 <= entry.value <= 1.0;
    }
  }
}

type Skill {
  id:         string;
  is_passive: bool;
  cooldown:   float? = null;
  range:      float? = null;

  check {
    id != "";
    when !is_passive {
      cooldown != null;
      cooldown > 0.0;
    }
    when is_passive {
      range != null;
      range > 0.0;
    }
  }
}
```
