# coflow Lexer 设计

## 概述

coflow lexer 是一个无外部依赖的手写扫描器，位于 `src/lexer.rs`。它将 UTF-8 源字符串转换为 token 流，设计目标是：输出完整且带精确位置信息的 token 列表，错误不中止扫描（尽力恢复），为 parser 提供干净的接口。

公开入口只有一个：

```rust
pub fn lex(source: &str) -> LexOutput
```

`LexOutput` 同时携带成功产出的 token 和扫描过程中遇到的错误，两者均可非空：

```rust
pub struct LexOutput {
    pub tokens: Vec<Token>,
    pub errors: Vec<LexError>,
}
```

## 核心数据结构

### Token

```rust
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}
```

`Span` 是 UTF-8 字节偏移区间 `[start, end)`，指向原始源字符串中的切片。所有位置信息均以字节偏移表示，与 Unicode 码点数量无关。

### LexError

```rust
pub struct LexError {
    pub kind: LexErrorKind,
    pub span: Span,
}
```

错误不中止扫描。遇到无效字符或格式错误的字面量时，lexer 将错误记录到 `errors` 列表，同时跳过该 token（不向 `tokens` 写入），然后从下一个字符继续扫描。

## TokenKind 完整列表

### 关键字（32 个）

| 关键字 | TokenKind | 关键字 | TokenKind |
|--------|-----------|--------|-----------|
| `import` | `Import` | `as` | `As` |
| `local` | `Local` | `class` | `Class` |
| `enum` | `Enum` | `check` | `Check` |
| `fn` | `Fn` | `iter` | `Iter` |
| `dict` | `Dict` | `var` | `Var` |
| `if` | `If` | `else` | `Else` |
| `while` | `While` | `until` | `Until` |
| `loop` | `Loop` | `for` | `For` |
| `in` | `In` | `break` | `Break` |
| `continue` | `Continue` | `return` | `Return` |
| `throw` | `Throw` | `try` | `Try` |
| `catch` | `Catch` | `yield` | `Yield` |
| `from` | `From` | `and` | `And` |
| `or` | `Or` | `not` | `Not` |
| `true` | `True` | `false` | `False` |
| `null` | `Null` | `self` | `SelfKw` |

### 字面量

| TokenKind | 描述 |
|-----------|------|
| `Ident` | 标识符 |
| `IntLiteral` | 整数字面量（十进制、十六进制、二进制、八进制） |
| `FloatLiteral` | 浮点数字面量 |
| `StringLiteral` | 普通字符串 `"..."` |
| `RawStringLiteral` | 原始字符串 `r"..."` |
| `MultilineStringLiteral` | 多行字符串 `"""..."""` |
| `RawMultilineStringLiteral` | 原始多行字符串 `r"""..."""` |

### 运算符

**赋值运算符**

`Eq` `PlusEq` `MinusEq` `StarEq` `SlashEq` `PercentEq` `StarStarEq` `SlashSlashEq` `QuestionQuestionEq` `AmpEq` `PipeEq` `CaretEq` `LtLtEq` `GtGtEq`

**算术 / 比较 / 位运算**

`Plus` `Minus` `Star` `StarStar` `Slash` `SlashSlash` `Percent` `EqEq` `BangEq` `Lt` `LtEq` `LtLt` `Gt` `GtEq` `GtGt` `Amp` `Pipe` `Caret` `Tilde`

**其他运算符**

`QuestionQuestion` `Dot` `DotDot` `DotDotEq` `QuestionDot` `QuestionLBracket` `Arrow` `FatArrow` `DotDotDot`

### 分隔符

`LParen` `RParen` `LBrace` `RBrace` `LBracket` `RBracket` `Comma` `Colon` `Semicolon`

## 扫描器架构

lexer 以私有结构体 `Lexer<'a>` 实现，持有源字符串切片、字节位置游标 `pos`、以及两个输出集合：

```rust
struct Lexer<'a> {
    source: &'a str,
    pos: usize,
    tokens: Vec<Token>,
    errors: Vec<LexError>,
}
```

主循环逻辑为：读取当前字符，根据字符选择扫描路径，完成后继续循环。各路径之间通过 `starts_with` 前缀匹配或单字符 match 分发。

```
主循环
├── 空白 → 跳过（不产 token）
├── '#' → 跳过行注释
├── "/*" → 跳过块注释
├── r"""  → 扫描原始多行字符串
├── r"    → 扫描原始字符串
├── """   → 扫描多行字符串
├── "     → 扫描普通字符串
├── 0-9   → 扫描数字
├── _ / XID_Start → 扫描标识符/关键字
└── 其他  → 扫描符号或报 UnexpectedChar
```

## 空白处理

空白字符不产生任何 token，直接跳过。coflow 识别 5 种空白字符：

| 字符 | 含义 |
|------|------|
| `' '` | 空格 |
| `'\t'` | 水平制表符 |
| `'\n'` | 换行 |
| `'\r'` | 回车 |
| `'\u{000C}'` | 换页符 |

换行符不具有语法意义（coflow 使用显式分号，不做自动分号插入）。

## 注释

- **行注释**：`#` 开头，跳过直到 `\n`（包含 `\n`）。
- **块注释**：`/* ... */`，不支持嵌套。未遇到 `*/` 即到达文件末尾时报 `UnterminatedBlockComment` 错误。

## 标识符与关键字

标识符规则遵循 Unicode 标准（借助 `unicode-ident` crate）：

- **起始字符**：`_` 或满足 `XID_Start` 的 Unicode 字符
- **后续字符**：`_` 或满足 `XID_Continue` 的 Unicode 字符

扫描完整个标识符文本后，通过 `keyword_kind()` 查表：若命中关键字则产出对应关键字 token，否则产出 `Ident`。**关键字优先于标识符**，不存在将关键字作为变量名使用的语法逃脱途径。

`self` 因与 Rust 语言关键字冲突，在 `TokenKind` 中表示为 `SelfKw`。

## 数字字面量

### 整数前缀

| 前缀 | 进制 | 示例 |
|------|------|------|
| `0x` / `0X` | 十六进制 | `0xFF` |
| `0b` / `0B` | 二进制 | `0b1010` |
| `0o` / `0O` | 八进制 | `0o77` |
| 无前缀 | 十进制 | `42` |

### 浮点数

- 必须有整数部分：`1.5` 合法，`.5` 不合法（`.` 将被单独解析为 `Dot`）
- 小数点后必须有数字：`1.` 不合法（lexer 回退 `.`，产出整数 token `1`，再产出 `Dot`）
- 科学计数法：`1e3`、`1.0e-3`、`1E+10`，指数部分必须有至少一位数字

### 数字分隔符

`_` 可作为视觉分隔符插入数字中，规则：

- `_` 只能出现在两个合法数字字符之间
- 不能连续出现（`1__2` 非法）
- 不能作为开头或结尾（`_1`、`1_` 非法）
- 不能跨越小数点（`1_.5`、`1._5` 非法）

### 无效数字报错

下列情形报 `InvalidNumber` 并不产出 token：

- 前缀整数中出现不属于该进制的字符（如 `0b2`）
- 前缀后无合法数字（如 `0x`）
- 数字字面量紧跟标识符字符（如 `1foo`、`0x1g`）

词素中不含数值解析（`parseInt`、`parseFloat`）：`Literal::Int { raw }` 和 `Literal::Float { raw }` 保存原始字符串，数值转换推迟到语义分析阶段。

## 字符串字面量

coflow 支持 4 种字符串形式：

| 语法 | TokenKind | 支持转义 | 允许换行 |
|------|-----------|----------|----------|
| `"..."` | `StringLiteral` | 是 | 否 |
| `r"..."` | `RawStringLiteral` | 否 | 否 |
| `"""..."""` | `MultilineStringLiteral` | 是 | 是 |
| `r"""..."""` | `RawMultilineStringLiteral` | 否 | 是 |

**普通字符串转义序列**（`StringLiteral` 和 `MultilineStringLiteral`）：

| 转义 | 含义 |
|------|------|
| `\"` | 双引号 |
| `\\` | 反斜杠 |
| `\n` | 换行 |
| `\r` | 回车 |
| `\t` | 水平制表符 |

遇到不支持的转义字符时报 `InvalidEscape` 错误并终止当前字符串扫描。

**原始字符串**中反斜杠视为普通字符，不触发转义解析。

普通单行字符串（`StringLiteral`）中遇到未转义的 `\n` 或 `\r` 时报 `UnterminatedString`。

Token 的 `span` 包含完整的定界符（引号、`r` 前缀），原始文本存储在 `StringLiteral.raw` 中，字符串内容的解码（去除定界符、处理转义）推迟到后续阶段。

## 符号（Punctuation）扫描

符号采用 **longest-match** 策略，通过手写 if-else 链实现优先级。相互前缀的符号按长到短排列：

```
??=  >  ??
**=  >  **  >  *=  >  *
//=  >  //  >  /=  >  /
<<=  >  <<  >  <=  >  <
>>=  >  >>  >  >=  >  >
...  >  ..= >  ..
=>   （独立，不与 = 冲突）
->   （独立）
?[   >  ?.  >  ??=  >  ??（通过位置保证）
```

不属于任何已知符号的字符报 `UnexpectedChar` 错误。

## LexErrorKind

| 错误类型 | 触发场景 |
|----------|----------|
| `UnexpectedChar` | 遇到不属于任何 token 的字符 |
| `UnterminatedString` | 字符串字面量未正常闭合 |
| `UnterminatedBlockComment` | `/* ... */` 块注释未闭合 |
| `InvalidEscape` | 普通字符串中出现不支持的 `\x` 转义 |
| `InvalidNumber` | 数字字面量格式非法 |

## 设计决策

**不依赖外部 lexer 生成器（如 logos）**。手写扫描器在 coflow 的 token 集规模下代码量可控，同时保留了对特殊扫描逻辑（数字回退、字符串类型分发）的完整控制权。

**错误不中止**。lexer 遇到错误后将其记录，跳过无效部分，继续产出后续 token。这使得 parser 能在存在词法错误的源代码上继续工作，提供更完整的诊断信息。然而，`parse_module` 在检测到 lex 错误时会提前返回（不启动 parser），因为脏 token 流会导致 parser 产出噪声错误。

**Span 使用字节偏移**。字节偏移可直接用于切片 `&str`，避免字符级遍历，也与 Rust 字符串 API 保持一致。诊断工具需要将字节偏移转换为行列号时，可在 lexer 之外单独处理。

**字面量值不在 lex 阶段计算**。数字和字符串的原始文本被原样保留，语义分析阶段负责转换。这样 lexer 无需关心整数溢出、编码规范化等问题，保持职责单一。
