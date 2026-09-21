# CFT 声明语言

CFT 定义数据类型、默认值、常量、维度和规则声明。`coflow cft check` 检查声明，
包括名称、类型、继承、默认值、函数、模板、check 和注解。程序体在契约生成时编译，由运行时按需执行。

```cft
namespace game;

enum Rarity { Common, Rare }
data Stats { hp: int = 100; }
table Item {
  name: string;
  rarity: Rarity = Rarity::Common;
  stats: Stats = Stats { hp: 100 };
  next: Item?;
  tags: [string] = [];
}
singleton Settings { title: string = "Game"; }
```

注释从 `#` 延续到行尾，标识符区分大小写。字段以分号分隔。

## 名称与类型声明

文件可先声明 `namespace game;`，再以 `use shared::Stats;` 导入名称。
跨命名空间使用完整限定名或显式导入的短名；文件路径不决定命名空间。

| 声明 | 用途 | 继承 |
| --- | --- | --- |
| `table Item { ... }` | 有 id 的记录 | 可继承 table |
| `singleton Settings { ... }` | 单例记录 | 可继承 table，本身不可继续派生 |
| `data Stats { ... }` | 内联数据对象 | 可继承 data |

`abstract` 类型不可直接构造，`sealed` 类型不可派生。记录不能内联构造，data 不能声明为记录。
字段类型直接写 `Item` 即表示记录，写 `Stats` 即表示内联数据。

`type Name = Type;` 声明类型别名。字段、常量和函数签名必须写明类型：

```cft
type ItemList = [Item];
const MAX_LEVEL: int = 100;
const DEFAULT_TAGS: [string] = ["common"];
type Callback = fn(value: int) -> int;
```

## 值类型

| 类型 | 写法 | 值示例 |
| --- | --- | --- |
| 32 位整数 | `int` | `42` |
| 32 位浮点 | `float` | `3.5`、`inf` |
| 布尔 | `bool` | `true` |
| 普通字符串 | `string` | `"text {braces}"` |
| 模板 | `fstring` | `f"名称：{self.name}"` |
| 枚举 | `Rarity` | `Rarity::Rare` |
| 内联数据 | `Stats` | `Stats { hp: 100 }` |
| 记录 | `Item` | `&Item::sword` |
| 数组 | `[T]` | `[1, 2]` |
| 字典 | `{K: V}` | `{ "hp": 100 }` |
| 可选值 | `T?` | `None` 或直接书写非空值 |
| 函数 | `fn(name: T) -> R` | 完整签名的函数字面量 |
| 无返回值 | `()` | 用于函数签名 |

字典 key 支持 string、int、bool 和 enum。可选类型不嵌套。
`(fn() -> int)?` 是可选函数，`fn() -> int?` 是返回可选整数的函数。
枚举值使用非负 32 位整数；`@flag` 枚举使用 32 位无符号位掩码，支持 `&`、`^`、`|`。

## 默认值与注解

字段可以使用 `field: Type = value;` 指定默认值。省略无默认值的可选字段得到 None，
其余无默认值字段必填。内联数据始终写出类型名，默认值展开必须有限。

普通字符串的花括号只是文本。模板必须起源于 `f"..."`，已有模板可按明确的 fstring 类型传递。
对象字段中直接声明的函数和模板绑定该对象，复制已有函数或模板不改变绑定。
函数、模板和 check 使用同一套静态类型检查与运行时执行机制。fstring 在普通读取时求值，check 仅在显式请求时执行。

| 注解 | 用途 |
| --- | --- |
| `@label`、`@description` | 显示名称与说明 |
| `@flag` | 位标志枚举 |
| `@struct` | 无继承的 sealed data 值类型 |
| `@Host` | 由宿主提供的 singleton 服务 |
| `@idAsEnum(Name)` | 将 table 的记录 key 映射到稳定枚举 |
| `@localized`、`@dimension("name")` | 记录字段的维度值 |

check 声明仅适用于记录和顶层规则，data 不声明 check。详见 [Check 校验](./check.md)。

## 局部构造

函数内可用 `build` 创建或修改 data、数组和字典，正常结束自动得到不可变值：

```cft
var values: [int] = build [int] as b {
  b.append(1);
  b.append(2);
};
var updated: [int] = build (values) as b {
  b[0] = 3;
  b.remove(1);
};
```

data 字段用 `b.field = value;` 赋值；所有正常出口须填完必填字段。字典覆盖保留顺序，删除后重新插入放在末尾。构造不会修改输入；嵌套值需单独构造后替换。构造绑定不能复制、捕获、返回或传入函数，记录与 singleton 不可动态构造。
