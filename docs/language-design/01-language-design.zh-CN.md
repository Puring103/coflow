# C# Runtime 基础语言设计

> 状态：当前内部设计契约
>
> 范围：C# Runtime 所编译的 CFD 函数语言，以及数据值与函数值共享的基础语义。

当前实现已经覆盖本文描述的类型、控制流、函数、closure、传播和高阶集合语义。第 11 节列出的执行
限制 fault 仍属于 VM 待实现契约；在此之前，函数语言不能作为不可信脚本 sandbox。源码排版及 CFD
对语义值的文本表示见 `04-source-formatting.zh-CN.md`。

本文档定义语言语义，不提供用户教程或调用示例。公开的 CFT、CFD 语法说明位于
`website/docs/docs/reference/03-language/`。加载、Module 和 Host 边界见
`02-api-runtime-design.zh-CN.md`；寄存器与执行机制见 `03-vm-design.zh-CN.md`。

## 1. 设计目标

基础语言必须满足：

- CFT 在构建期定义类型、字段、默认值、函数签名和注解。
- CFD 在运行期提供记录值，并可覆盖 CFT 中的普通函数默认实现。
- 数据值与函数值使用同一套静态类型，不在 VM 中建立第二套语义类型。
- `Load` 可以只读取数据；`LoadAndCompile` 才检查并编译函数体。
- 类型错误在 Module 发布前诊断，执行期只处理动态 fault。
- 语言行为不依赖 C# 反射顺序、文化区域、CLR 对象地址或容器具体实现。

编译关系为：

```text
CFT
├── type declarations
├── field declarations
├── function signatures
├── constants and defaults, including direct function defaults
└── annotations

CFD
├── record values
├── inline values
├── record references
└── function bodies
```

## 2. 名称与作用域

type、enum、constant 和函数签名属于 CFT 声明空间；record key、局部变量、参数和匿名函数捕获属于
CFD 值或函数作用域。

名称解析遵循以下边界：

- canonical type identity 包含 namespace 和类型名。
- `namespace` 与显式 `use` 决定非限定名称的解析，不执行模糊 fallback。
- 同一作用域不能重复声明局部名称；内层 block 可以遮蔽外层局部值。
- 参数名不参与函数类型相等性或调用 ABI。
- 字段和函数 identity 由所属 canonical type、record key 与字段名共同确定。
- `$type`、`$field`、`$id` 等编译期元数据只能在其定义的函数上下文使用。

## 3. 类型系统

Runtime 函数语言使用以下静态类型类别：

| 类别 | 类型 |
| --- | --- |
| unit | `()` |
| scalar | `int`、`float`、`bool`、`string` |
| nominal | enum、生成 object type、记录类型 |
| collection | `[T]`、`{K: V}` |
| algebraic | `Option<T>`、`Result<T, E>` |
| callable | `fn(A...) -> R` |

类型规则如下：

- `int` 是有符号 64 位整数，`float` 是 IEEE 754 binary64。
- `bool` 只有 `true` 和 `false` 两个规范值。
- enum 是 nominal type；不同 enum 即使底层整数相同也不兼容。
- object 继承只允许子类型用于声明的父类型位置，不提供用户定义隐式转换。
- list 元素类型、dictionary key/value 类型和 Option/Result payload 都是不变的静态类型参数。
- dictionary key 只允许语言明确支持的可哈希 scalar 或 enum 类型。
- 函数类型由参数类型序列和返回类型组成，不支持协变、逆变或用户泛型函数。
- 不执行隐式 int/float、string/enum、nullable/Option 转换。

值的逻辑形状为：

```text
ValueShape
├── Unit
├── Scalar(T)
├── Object(T)
├── List(T)
├── Dictionary(K, V)
├── Option
│   ├── tag
│   └── payload(T)
├── Result
│   ├── tag
│   ├── ok payload(T)
│   └── error payload(E)
└── Function
    ├── signature
    └── callable identity or closure
```

## 4. 数据值与配置边界

普通 CFD 字段只接受 schema-guided 的结构化值。默认值、引用、继承字段和集合在 Module 构建阶段
解析，不在字段首次读取时延迟求值。

数据值遵循：

- `Option<T>` 的语义值由缺失/存在 tag 和 `T` payload 构成。函数表达式使用 `None` / `Some(T)`；
  schema-guided CFD 对存在值允许直接写 `T`，writer 也规范写回裸值。
- `Result<T, E>` 只通过 `Ok(T)` 或 `Err(E)` 表达业务结果。
- 记录引用按声明的目标记录域和 key 解析，不能退化为普通 string。
- inline object 与 record reference 是不同值类别。
- list 保留源顺序；dictionary key 必须唯一。
- 发布后的数据不可变，函数执行不能修改配置记录或集合。
- 普通配置字段不执行任意算术、控制流或 Host 调用。

## 5. 函数声明与实现

CFT 声明函数签名，普通函数字段可以同时声明默认 body；CFD 提供同字段函数值时覆盖该默认实现。
函数默认值只允许直接用于函数字段，不嵌套在其他默认值中。`@Host @singleton` 的函数由应用配置，
CFT 不能声明默认 body，CFD 也不能为其提供实现。Rust 数据模型保存函数源码但不提供执行引擎；C#
Runtime 的 `Load` 只构建并保留函数值，`LoadAndCompile` 才对所有有效函数体进行类型检查和编译。

函数值具有统一语义：

```text
CallableValue
├── Signature
├── DirectFunction
│   └── function identity
├── Closure
│   ├── function identity
│   └── captured values
└── HostFunction
    └── configured delegate
```

直接调用、间接调用和 Host 调用必须使用同一静态签名。无捕获函数可以直接作为函数值；匿名函数按值
捕获实际使用的外层局部值。捕获分析在编译期完成，不提供可变 upvalue 或通用闭包 GC。

普通递归、相互递归和通过函数值的递归均允许。是否降低为尾调用由控制流位置决定，不改变语言结果。

## 6. 表达式与 block

函数 body 是有类型的表达式。block 包含零个或多个 statement，并可由最后一个未加分号的表达式产生
block 值。

核心表达式类别为：

```text
Expression
├── literal and metadata
├── local and argument read
├── field and index read
├── object, list and dictionary construction
├── Option and Result construction
├── unary and binary operation
├── conversion and type test
├── direct and indirect call
├── anonymous function
├── block and explicit return
├── if and match
├── while and for
└── propagation
```

局部变量通过 `var` 引入，声明后类型固定。赋值只允许写入可变局部变量，不允许写参数、配置字段、集合
元素或闭包捕获。复合赋值先按普通运算符完成类型检查，再写回同一局部变量。

显式 `return` 立即结束当前函数。一个可到达函数出口的路径必须产生声明返回类型；已经终止的路径不再
参与后续表达式的类型合并。

## 7. 控制流

`if` 条件必须是 bool。两个有值分支必须产生相同静态类型；缺少 `else` 时整个表达式只能作为 unit。

`match` 在编译期检查 pattern 与被匹配值的类型，并要求表达式位置上的分支穷尽。支持的 pattern 类别
包括 literal、bool、enum、Option、Result、类型 pattern、绑定 pattern 和 catch-all。pattern 绑定只在
对应 arm 内可见。

循环语义如下：

- `while` 每轮重新计算 bool 条件。
- range `for` 按整数顺序迭代，并明确区分开区间终点与闭区间终点。
- list `for` 按稳定整数顺序读取元素。
- dictionary `for` 使用当前只读集合提供的稳定枚举快照。
- `break` 和 `continue` 只在最近循环内有效。
- 循环表达式结果是 unit；从循环返回函数值必须使用 `return`。

循环和高阶集合操作最终必须受 VM 工作量预算约束，语法层不提供绕过执行限制的迭代入口。该预算当前
尚未实现，状态见 VM 设计第 13 节。

## 8. Option、Result 与传播

Option 和 Result 是语言值，不是异常协议。构造、匹配和传播都保留完整静态 payload 类型。

传播规则为：

```text
Option<T> propagation
├── Some(value) -> continue with value
└── None        -> return None

Result<T, E> propagation
├── Ok(value)   -> continue with value
└── Err(error)  -> return Err(error)
```

传播源与当前函数返回类型必须具有相同外层构造；Result 的 error 类型必须一致。嵌套传播每次只处理一层
tag，不隐式扁平化不同的 Option/Result 组合。

## 9. 运算与转换

运算符由 operand 静态类型决定，不执行运行时重载搜索。

| 类别 | 语义约束 |
| --- | --- |
| 整数算术 | checked 64 位运算；溢出与除零产生 fault |
| 浮点算术 | IEEE 754 binary64；不隐式转为整数 |
| 字符串连接 | 只接受 string operand |
| 比较 | 两侧类型必须兼容，string 使用 ordinal 语义 |
| 相等 | 遵循对应生成类型或集合的语言相等语义 |
| 位运算 | 只接受 int 或语言允许的 flag enum |
| 移位 | 左值和位数均为 int；非法位数与溢出语义必须由统一语言规则确定 |
| 逻辑运算 | 只接受 bool，并保持短路求值 |
| 显式转换 | 只允许语言注册的转换，失败产生 source-mapped fault |

常量折叠必须与运行时执行使用相同语义。编译器不能因 CLR 运算符的掩码、舍入或文化区域规则导致常量
表达式与非恒定表达式结果不同。

## 10. 内建方法

内建方法是编译器理解的静态操作，不是通过反射发现的普通成员。内建集合方法可以具有编译器提供的
泛型签名，但用户函数不能声明泛型参数。

内建方法分为：

- string 长度、包含、前后缀、空白与正则匹配。
- list 长度、包含、唯一性、排序性、聚合与集合关系。
- dictionary 长度、key/value 查询和只读 keys/values。
- int/float 的绝对值、平方根、有限性和近似比较。
- list 的 map、filter、fold、find、any 和 all。

高阶内建必须验证 callback 签名；`find` 返回 Option，空集合上的 fold 使用显式初值，any/all 使用各自
的空集合恒等值。执行限制落地后，线性内建的元素工作量必须计入 VM 预算，不能以单次 native 调用隐藏
无界扫描。

## 11. Fault 语义

业务失败通过 Result 返回。以下情况属于运行时 fault：

- 整数溢出、除零和非法转换。
- 无效索引或不满足语言前置条件的内建操作。
- 未配置、签名不匹配或抛出未处理异常的 Host function。
- 无效间接调用目标或 VM 状态不变量破坏。
- 指令、frame、寄存器或集合工作量超限（待 VM 执行限制实现）。

fault 必须绑定当前函数、来源路径、产生故障的表达式 span 和精简 Coflow 调用栈。fault 不暴露寄存器
快照，也不转换为语言 Result。

## 12. 固定不支持的语言能力

当前函数语言不支持：

- 用户泛型、隐式数值转换和异常捕获语法。
- 可变配置对象、可变集合和可变 closure capture。
- coroutine、`await`、`yield`、Task、continuation 或 scheduler。
- 动态字段访问、按字符串调用函数或运行时类型声明。
- 宏、运行时代码加载和可序列化 bytecode。
