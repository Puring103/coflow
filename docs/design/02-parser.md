# coflow Parser 设计

## 概述

coflow parser 位于 `src/parser.rs`，将 token 流转换为抽象语法树（AST，定义在 `src/ast.rs`）。它是一个手写的递归下降解析器，对表达式部分使用 Pratt 解析（binding power）。

公开入口：

```rust
pub fn parse_module(source: &str) -> ParseOutput
```

parser 内部首先调用 `lex(source)`。若 lex 产生任何错误，`parse_module` 直接返回，不启动 parser，此时 `module` 为 `None`，`errors` 包含所有 lex 错误（包装为 `ParseErrorKind::Lex`）：

```rust
pub struct ParseOutput {
    pub module: Option<Module>,
    pub errors: Vec<ParseError>,
}
```

若 lex 无错误，parser 继续工作。即使 parse 阶段产生错误，`module` 仍然返回（可能不完整），以便工具链在有错误时仍能获得部分 AST。

## ParseError

```rust
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub span: Span,
}
```

`ParseErrorKind` 枚举：

| 变体 | 含义 |
|------|------|
| `Lex(LexErrorKind)` | 来自 lexer 的错误（转发） |
| `UnexpectedEof` | 意外到达文件末尾 |
| `UnexpectedToken` | 当前 token 不符合预期 |
| `ExpectedItem` | 期望顶层 item |
| `ExpectedType` | 期望类型表达式 |
| `ExpectedExpression` | 期望表达式 |
| `ExpectedIdentifier` | 期望标识符 |
| `ExpectedToken` | 期望特定 token |
| `InvalidAssignmentTarget` | 赋值目标非法（不是变量名、字段或下标） |
| `MissingCatch` | `try` 块后缺少 `catch` |
| `MissingSemicolon` | 语句缺少分号 |
| `UnsupportedParserNotImplemented` | 未实现的语法路径 |

## AST 结构概览

### 模块与顶层 Item

```
Module
└── items: Vec<Item>
    ├── Import(ImportDecl)       # import path.to.mod as alias;
    ├── Class(ClassDecl)         # class Foo { ... }
    ├── Enum(EnumDecl)           # enum Color { ... }
    ├── Function(FnDecl)         # fn foo(...) { ... }
    ├── Var(VarDecl)             # var x: T = expr;
    └── Config(ConfigDecl)       # name: T = expr;
```

`local` 修饰符（`local fn`、`local class`、`local var`）存储为对应声明节点的 `local: bool` 字段。

### 语句 Stmt

```
Stmt
├── Function(FnDecl)
├── Var(VarDecl)
├── Assign(AssignStmt)     # target op= value;
├── Expr(Expr)             # 表达式语句
├── If(IfStmt)
├── While(WhileStmt)
├── Until(UntilStmt)
├── Loop(LoopStmt)
├── ForIn(ForInStmt)
├── Break(Span)
├── Continue(Span)
├── Return(ReturnStmt)
├── Throw(ThrowStmt)
├── TryCatch(TryCatchStmt)
└── Yield(YieldStmt)
```

### 表达式 Expr

```
Expr
├── Literal(Literal)              # 字面量
├── Name(Ident)                   # 变量名
├── Array(ArrayLiteral)           # [a, b, c]
├── Record(RecordLiteral)         # {k: v} 或 {"k": v}
├── Fn(FnExpr)                    # fn(...) { } 匿名函数
├── Lambda(LambdaExpr)            # (x) => expr
├── Range(RangeExpr)              # a..b 或 a..=b
├── Unary(UnaryExpr)              # -x, not x, ~x
├── Binary(BinaryExpr)            # 二元运算
├── Call(CallExpr)                # f(a, b: c)
├── Field(FieldExpr)              # obj.field
├── OptionalField(OptionalFieldExpr) # obj?.field
├── Index(IndexExpr)              # obj[i]
├── OptionalIndex(OptionalIndexExpr) # obj?[i]
└── If(IfExpr)                    # if cond { a } else { b }
```

## 表达式解析：Pratt 解析器

表达式解析的核心函数为 `parse_expr_bp(min_bp: u8)`，基于 binding power（绑定力）实现算符优先级。函数遵循"前缀 → 后缀 → 中缀"的顺序：

1. 调用 `parse_prefix()` 解析原子表达式和前缀运算符
2. 在后缀循环中处理调用、字段访问、下标访问（优先级最高）
3. 在中缀循环中处理二元运算符，直到遇到绑定力低于 `min_bp` 的 token

### 完整运算符优先级（低到高）

| 优先级 | 运算符 | 结合性 | BinaryOp |
|--------|--------|--------|----------|
| 1/2 | `or` | 左结合 | `Or` |
| 3/4 | `and` | 左结合 | `And` |
| 5/5 | `??` | 右结合 | `NullCoalesce` |
| 6/7 | `\|` | 左结合 | `BitOr` |
| 8/9 | `^` | 左结合 | `BitXor` |
| 10/11 | `&` | 左结合 | `BitAnd` |
| 12/13 | `==` `!=` `<` `<=` `>` `>=` `in` `not in` | 不可结合 | `Eq` `NotEq` `Lt` `LtEq` `Gt` `GtEq` `In` `NotIn` |
| 14/15 | `+` `-` | 左结合 | `Add` `Sub` |
| 16/17 | `<<` `>>` | 左结合 | `Shl` `Shr` |
| 18/19 | `*` `/` `//` `%` | 左结合 | `Mul` `Div` `IntDiv` `Rem` |
| 21/20 | `**` | 右结合 | `Pow` |
| 25（前缀） | `-` `not` `~` | — | `Neg` `Not` `BitNot` |
| 最高（后缀） | `()` `.` `?.` `[]` `?[` | 左结合 | — |

右结合通过将 `right_bp` 设为比 `left_bp` 小 1 实现（`**` 为 `(21, 20)`，`??` 为 `(5, 5)`）。

`not in` 作为双 token 中缀运算符，在 `parse_expr_bp` 循环中特殊处理：检测到 `Not` 后紧跟 `In` 时，使用与 `In` 相同的绑定力。

### 链式比较

比较运算符（`==` `!=` `<` `<=` `>` `>=`）不可链式使用（left_bp == right_bp - 1，普通左结合逻辑下不能堆叠），但 parser 对三操作数的情形做了特殊扩展：当解析完一个比较表达式后，若下一个 token 仍是比较运算符，则将其展开为 `(a op1 b) and (b op2 c)` 形式的 `BinaryExpr`，其中 `b` 被求值两次（节点共享但求值语义由 sema 处理）。

### 范围表达式

`..` 和 `..=` 不在 `infix_binding_power` 表中，而是通过 `try_parse_range` 单独处理，仅在 `min_bp == 0`（即顶层表达式）时尝试解析，产出 `RangeExpr { inclusive: bool }`。

## 分号规则

coflow 使用**显式分号**，以下语句需要分号结尾（通过 `expect_semicolon()` 强制）：

- `import` 声明
- `var` 声明
- 配置定义（顶层 `name = value`）
- 赋值语句（`target op= value`）
- 表达式语句
- `break` / `continue` / `return` / `throw`
- `yield` / `yield from`

以下结构**不需要**分号：

- `fn` / `iter fn` 声明（函数体以 `}` 结尾）
- `class` / `enum` 声明
- `if` / `while` / `until` / `loop` / `for in` / `try catch`

## 顶层 Item 解析

顶层 item 通过 `parse_item()` 分发：

- 前置 `local` 修饰符会被先消耗，然后传入子解析函数
- `local` 后跟 `Ident`（而非关键字）是非法的 config 定义，报 `ExpectedItem` 并调用 `consume_malformed_local_config()` 跳过
- 顶层配置定义（`name = value` 或 `name: Type = value`）与 `var` 的区分：parser 看到 `Ident` 时走 `parse_config_decl()` 路径，看到 `Var` 关键字时走 `parse_var_decl()` 路径

## 特殊语法的消歧

### Lambda vs 分组表达式

`(` 可以开始 lambda 表达式 `(x) => expr` 或普通分组 `(expr)`。Parser 通过 `is_lambda_start()` 进行前瞻判断，规则：

- `()` 后紧跟 `=>` → lambda
- `(ident` 后紧跟 `:` `,` `)` `=` 之一 → lambda
- 其他情形 → 分组表达式

这个前瞻最多查看 3 个 token，不需要回溯。

### if 语句 vs if 表达式

`if` 在语句位置产出 `IfStmt`（条件 + then 块 + 可选 else 分支），在表达式位置（`parse_if_expr()`）产出 `IfExpr`（条件 + then 表达式 + else 表达式，均以 `{ }` 包裹）。上下文由调用方决定：`parse_stmt` 和 `parse_prefix` 各自路由到不同函数。

### Record vs Dict 字面量

`{id: v}` 和 `{"k": v}` 在 parser 层面统一表示为 `RecordLiteral`，键的类型通过 `RecordKey` 区分：

```rust
pub enum RecordKey {
    Ident(Ident),     // {id: v}  — 可能是对象或 dict
    String(StringLiteral), // {"k": v} — 只能是 dict
}
```

是否为对象构造还是 dict 字面量，推迟到语义分析阶段根据上下文类型判断。

### iter fn

`iter` 关键字不能独立出现，必须紧跟 `fn`。Parser 消耗 `iter` 后立即检查下一个 token 是否为 `Fn`，若不是则报 `ExpectedToken`。顶层、语句内、表达式内（`iter fn` 匿名函数）均遵循相同规则。

## 函数声明与函数体

函数声明（`FnDecl`）和函数表达式（`FnExpr`）共享相同的参数列表和函数体解析逻辑：

```
参数: name [: Type] [= default]
函数体: FnBody::Block(Block) 或 FnBody::Expr(Box<Expr>)
```

函数体的两种形式：
- `{ stmts }` → `FnBody::Block`
- `=> expr` → `FnBody::Expr`（表达式函数体）

Lambda 的函数体由 `=>` 引出，后接 `{ stmts }` 或直接接表达式：

```
(x, y) => x + y          // FnBody::Expr
(x, y) => { return x; }  // FnBody::Block
```

## Class 结构解析

`ClassDecl` 包含三类成员，在 `{...}` 块内按出现顺序解析：

- **字段**：`name: Type [= default]` → `ClassField`
- **方法**：`fn name(...) { }` → `FnDecl`，存入 `ClassDecl.methods`
- **check 块**：`check { condition => message ... }` → `CheckArm` 列表

`CheckArm` 的 `condition` 和 `message` 之间以 `FatArrow`（`=>`）分隔。

## 调用参数

函数调用支持位置参数和具名参数，在同一调用中可以混用：

```
Arg { name: Option<Ident>, value: Expr }
```

具名参数通过前瞻 `Ident` + `:` 消歧：若 `tokens[pos]` 为 `Ident` 且 `tokens[pos+1]` 为 `Colon`，则解析为具名参数，否则解析为位置参数（表达式）。

## 赋值语句解析

表达式语句和赋值语句共享同一入口 `parse_expr_or_assignment_stmt()`：先解析一个表达式，然后检查下一个 token 是否为赋值运算符（`=`、`+=`、`-=` 等共 14 种）。若是，则将已解析的表达式转换为 `AssignTarget`：

```rust
pub enum AssignTarget {
    Name(Ident),
    Field { object: Box<Expr>, field: Ident, span: Span },
    Index { object: Box<Expr>, index: Box<Expr>, span: Span },
}
```

只有 `Expr::Name`、`Expr::Field`、`Expr::Index` 可以转换为赋值目标，其他形式报 `InvalidAssignmentTarget`。

## 类型表达式

```rust
pub enum TypeExpr {
    Name(Path),                                    // Foo 或 a.b.C
    Array { element: Box<TypeExpr>, span },        // [T]
    Dict { key: Box<TypeExpr>, value: Box<TypeExpr>, span }, // dict[K, V]
    Function { params: Option<Vec<TypeExpr>>, return_ty: Option<Box<TypeExpr>>, span },
}
```

类型表达式用于变量声明、函数参数、函数返回值和 class 字段的类型标注。`fn` 类型有三种形式：`fn`（无参数列表信息）、`fn()`（空参数列表）、`fn(T, U) -> R`（完整签名）。

## 错误恢复

parser 在三个层级实施错误恢复，目标是在遇到错误后跳过最小范围的 token，继续解析后续结构：

### 顶层恢复：`synchronize_top_level()`

跳过 token 直到遇到顶层 item 起始关键字（`import` `local` `class` `enum` `var` `fn` `iter` `Ident`）。

### 语句级恢复：`synchronize_stmt()`

跳过 token 直到遇到语句起始关键字或 `}`，同时追踪括号深度以避免跳过嵌套结构内部的语句起始 token。遇到 `}` 时无条件停止（不消耗），以保证块解析循环能正常结束。

### Class 成员恢复：`synchronize_class_member()`

跳过 token 直到遇到 `}` `check` 或 `Ident`（下一个字段名或方法名的起始）。

恢复机制的共同设计原则：恢复函数只跳过 token，不消耗恢复点 token，让上层解析循环的 `if self.pos == before { self.bump(); }` 保护避免无限循环。

## 设计决策

**lex 有错则不解析**。脏 token 流会导致 parser 产出大量噪声错误，掩盖真正的根因。将 lex 错误直接返回，让用户先修复词法问题，提供更清晰的诊断体验。

**Pratt 解析器处理表达式**。递归下降对于算符优先级需要为每个优先级级别编写独立函数，而 Pratt 解析器通过一张 binding power 表统一处理，添加新运算符只需修改 `infix_binding_power`，无需重构调用链。

**对象/字典字面量延迟消歧**。parser 层面统一为 `RecordLiteral`，避免引入类型信息依赖。消歧推迟到 sema 阶段，保持 parser 无上下文（context-free）。

**链式比较在 parser 中展开**。`a < b < c` 在 parser 阶段展开为 `(a < b) and (b < c)`，而非在 sema 阶段处理。这使得 AST 表示无需特殊节点，降低了后续阶段的复杂度，代价是 `b` 在 AST 中出现两次（sema 需确保不重复求值副作用）。

**数字和字符串字面量以原始文本存储**。`Literal::Int { raw: String }` 和 `Literal::Float { raw: String }` 保留词法形式，数值解析推迟到 sema 阶段。这使 parser 无需处理整数溢出、进制转换等问题，保持职责单一。
