# Coflow 基础语言设计

> 状态：内部设计契约
>
> 范围：CFT 声明、CFD 数据与函数共享的语言语义。

源码排版及 CFD 对语义值的文本表示见 `04-source-formatting.zh-CN.md`。公开语法说明位于
`website/docs/docs/reference/03-language/`；加载、Module 和 Host 边界见
`02-api-runtime-design.zh-CN.md`；寄存器 VM 见 `03-vm-design.zh-CN.md`。

## 1. 设计边界

- CFT 定义类型、字段、默认值、函数签名、check 和注解。
- CFD 提供记录值，并可覆盖 CFT 中普通函数字段的默认实现。
- Rust Runtime 完成声明编译、数据实体化、函数编译、链接和执行。
- C# Runtime 加载 Rust 生成的契约与数据，通过 FFI 访问值、调用函数并绑定 Host。
- 数据值与函数值共享一套静态类型；发布后的 Runtime 快照不可变。
- 类型错误在快照发布前诊断，执行期只处理动态 fault。

## 2. 名称与作用域

type、enum、constant、type alias 和命名 check 使用项目全局唯一的短名；源文件和目录不创建
命名空间。类型位置只接受短名，`::` 用于 enum 静态成员、记录引用以及内建函数路径。

局部变量、参数和匿名函数捕获属于函数作用域。同一作用域不能重复声明局部名称，内层 block 可以
遮蔽外层局部值。参数名不参与函数类型相等性或调用 ABI。字段和函数 identity 由所属类型、记录 key
与字段名共同确定。

## 3. 类型系统

| 类别 | 类型 |
| --- | --- |
| 无返回值 | `()` |
| 标量 | `int`、`float`、`bool`、`string` |
| 名义类型 | enum、data、table、singleton |
| 集合 | `[T]`、`{K: V}` |
| 可选 | `T?` |
| 函数 | `fn(A...) -> R` |
| 模板 | `fstring` |

- `int` 是有符号 64 位整数，`float` 是 IEEE 754 binary64。
- enum 是名义类型；不同 enum 的底层整数相同也不兼容。
- data 支持 sealed 与继承；table 是有稳定 key 的记录类型；singleton 每个类型只有一个值。
- `@struct` 只适用于无继承的 sealed data，并影响 C# 值类型生成。
- list 元素和 dictionary key/value 是不变类型；dictionary key 只允许 string、int、bool 和 enum。
- `T?` 表示单层可选类型，不允许 `T??`；非空值使用 `T` 本身，不存在 `Some(...)` 构造语法。
- 函数类型由参数类型序列和返回类型组成，不支持用户泛型、协变或逆变。
- 不执行隐式数值、字符串、enum 或可选类型转换。

## 4. 数据值与配置边界

普通 CFD 字段只接受 schema 引导的结构化值。默认值、引用、继承字段和集合在候选快照构建阶段
解析，不在首次读取时延迟求值。

- 可选值是 `None` 或直接的非空值；writer 对非空值规范写回裸值。
- 记录引用按声明的目标记录域和 key 解析，不能退化为普通 string。
- inline object 与 record reference 是不同值类别。
- list 保留源顺序；dictionary key 必须唯一。
- object 可通过可选、list、dictionary 和记录引用形成有限递归结构；必填 object 环与默认值物化环
  在 schema 编译阶段拒绝。
- 普通配置字段不执行任意算术、控制流或 Host 调用。

## 5. 函数、模板与 Host

CFT 函数字段可以声明默认 body；CFD 为同一字段提供函数值时覆盖默认实现。函数默认值只直接用于
函数字段，不嵌套在其他默认值中。`@Host` singleton 的函数由应用绑定，CFT 和 CFD 均不提供实现。

Rust Runtime 对所有有效函数体进行类型检查并编译为寄存器 VM 程序。直接调用、间接调用和 Host 调用
使用同一静态签名。匿名函数按值捕获实际使用的外层局部值；捕获分析在编译期完成，不提供可变
upvalue。递归调用受统一执行限制约束。

fstring 是独立的模板值类型。模板在读取时由 Rust Runtime 求值；普通 string 中的花括号没有插值
语义。复制函数或模板值不改变其对象绑定。

## 6. 表达式与控制流

函数 body 是有类型的表达式。block 包含零个或多个 statement，并可由最后一个未加分号的表达式产生
block 值。核心能力包括字面量、局部读取、字段和索引读取、集合与对象构造、运算、调用、匿名函数、
block、`return`、`if`、`while`、`for`、类型测试和可选值传播。

局部变量通过 `var` 引入，声明后类型固定。赋值只允许写入可变局部变量，不允许写参数、配置字段、
集合元素或闭包捕获。显式 `return` 立即结束当前函数；所有可到达出口必须产生声明的返回类型。

`if` 条件必须为 bool。有值分支必须产生相同静态类型；缺少 `else` 时只能作为 `()`。条件中的
`value is Some(name)` 解包非空可选值，`value is TypeName` 完成名义类型收窄。语言不提供 `match`。

`while` 每轮重新计算条件。`for` 可遍历整数区间、list、dictionary 和记录集合。循环结果为 `()`，
`break` 与 `continue` 只影响最近循环。循环、调用和集合操作统一计入 Runtime 执行预算。

## 7. 可选值传播

表达式 `value?` 要求 `value` 为 `T?`，当前函数返回类型也必须为可选类型。非空时表达式继续产生
`T`；为 `None` 时当前函数立即返回 `None`。传播一次只处理一层，不执行隐式类型转换。

## 8. 运算与内建

运算符由静态类型决定，不进行运行时重载搜索。整数使用 checked 64 位运算；浮点遵循 binary64；
字符串比较使用 ordinal 语义；逻辑运算保持短路。位运算只接受 int 或 `@flag` enum。

内建方法由编译器静态解析，包括 string 查询、集合长度与关系、dictionary key/value 查询、数值操作，
以及 map、filter、fold、any 和 all。高阶内建必须验证回调签名，全部工作量计入 VM 预算。

## 9. Fault 语义

整数溢出、除零、非法转换、越界索引、缺失函数、Host 异常、错误 Runtime 代际以及执行预算超限均为
运行时 fault。fault 携带函数、来源路径、表达式 span 和精简调用栈，不暴露寄存器快照。

## 10. 固定不支持

- `Result<T, E>`、`Ok(...)`、`Err(...)` 和异常捕获语法。
- `Some(...)` 值构造、嵌套可选类型和 `match`。
- 用户泛型、隐式数值转换和运行时类型声明。
- 可变配置对象、可变集合和可变闭包捕获。
- coroutine、`await`、`yield`、Task、continuation 或 scheduler。
- 动态字段访问、按字符串调用函数、宏和运行时代码加载。
