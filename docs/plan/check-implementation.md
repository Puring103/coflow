# check 块实现计划

> 状态：已实现。本文保留为实现记录；当前权威语义以 `docs/spec/02-cfc.md` 和 `src/coflow-cfc` 为准。

## 形式语法

```
check-block ::= "check" "{" cond-stmt* "}"

cond-stmt   ::= check-expr ";"
              | quantifier IDENT "in" check-expr "{" cond-stmt* "}"

check-expr  ::= or-expr
or-expr     ::= and-expr ("||" and-expr)*
and-expr    ::= is-expr ("&&" is-expr)*
is-expr     ::= cmp-chain ("is" type-predicate)?
cmp-chain   ::= bitor-expr (cmp-op bitor-expr)*
                  // 方向一致约束：
                  // 全递增：< 与 <= 可混用
                  // 全递减：> 与 >= 可混用
                  // 全等：仅 ==
                  // != 不参与链式
bitor-expr  ::= bitxor-expr ("|" bitxor-expr)*
bitxor-expr ::= bitand-expr ("^" bitand-expr)*
bitand-expr ::= add-expr ("&" add-expr)*
cmp-op      ::= "==" | "!=" | "<" | "<=" | ">" | ">="
add-expr    ::= shift-expr (("+" | "-") shift-expr)*
shift-expr  ::= mul-expr (("<<" | ">>") mul-expr)*
mul-expr    ::= prefix (("*" | "/" | "//" | "%") prefix)*
prefix      ::= "!" prefix | "~" prefix | "-" prefix | power
power       ::= postfix ("**" prefix)?          // 右结合
postfix     ::= primary ("(" args? ")" | "." IDENT | "[" check-expr "]")*
primary     ::= INT | FLOAT | BOOL | STRING | NULL
              | IDENT
              | "(" check-expr ")"
args        ::= check-expr ("," check-expr)*
quantifier  ::= "all" | "any" | "none"
type-predicate ::= type-name | "null"
```

## 运算符语义

| 运算符 | 支持类型 | 说明 |
|--------|---------|------|
| `\|\|` `&&` | bool | 短路求值 |
| `is` | object, union, null | 名义类型、union alias 或 null 判断 |
| `!` | bool | 逻辑非 |
| `\|` `^` `&` `~` | int | 按位运算 |
| `==` `!=` | 全部，同类型 | 相等比较 |
| `<` `<=` `>` `>=` | int, float, enum | 大小比较，enum 按整数值 |
| `+` `-` `*` | int, float | 算术；`+` 也支持 string 拼接 |
| `/` | int, float | 浮点除法，结果 float |
| `//` `%` | int | 整除（截断向零）、取余 |
| `**` | int, float | 幂运算，右结合 |
| `-`（一元） | int, float | 取负 |
| `<<` `>>` | int | 位移 |
| `.` | object | 字段访问 |
| `[]` | array, dict | 索引访问 |
| `()` | 内建函数名 | check-only 内建函数调用 |

枚举与 int 不隐式互转，枚举之间只有同类型才能比较。

## AST 变更

### `ast.rs`

```rust
// 扩充 CheckBlock
pub struct CheckBlock {
    pub stmts: Vec<CondStmt>,
    pub span: Span,
}

pub enum CondStmt {
    Expr(CheckExpr),
    Quantifier {
        kind: QuantifierKind,
        binding: String,
        collection: CheckExpr,
        body: Vec<CondStmt>,
        span: Span,
    },
}

pub enum QuantifierKind { All, Any, None }

pub struct CheckExpr {
    pub kind: CheckExprKind,
    pub span: Span,
}

pub enum CheckExprKind {
    Int(i64),
    Float(f64),
    Bool(bool),
    Null,
    Str(String),
    Name(String),                          // 字段名 / 枚举名 / 量词绑定变量
    Field { expr: Box<CheckExpr>, name: String },
    Index { expr: Box<CheckExpr>, index: Box<CheckExpr> },
    Is { expr: Box<CheckExpr>, predicate: TypePredicate },
    Call { name: String, args: Vec<CheckExpr> },
    BinOp { op: BinOp, lhs: Box<CheckExpr>, rhs: Box<CheckExpr> },
    Unary { op: UnaryOp, expr: Box<CheckExpr> },
    CmpChain { first: Box<CheckExpr>, rest: Vec<(CmpOp, CheckExpr)> },
}

pub enum BinOp {
    Or, And,
    BitOr, BitXor, BitAnd,
    Add, Sub, Mul, Div, IntDiv, Mod, Pow,
    Shl, Shr,
}

pub enum UnaryOp { Not, BitNot, Neg }

pub enum CmpOp { Eq, Ne, Lt, Le, Gt, Ge }
```

### `error.rs`

```rust
pub struct CheckError {
    pub kind: CheckErrorKind,
    pub span: Option<Span>,
}

pub enum CheckErrorKind {
    // 条件为假，附带表达式原文和值替换后的文本
    CondFailed {
        source: String,           // 原始表达式文本
        evaluated: String,        // 值替换后：如 "10 <= 3"
        context: String,          // "[Range, line 5]"
    },
    // all 量词失败，展开失败元素
    AllFailed {
        source: String,
        context: String,
        total: usize,
        failed: Vec<AllFailedItem>,
    },
    // 求值错误：类型不匹配、字段不存在、数组越界
    EvalError {
        message: String,
        context: String,
    },
}

pub struct AllFailedItem {
    pub key: String,              // "drops[1]" 或 "entry \"alice\""
    pub errors: Vec<CheckError>,  // 该元素内的失败列表
}
```

报错输出示例：

```
check failed [Range, line 5]:
  min <= max  (min=10, max=3)

check failed [Monster, line 12]:
  all drop in drops { drop.value > 0 }  (2/5 failed)
    drops[1]: drop.value = 0
    drops[3]: drop.value = -1

check failed [Zone, line 20]:
  all monster in monsters { ... }  (1/3 failed)
    monsters[2]: all drop in monster.drops { drop.value > 0 }  (2/4 failed)
      drops[0]: drop.value = 0
      drops[2]: drop.value = -1
```

## 实现状态

### 阶段 1：Lexer 补充 token（已完成）

已新增或确认以下 token：
- `AmpAmp` (`&&`)、`PipePipe` (`||`)、`Bang` (`!`)
- `Amp` (`&`)、`Pipe` (`|`)、`Caret` (`^`)、`Tilde` (`~`)
- `EqEq` (`==`)、`BangEq` (`!=`)
- `Less` (`<`)、`LessEq` (`<=`)、`Greater` (`>`)、`GreaterEq` (`>=`)
- `LessLess` (`<<`)、`GreaterGreater` (`>>`)
- `StarStar` (`**`)、`SlashSlash` (`//`)、`Percent` (`%`)
- `All`、`Any`、`None`
- `Null`、`In`、`Is`

### 阶段 2：Parser 填充 check 块（已完成）

当前 parser 已支持：
- 解析 `cond-stmt*`
- 解析 `all` / `any` / `none IDENT in check-expr { cond-stmt* }`
- 解析 `check-expr`，包含逻辑、比较、`is`、按位、算术、后缀访问和内建函数调用
- 链式比较方向一致性检查在 parser 层完成

### 阶段 3：新增 `src/check.rs`（已完成）

求值引擎核心结构：

```rust
// 求值上下文：作用域栈
struct CheckScope<'a> {
    // 每层：绑定变量名 → 值
    layers: Vec<HashMap<String, CfcValueRef>>,
    // 符号表，用于枚举名解析
    symbols: &'a SymbolTable,
}

impl CheckScope<'_> {
    fn lookup(&self, name: &str) -> Option<CfcValueRef>;
    fn push(&mut self, binding: String, value: CfcValueRef);
    fn pop(&mut self);
}
```

两类入口：

- `check_type_instance`：遍历对象图，找出所有带 type 的 Object，查找对应 TypeDef 的 check 块，以对象字段为初始 scope 执行
- `check_top_level`：遍历每个模块 AST 中的顶层 CheckBlock，以模块命名节点为初始 scope 执行

对象图遍历用 `CfcValueRef` 的指针 key 做 visited 标记，防止循环引用导致无限递归。

### 阶段 4：`container.rs` 接入（已完成）

```rust
impl CfcContainer {
    pub fn check(&self, result: &CfcResult) -> Vec<CheckError> {
        check::run(self, result)
    }
}
```

## 测试计划

### 基础条件语句

- `min <= max` 通过
- `min <= max` 失败，验证报错含原始表达式和求值结果
- 多条语句，第一条失败后继续收集第二条失败
- 类型错误（int 与 string 比较）立即停止，报 EvalError

### 链式比较

- `0 < x <= 100` 通过
- `0 < x <= 100` 失败（x=200），报错展示正确
- 方向不一致（`a < b > c`）在 parser 阶段报错

### 算术运算

- `damage * 2 <= max_damage` 通过和失败
- 整除 `hp // 10 > 0`
- 幂运算 `base ** level <= cap`

### 按位运算

- `flags & mask != 0`
- `~flags | other == 0`

### `all` 量词

- 空集合通过（vacuous truth）
- 全部通过
- 部分失败，报错含 N/M 和失败元素列表
- Dict 迭代：`entry.key`、`entry.value` 访问正确
- entry 访问不存在字段报 EvalError

### 嵌套 `all`

- 两层嵌套，外层部分失败展开内层错误
- 三层嵌套，验证缩进格式正确

### 访问范围

- type 内 check 访问外部命名节点报错
- 顶层 check 访问命名节点正常

### shared 对象去重

- 两个节点引用同一对象，check 只执行一次，错误不重复

### 枚举比较

- `rarity >= Rarity.rare` 通过和失败
- 不同枚举类型比较报类型错误

### 数组越界

- `drops[10]` 在空数组上立即停止，报 EvalError

### 顶层 check

- 跨节点约束通过和失败
- 报错标识含模块名和行号
