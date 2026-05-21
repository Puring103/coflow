# coflow 表达式

## 字面量

### 整数字面量

十进制整数：

```coflow
100
1_000_000
```

带进制前缀的整数：

```coflow
0xff         # 十六进制
0xFF         # 十六进制（字母大小写均可）
0b1010_0101  # 二进制
0o755        # 八进制
```

数字分隔符 `_` 的规则：
- 只能出现在两个合法数字字符之间
- 不能出现在数字开头、结尾，不能连续出现（`__`）
- 带进制前缀时，前缀后至少需要一个合法数字

负号不是整数字面量的一部分，`-1` 解析为一元负号 `-` 加整数字面量 `1`。

### 浮点字面量

```coflow
3.5
1_000.000_5
1e3
1.0e-3
1E+3
```

规则：
- 小数点后必须紧跟数字或 `_`（`1.` 不合法，`.5` 不合法）
- 科学计数法：`e` 或 `E` 后可选 `+`/`-`，之后必须有至少一个数字
- `_` 只能出现在两个数字之间，不能出现在小数点两侧（`1_.0` 和 `1._0` 均非法）

### 布尔字面量

```coflow
true
false
```

### null 字面量

```coflow
null
```

### 字符串字面量

**普通字符串**：双引号包围，支持转义序列，不能跨行。

```coflow
"hello"
"line one\nline two"
"path: C:\\game"
```

支持的转义序列：

| 转义 | 含义 |
|------|------|
| `\"` | 双引号 `"` |
| `\\` | 反斜杠 `\` |
| `\n` | 换行符 LF |
| `\r` | 回车符 CR |
| `\t` | 水平制表符 |

普通字符串内出现未转义的换行符是词法错误。

**原始字符串**：以 `r"` 开始，以 `"` 结束，不处理转义序列，反斜杠视为普通字符，不能跨行。

```coflow
r"C:\game\assets\hero.png"
r"pattern: \d+"
```

**多行字符串**：以 `"""` 开始，以 `"""` 结束，保留内容中的换行，支持转义序列（与普通字符串相同）。

```coflow
var text = """
line one
line two
""";
```

**原始多行字符串**：以 `r"""` 开始，以 `"""` 结束，不处理转义序列。

```coflow
var shader = r"""
float4 main() {
  return float4(1, 0, 0, 1);
}
""";
```

### 数组字面量

```coflow
[1, 2, 3]
["a", "b", "c"]
[]
```

元素之间用逗号分隔，允许末尾逗号。

### 对象/字典字面量

使用 `{ }` 语法。字段/条目之间用逗号分隔，允许末尾逗号。

```coflow
# 对象（标识符 key）
{ id: "sword", damage: 10 }

# 字典（字符串 key）
{ "alice": 10, "bob": 20 }
```

对象字面量支持 `...` 展开：

```coflow
var base = { damage: 10, speed: 1.0 };
var sword = { ...base, name: "Iron Sword", damage: 15 };
```

展开可以出现在字段列表的任意位置，后面的同名字段覆盖前面的。

对象与字典的消歧规则见 [02-types.md](./02-types.md)。

## 名称

标识符引用当前作用域中的变量、函数或参数：

```coflow
hp
player
count
```

带 `.` 分隔的路径用于访问模块成员或枚举变体：

```coflow
Rarity.common
weapons.sword
game.utils.helper
```

## 函数表达式

匿名函数使用 `fn` 关键字，语法与顶层 `fn` 声明相同，但没有名称：

```coflow
fn(x) => x * 2

fn(name: string) -> string {
  return "hello " + name;
}

fn(a: int, b: int = 0) -> int => a + b
```

`iter fn` 匿名表达式：

```coflow
var gen = iter fn() {
  yield 1;
  yield 2;
};
```

## Lambda 表达式

Lambda 使用 `(params) => body` 语法，不需要 `fn` 关键字：

```coflow
(x) => x * 2
(x, y) => x + y
() => 42
```

Lambda 参数可以有类型标注、默认值和返回类型标注：

```coflow
(x: int, y: int) -> int => x + y
(name: string = "hero") => "hello " + name
```

Lambda 体可以是单个表达式或块：

```coflow
(x) => x * 2

(x) => {
  var result = x * 2;
  return result;
}
```

## if 表达式

`if` 作为表达式时，两个分支必须都存在，且每个分支是用 `{ }` 包裹的单个表达式：

```coflow
if condition { then_expr } else { else_expr }
```

示例：

```coflow
var label = if hp > 0 { "alive" } else { "dead" };
var abs_val = if x >= 0 { x } else { -x };
```

if 表达式没有 `else if` 形式（需要嵌套）。if 表达式可以出现在任何需要值的位置。

与 if **语句**的区别：if 语句允许 `else if` 链、分支体是语句块，且不产生值；if 表达式必须有 `else`、每个分支是单个表达式并产生值。

## 一元运算符

所有前缀一元运算符优先级相同，高于所有二元运算符：

| 运算符 | 含义 | 示例 |
|--------|------|------|
| `-`    | 取负 | `-hp` |
| `not`  | 逻辑非 | `not alive` |
| `~`    | 按位取反 | `~mask` |

## 二元运算符与优先级

下表按优先级从低到高排列（同一行优先级相同）：

| 优先级（低→高） | 运算符 | 结合性 | 说明 |
|----------------|--------|--------|------|
| 1  | `or`  | 左结合 | 逻辑或 |
| 2  | `and` | 左结合 | 逻辑与 |
| 3  | `??`  | 右结合 | 空值合并 |
| 4  | `\|`  | 左结合 | 按位或 |
| 5  | `^`   | 左结合 | 按位异或 |
| 6  | `&`   | 左结合 | 按位与 |
| 7  | `==` `!=` `<` `<=` `>` `>=` `in` `not in` | 不可结合 | 比较与成员判断 |
| 8  | `+` `-` | 左结合 | 加减 / 字符串拼接 |
| 9  | `<<` `>>` | 左结合 | 位移 |
| 10 | `*` `/` `//` `%` | 左结合 | 乘除取余 |
| 11 | `**` | 右结合 | 幂运算 |
| 12 | `-` `not` `~`（前缀） | — | 一元前缀 |
| 13 | `()` `.` `?.` `[]` `?[]`（后缀） | 左结合 | 调用与访问 |

### 逻辑运算

```coflow
if alive and not dead {
  update();
}

var name = input or "unknown";
```

`and` 和 `or` 短路求值：
- `a and b`：若 `a` 为假则返回 `a`，否则返回 `b`（不求值 `a` 为假时的 `b`）
- `a or b`：若 `a` 为真则返回 `a`，否则返回 `b`（不求值 `a` 为真时的 `b`）

### 空值合并

`a ?? b`：若 `a` 不为 `null` 则返回 `a`，否则返回 `b`。`??` 是右结合：

```coflow
var name = player?.name ?? config.default_name ?? "unknown";
# 等价于：player?.name ?? (config.default_name ?? "unknown")
```

### 比较运算

支持**链式比较**，三个操作数的链式写法等价于两段比较用 `and` 连接：

```coflow
if 0 < damage <= 100 {
  apply(damage);
}
# 等价于：damage > 0 and damage <= 100
```

链式比较只支持两段（三个操作数）。

比较运算符不可结合，`a < b < c` 必须写成链式形式（不能写 `(a < b) < c` 期望得到布尔比较）。

### 位运算

```coflow
var flags = STATUS_DEAD | STATUS_STUNNED;
if (flags & STATUS_DEAD) != 0 {
  die();
}
var cleared = flags & ~STATUS_STUNNED;
```

注意位运算符优先级高于比较运算符，但低于加减和乘除。在比较表达式中混用位运算通常需要加括号：

```coflow
if (flags & MASK) != 0 { ... }    # 正确
if flags & MASK != 0 { ... }      # 错误：解析为 flags & (MASK != 0)
```

### 整数除法与幂运算

```coflow
var q = damage // armor;   # 截断整除，结果为 int
var area = radius ** 2;    # 幂运算
```

`**` 右结合：`2 ** 3 ** 2` 等价于 `2 ** (3 ** 2)` = 512。

### 成员判断

`in` 和 `not in` 判断元素是否属于集合：

```coflow
if item in items { ... }        # 数组成员判断
if key in scores { ... }        # 字典 key 判断
if hp in 1..=100 { ... }        # Range 范围判断
if status not in banned { ... } # 非成员判断
```

## 后缀操作

后缀操作的优先级最高，从左到右依次应用。

### 函数调用

```coflow
fn_name(arg1, arg2)
obj.method(arg1)
```

**具名参数**：调用时以 `param_name: value` 形式传入具名参数：

```coflow
spawn("goblin");
spawn("boss", hp: 500);
spawn("elite", 200, 1);
```

规则：具名参数必须与形参名一致；位置参数和具名参数可以混用；位置参数之后可以接具名参数，但具名参数之后不能再有位置参数。

### 字段访问

```coflow
obj.field
player.profile.name
```

访问不存在的字段返回 `null`。

### 可选字段访问

```coflow
obj?.field
```

若 `obj` 为 `null`，结果为 `null`，不抛错。可以链式使用：

```coflow
var name = player?.profile?.name ?? "unknown";
```

### 索引访问

```coflow
arr[0]
dict["key"]
```

数组越界或字典键不存在时返回 `null`。

### 可选索引访问

```coflow
arr?[0]
dict?["key"]
```

若左侧为 `null`，结果为 `null`，不抛错。

## Range 表达式

Range 产生可迭代的整数序列，只能用于 `for in` 或 `in` 判断：

```coflow
0..10    # [0, 10)，不含 10
0..=10   # [0, 10]，含 10
```

```coflow
for i in 0..10 { ... }

if hp in 1..=100 { ... }
```

Range 的起始和终止都是整数表达式：

```coflow
for i in start..(start + count) { ... }
```

## 括号分组

圆括号 `( )` 用于显式控制求值顺序：

```coflow
var result = (a + b) * c;
if (flags & MASK) != 0 { ... }
```
