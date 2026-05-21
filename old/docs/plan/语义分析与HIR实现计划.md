# 语义分析与HIR实现计划

本文整理 coflow 核心版本的语义分析阶段需要完成的工作，以及 AST 降级到 HIR 的形态和流程。实现语言使用 Rust，与已有的 lexer / parser 同处一个 crate。

核心依据：`docs/spec/` 下的语言规范文档。

后续阶段（字节码 VM 与 codegen）单独立项，本计划只覆盖语义分析与 HIR。

## 目标

将 parser 产出的 AST 处理成 HIR（High-level IR），HIR 同时服务于：

1. 配置常量求值器（加载期执行，产出深只读配置值）
2. 字节码 codegen（运行时执行，编译成 Proto）

HIR 应满足：

1. 所有名字引用消解为 ID，无字符串查找。
2. 所有类型注解解析为 `Ty`，无 `TypeExpr` 残留。
3. 所有字面量规范化为 `Value`。
4. 语法糖脱掉（链式比较、`until`、`for in 0..N` 等）。
5. 闭包带显式捕获列表（upvalue 链已建立）。
6. 对象与字典已消歧。
7. 类型注解的运行时检查以显式 `TypeGuard` 节点表达。

语义分析阶段不负责：

1. 字节码生成。
2. 寄存器/槽位分配。
3. 模块路径解析与跨模块加载（先做单文件版本，跨模块在后续阶段补）。

## 依赖

无新增 crate。复用已有的 `ast`、`lexer`、`parser`、`span` 模块。

新增模块结构建议：

```
src/
  hir.rs            # HIR 数据结构与 Ty / Value
  sema/
    mod.rs          # 公共入口与 Diagnostic
    collect.rs      # P1 声明收集
    lower/
      mod.rs        # P2 降级总入口
      scope.rs      # 作用域栈与 upvalue 链
      expr.rs       # 表达式降级
      stmt.rs       # 语句降级
      tycheck.rs    # 类型检查与 TypeGuard 插入
      record.rs     # 对象/字典消歧
    config_eval.rs  # P3 配置常量求值器
```

## 总体阶段

```
AST
 │
 ├── P1 声明收集 ──> ModuleSymbols
 │
 ├── P2 单遍降级 ──> HirModule
 │      （名字解析 + 上下文检查 + 类型检查 + lower 一遍走完）
 │
 └── P3 配置求值 ──> HirModule.config_values（深只读 Value 表）
```

P1 必须独立先跑，因为函数体内可前向引用同模块其它顶层成员，必须先把所有顶层名字收集好。

P2 把名字解析、上下文检查、类型检查、lower 合并为一遍：四件事共用同一个上下文，分开维护多份边表反而成本更高。

P3 在 HIR 上跑，复用同一份 `Value` 表示，避免配置语义和运行时语义分裂。

## P1 声明收集

输入：`ast::Module`。
输出：`ModuleSymbols`。

```rust
pub struct ModuleSymbols {
    pub globals: IndexMap<String, GlobalEntry>,
    pub classes: IndexVec<ClassId, ClassInfo>,
    pub enums:   IndexVec<EnumId, EnumInfo>,
    pub imports: IndexMap<String, ModuleRef>,
    pub builtins: BuiltinSymbols,
}

pub enum GlobalEntry {
    Config   { id: ConfigId,   ty: Option<TypeExprRef> },
    Var      { id: VarId,      ty: Option<TypeExprRef> },
    Function { id: FunctionId, is_iter: bool },
    Class    (ClassId),
    Enum     (EnumId),
    Import   (ModuleRef),
    Builtin  (BuiltinId),
}

pub struct ClassInfo {
    pub name: String,
    pub local: bool,
    pub fields: IndexMap<String, ClassFieldInfo>,
    pub has_check: bool,
    pub span: Span,
    pub ast_id: AstClassId,  // 回链到 AST 用于 P2 降级 check 块
}

pub struct EnumInfo {
    pub name: String,
    pub local: bool,
    pub variants: IndexMap<String, EnumVariantInfo>,
    pub span: Span,
}
```

P1 检查：

1. 顶层重复定义。
2. class 字段重复。
3. enum 变体重复。
4. enum 自动编号（填齐 `EnumVariant.value` 为 `None` 的项，按"前一个值 + 1"规则）。

P1 不进函数体，不解析 `TypeExpr`，不做类型检查。`ty: Option<TypeExprRef>` 只是回链到 AST，等 P2 解析。

`BuiltinSymbols` 注入预定义全局，至少包含：

- `error`：构造 error 对象的内建函数
- `iter`：取 iterator 的内建函数
- `range`：标准库 range 函数
- `print`：临时调试用，最终由宿主决定

内建以独立的 `BuiltinId` 表示，避免与用户定义混淆。

## P2 降级到 HIR

### Ty 与类型解析

```rust
#[derive(Clone, PartialEq, Eq)]
pub enum Ty {
    Int, Float, Bool, String, Null, Any,
    Array(Box<Ty>),
    Dict(Box<Ty>, Box<Ty>),
    Class(ClassId),
    Enum(EnumId),
    Function,                            // 顶类型：任意函数
    FunctionSig {
        params: Vec<Ty>,                 // 必填位置参数类型
        return_ty: Box<Ty>,              // 返回类型；无 -> 标注视为 Any
    },
    Iterator,
    Error,                               // 解析失败占位，参与类型检查时短路
}
```

`TypeExpr` 在 P2 中解析成 `Ty`：

| TypeExpr | Ty |
| --- | --- |
| `Name("int")` | `Ty::Int` |
| `Name("float")` | `Ty::Float` |
| `Name("bool")` | `Ty::Bool` |
| `Name("string")` | `Ty::String` |
| `Name("any")` | `Ty::Any` |
| `Name("null")` | `Ty::Null` |
| `Name("Foo")` | 查 ModuleSymbols → `Class(id)` / `Enum(id)`；找不到报错 |
| `Array(T)` | `Ty::Array(resolve(T))` |
| `Dict(K, V)` | `Ty::Dict(resolve(K), resolve(V))` |
| `Function(P1..Pn) -> R` | `Ty::FunctionSig { params, return_ty }` |
| `Function` 无参无返回 | `Ty::Function`（顶类型，写法 `fn` 或 `fn() -> any`） |

Path 类型暂只支持单段（无跨模块类型引用），跨模块在后续阶段补。

### 函数类型语法

需要扩展 `TypeExpr` 支持函数类型。建议语法：

```coflow
var on_hit: fn(int, int) -> int = ...
var callback: fn(string) = ...        # 无返回类型 → return_ty = Any
var any_fn: fn = ...                  # 顶类型 Function
```

AST `TypeExpr` 增加：

```rust
pub enum TypeExpr {
    Name(Path),
    Array { element: Box<TypeExpr>, span: Span },
    Dict  { key: Box<TypeExpr>, value: Box<TypeExpr>, span: Span },
    Function {
        params: Vec<TypeExpr>,
        return_ty: Option<Box<TypeExpr>>,  // None → Any
        span: Span,
    },
}
```

parser 需要扩展：在 type 位置遇到 `fn` token，按 `fn ( T1, T2, ... ) -> R` 解析。无 `(` 时是顶类型 `Ty::Function`。

### 函数签名规则

**签名计算**：

`fn` / `iter fn` / `lambda` 声明出来的函数值，签名按"必填参数"计算：

| fn 声明 | 签名 |
| --- | --- |
| `fn add(a: int, b: int) -> int` | `FunctionSig { params: [Int, Int], return_ty: Int }` |
| `fn spawn(name: string, hp: int = 100)` | `FunctionSig { params: [String], return_ty: Any }` |
| `fn unknown(a, b)` | `FunctionSig { params: [Any, Any], return_ty: Any }` |
| `fn(x) => x * 2`（无类型上下文） | `FunctionSig { params: [Any], return_ty: Any }` |

带默认值的参数不进 `params`。具名参数仍合法（调用时按名字传），但签名不描述具名能力。

**iter fn 的返回类型固定为 `Iterator`**，不受 `-> T` 影响（核心版本 iter fn 不允许 `-> T` 标注）。

### 子类型与兼容性

### Value 类型

```rust
#[derive(Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(Rc<str>),
    Array(Rc<Vec<Value>>),
    Dict(Rc<Vec<(Value, Value)>>),
    Object {
        class: Option<ClassId>,
        fields: Rc<Vec<(String, Value)>>,
    },
    Range { start: i64, end: i64, inclusive: bool },
    EnumVariant(EnumId, VariantId),
    Closure(Rc<ClosureData>),
    // Iterator 不进配置阶段
}
```

配置阶段产出的所有 `Value` 都是深只读。`Rc` 包装保证引用语义，运行时修改通过 VM 的写时校验拦截。

### HIR 数据结构

```rust
pub struct HirModule {
    pub globals:   IndexVec<GlobalId, HirGlobal>,
    pub functions: IndexVec<FunctionId, HirFunction>,
    pub classes:   IndexVec<ClassId, HirClass>,
    pub enums:     IndexVec<EnumId, HirEnum>,

    pub config_eval_order: Vec<GlobalId>, // P3 拓扑排序后填充
    pub config_values: IndexMap<GlobalId, Value>, // P3 求值后填充
}

pub enum HirGlobal {
    Config {
        id: GlobalId,
        name: String,
        ty: Option<Ty>,
        value: HirExpr,
        span: Span,
    },
    Var {
        id: GlobalId,
        name: String,
        local: bool,
        ty: Option<Ty>,
        init: Option<HirExpr>,
        span: Span,
    },
    Function {
        id: GlobalId,
        name: String,
        local: bool,
        fn_id: FunctionId,
        span: Span,
    },
    Class { id: GlobalId, class_id: ClassId, span: Span },
    Enum  { id: GlobalId, enum_id: EnumId, span: Span },
}

pub struct HirFunction {
    pub is_iter: bool,
    pub params: Vec<HirParam>,
    pub return_ty: Option<Ty>,
    pub locals: IndexVec<LocalId, HirLocal>,
    pub upvalues: Vec<UpvalueDesc>,
    pub body: Vec<HirStmt>,
    pub span: Span,
}

pub struct HirParam {
    pub local_id: LocalId,
    pub name: String,
    pub ty: Option<Ty>,
    pub default: Option<HirExpr>,
    pub span: Span,
}

pub struct HirLocal {
    pub name: String,
    pub ty: Option<Ty>,
    pub is_captured: bool,
    pub span: Span,
}

pub enum UpvalueDesc {
    Local(LocalId),       // 捕获父函数的 local
    Upvalue(UpvalueId),   // 捕获父函数的 upvalue（多级）
}

pub struct HirClass {
    pub name: String,
    pub local: bool,
    pub fields: Vec<HirClassField>,
    pub checks: Vec<HirCheckArm>,
    pub span: Span,
}

pub struct HirClassField {
    pub name: String,
    pub ty: Ty,
    pub default: Option<HirExpr>,  // 必须是常量表达式
    pub span: Span,
}

pub struct HirCheckArm {
    pub cond: HirExpr,
    pub message: HirExpr,
    pub span: Span,
}

pub struct HirEnum {
    pub name: String,
    pub local: bool,
    pub variants: Vec<HirEnumVariant>,
    pub span: Span,
}

pub struct HirEnumVariant {
    pub name: String,
    pub value: i64,    // 已自动编号
    pub span: Span,
}
```

#### 语句

```rust
pub enum HirStmt {
    Local      { id: LocalId, init: Option<HirExpr>, span: Span },
    Assign     { target: HirAssignTarget, op: AssignOp, value: HirExpr, span: Span },
    Expr       (HirExpr),
    If         { cond: HirExpr, then_: Vec<HirStmt>, else_: Option<Vec<HirStmt>>, span: Span },
    While      { cond: HirExpr, body: Vec<HirStmt>, span: Span },
    Loop       { body: Vec<HirStmt>, span: Span },
    ForIn      { item: LocalId, iter: HirExpr, body: Vec<HirStmt>, span: Span },
    ForRange   { var: LocalId, start: HirExpr, end: HirExpr, inclusive: bool,
                 body: Vec<HirStmt>, span: Span },
    Break      (Span),
    Continue   (Span),
    Return     { value: Option<HirExpr>, span: Span },
    Throw      { value: HirExpr, span: Span },
    TryCatch   { try_: Vec<HirStmt>, err: LocalId, catch_: Vec<HirStmt>, span: Span },
    Yield      { value: HirExpr, span: Span },
    YieldFrom  { value: HirExpr, span: Span },
}

pub enum HirAssignTarget {
    Local    (LocalId),
    Upvalue  (UpvalueId),
    Global   (GlobalId),
    Field    { obj: Box<HirExpr>, field: String, span: Span },
    Index    { obj: Box<HirExpr>, index: Box<HirExpr>, span: Span },
}
```

`until` 在降级时反转条件，转成 `While { cond: Not(cond), ... }`，HIR 中没有 `Until` 节点。

`ForRange` 是 `for i in start..end` 的优化路径，codegen 可以直接生成计数循环，省掉 iterator 对象分配。其它可迭代值走 `ForIn`。

#### 表达式

```rust
pub enum HirExpr {
    Const     { value: Value, span: Span },
    Local     { id: LocalId, span: Span },
    Upvalue   { id: UpvalueId, span: Span },
    Global    { id: GlobalId, span: Span },
    Variant   { enum_id: EnumId, variant: VariantId, span: Span },
    Closure   { fn_id: FunctionId, span: Span },
    SelfField { name: String, span: Span },  // check 块内 self.field

    Unary     { op: UnaryOp, expr: Box<HirExpr>, span: Span },
    Binary    { op: BinaryOp, lhs: Box<HirExpr>, rhs: Box<HirExpr>, span: Span },
    AndChain  { exprs: Vec<HirExpr>, span: Span },          // 链式比较已展开
    NullCoalesce { left: Box<HirExpr>, right: Box<HirExpr>, span: Span },

    Call      { callee: Box<HirExpr>, args: Vec<HirArg>, span: Span },
    Field     { obj: Box<HirExpr>, field: String, span: Span },
    OptField  { obj: Box<HirExpr>, field: String, span: Span },
    Index     { obj: Box<HirExpr>, index: Box<HirExpr>, span: Span },

    Array     { elements: Vec<HirExpr>, span: Span },
    Object    { class: Option<ClassId>, fields: Vec<(String, HirExpr)>,
                spreads: Vec<HirExpr>, span: Span },
    Dict      { entries: Vec<(HirExpr, HirExpr)>, span: Span },
    Range     { start: Box<HirExpr>, end: Box<HirExpr>, inclusive: bool, span: Span },

    TypeGuard { expr: Box<HirExpr>, ty: Ty, span: Span },

    Error     (Span),  // 降级失败占位，类型检查遇此短路
}

pub struct HirArg {
    pub name: Option<String>,  // 具名参数
    pub value: HirExpr,
    pub span: Span,
}
```

设计说明：

1. **`OptField` 不展开**：保留为节点，codegen 直接翻成"null check + jump + GET_FIELD"。展开成 `If` 需要插临时变量，HIR 层不必污染。
2. **`NullCoalesce` 不展开**：同上，codegen 翻成短路指令。
3. **`AndChain` 已展开**：链式比较 `0 < x <= 100` 拆成 `[0 < x, x <= 100]` 的 and 链。`x` 是有副作用表达式时插临时 local。
4. **`SelfField`**：check 块专用，避免引入完整的 `Self` 表达式。求值器看到此节点时绑定到当前校验对象。
5. **`Object.spreads`**：对象字面量的 `...base` 展开保留为单独的 spread 列表，避免和命名字段混在一起。
6. **`Error` 节点**：类型检查、求值器遇到都短路，不再传播错误。

### 名字解析与作用域

降级上下文：

```rust
struct LowerCtx<'a> {
    symbols: &'a ModuleSymbols,
    module: HirModule,
    fn_stack: Vec<FnCtx>,
    diagnostics: Vec<Diagnostic>,
}

struct FnCtx {
    fn_id: FunctionId,
    is_iter: bool,
    return_ty: Option<Ty>,
    scopes: Vec<Scope>,
    loop_depth: usize,
    in_check_block: bool,
    locals: IndexVec<LocalId, HirLocal>,
    upvalues: Vec<UpvalueDesc>,
    upvalue_index: HashMap<UpvalueKey, UpvalueId>,
}

struct Scope {
    vars: HashMap<String, LocalId>,
}
```

**查找规则**：

```
resolve_name(name):
  1. 当前函数 scopes 栈顶往下找 → HirExpr::Local(id)
  2. 沿 fn_stack 外层逐级查找局部
     - 在外层第 k 层找到 local L：
       - 标记 L.is_captured = true
       - 在第 k+1 层注册 UpvalueDesc::Local(L) → upvalue_0
       - 在第 k+2 层注册 UpvalueDesc::Upvalue(upvalue_0) → upvalue_0
       - 一直向内传递，直到当前层
       - 返回 HirExpr::Upvalue(当前层 upvalue id)
  3. 查 ModuleSymbols.globals
     - Config / Var / Function → HirExpr::Global(id)
     - Class → HirExpr::Global(id)（class 名作为类型用）
     - Enum  → HirExpr::Global(id)（用于 Enum.Variant 形式）
     - Import → 等待 .field 访问处理
     - Builtin → HirExpr::Global(builtin_id)
  4. 在 check 块内 + name == "self" → 内部状态切换为 self 上下文
     字段访问 self.x 降级为 HirExpr::SelfField("x")
  5. 都找不到 → Diagnostic::UndefinedName，返回 HirExpr::Error
```

**Path 访问**：

`a.b` 在 P2 降级时按以下顺序判定：

1. `a` 是 import 别名 → `b` 是该模块的顶层名（暂未实现，留占位）。
2. `a` 是 enum 名 → `b` 是变体 → `HirExpr::Variant`。
3. 其它 → `HirExpr::Field`。

**check 块的 self 处理**：

降级 `HirCheckArm` 时：

1. 设置 `in_check_block = true`，无 `FnCtx`（check 不是函数，是表达式上下文）。
2. 表达式中遇到 `self` 名字 → 进入 self 接收态。
3. 后续 `.field` 转成 `HirExpr::SelfField(field)`。
4. 单独的 `self`（不带字段）报错。
5. check 块内禁止：赋值、调用（除 enum 变体引用、纯算术、字符串操作）、`yield` 类语句。

### 上下文检查

P2 降级过程中执行，与名字解析合在同一遍：

| 检查 | 失败时机 |
| --- | --- |
| 顶层只允许 6 类声明 | parser 已基本拦下，P2 兜底 |
| `break` / `continue` 必须在循环内 | `loop_depth == 0` 时报错 |
| `break` / `continue` 不跨函数边界 | 进入 fn 时 `loop_depth` 重置 |
| `yield` / `yield from` 必须在 iter fn | `fn_stack` 顶 `is_iter == false` 时报错 |
| `iter fn` 中禁止 `return value` | iter fn 中 `Return { value: Some(_) }` 报错 |
| `self` 仅在 check 块 | check 之外引用 `self` 报错 |
| check 块禁修改 / 禁调用宿主 | 见上节 |
| 公开 API 不泄露 local 类型 | P2 解析类型后做检查 |
| 参数默认值 / class 字段默认值是常量 | P3 求值时验证（HIR 阶段不做） |

公开 API 不泄露 local 类型的具体规则：

- 公开 fn 的参数类型、返回类型不能引用 local class/enum
- 公开 class 的字段类型不能引用 local class/enum
- 公开 var / 公开 config 的类型标注不能引用 local class/enum

### 类型检查

兼容性规则（`T ← U` 表示 T 接收 U）：

```
T ← T                           ✓
Any ← T                         ✓ 任意值上升为 Any
T ← Any                         ✓ 但插 TypeGuard
Any ← Null                      ✓
T ← Null                        ✗（核心版本无可空类型，T 非 Any）
Array<T> ← Array<T>             ✓
Dict<K, V> ← Dict<K, V>         ✓
Class(C) ← Class(C)             ✓
Enum(E)  ← Enum(E)              ✓
Function ← Function             ✓
Function ← FunctionSig{..}      ✓ 具名签名是 Function 的子类型
FunctionSig ← FunctionSig       见下方"函数子类型规则"
FunctionSig ← Function          ✗ 静态不兼容（赋值需显式中间变量），运行时收窄需 TypeGuard
其它                             ✗
Error ← _ / _ ← Error           ✓ 短路，避免错误风暴
```

**函数子类型规则**：

```
FunctionSig { params: P1..Pn, return_ty: R }
   ←
FunctionSig { params: Q1..Qm, return_ty: S }
```

成立当且仅当：

1. 参数数量相等：`n == m`
2. 参数类型**逆变**：每个 `Qi ← Pi`（接收方参数比来源方更宽）
3. 返回类型**协变**：`R ← S`（来源方返回比接收方更窄）

第一版可以用更严的"参数与返回都不变"（`Pi == Qi` 且 `R == S`）替代，避免实现 PEC（参数逆变 / 返回协变）的复杂度。**建议第一版用不变规则**，下一版需要时再放宽。

**静态报错（编译期可证）**：

1. 字面量类型已知且不兼容：`var x: int = "s"`、`return "s"` vs `-> int`
2. class 字段默认值类型不兼容
3. fn 参数默认值类型不兼容
4. 配置值类型与配置标注不兼容
5. class 实例化字段类型不兼容
6. dict 字面量值类型与标注不兼容
7. 函数值赋给具名签名时签名不匹配（参数数量、参数类型、返回类型）
8. 调用点：callee 是具名签名时，必填参数数量不足，或具名实参类型不兼容

**插 `TypeGuard`（编译期不可证，运行时检查）**：

1. 函数入口：每个有类型注解的参数插 guard（在 body 第一条语句之前）
2. 函数返回：有 `-> T` 时给每个 `return expr` 的 expr 包 guard
3. 带类型标注的 var：初始化和后续赋值的 RHS 包 guard
4. `Any → T` 的隐式收窄统一插 guard
5. `Function → FunctionSig` 的收窄：在赋值点插 guard，运行时检查闭包签名

**TyCtx 上下文传递**：

```rust
enum TyCtx {
    None,
    Expect(Ty),
}
```

降级表达式时向下传递：

| 位置 | TyCtx |
| --- | --- |
| 顶层 `name: T = expr` | `Expect(T)` |
| `var x: T = expr` | `Expect(T)` |
| `return expr` 在 `-> T` 函数中 | `Expect(T)` |
| 数组字面量元素，外层是 `Array<E>` | `Expect(E)` |
| 对象字面量字段值，外层是 `Class(C)` 的字段 `f: F` | `Expect(F)` |
| 字典字面量 entry，外层是 `Dict<K, V>` | key `Expect(K)`, value `Expect(V)` |
| 函数实参，callee 是 `FunctionSig` | 第 i 个实参 `Expect(params[i])` |
| 函数实参，callee 是 `Function` 或动态 | `None` |
| 二元运算操作数 | `None`（按动态类型） |
| 其它 | `None` |

### Lambda 类型推断

无类型上下文的 lambda 默认签名是 `FunctionSig { params: [Any; n], return_ty: Any }`。

带类型上下文（`TyCtx::Expect(FunctionSig { params, return_ty })`）时：

1. 实参数量与上下文 params 不一致 → `TypeMismatch`，仍按上下文降级避免错误传染
2. lambda 参数无类型注解 → 用上下文 `params[i]` 作为参数类型
3. lambda 参数有类型注解 → 用注解，但 `params[i] ← annotated` 必须成立
4. 返回类型同理：无 `-> T` 标注 → 用上下文 `return_ty`；有标注 → 用标注，且 `return_ty ← annotated` 成立

```coflow
class Skill {
    apply: fn(any, any)
}

fireball: Skill = {
    apply: (caster, target) => target.hp -= 10
}
# lambda 推断为 fn(any, any) -> any
```

普通 fn 声明不参与上下文推断（fn 声明出现在语句位置，无 TyCtx 传入）。

### 调用点检查

callee 是 `Ty::FunctionSig { params, return_ty }` 时：

1. 收集位置参数与具名参数
2. 必填参数数量必须 ≥ `params.len()`
3. 多余参数报 `TypeMismatch`（核心版本不支持变长参数）
4. 每个位置参数类型 `params[i] ← arg_ty`
5. 具名参数：核心版本签名不带具名信息 → 报 `TypeMismatch`（建议用户用普通 fn 声明而非函数值传递具名）
6. 调用表达式的类型为 `return_ty`

callee 是 `Ty::Function`（顶类型）或 `Ty::Any` 时：

1. 不做参数检查，调用表达式类型为 `Any`
2. 运行时分派

### 对象/字典消歧

`RecordLiteral` 在 P2 降级，按 TyCtx 和 key 形式判定：

```
lower_record(entries, ctx):
  match ctx:
    Expect(Class(c)) → 走 Object 路径，按 c 校验
    Expect(Dict(k, v)) → 走 Dict 路径，类型检查 k/v
    Expect(Any) | None:
      所有 entry 是 Field 且 key 是 Ident → Object（无 class）
      所有 entry 是 Field 且 key 是 String → Dict
      混合 ident / string key → 报错（核心版本不支持）
      含 Spread → 默认 Object（spread 仅用于对象，字典展开放入提案）
```

**Object 路径校验（class 已知）**：

1. 必填字段必须存在（无默认值的字段）
2. 字段类型按 TyCtx 递归检查
3. 不允许多余字段
4. 字段顺序与 class 声明顺序无关

**Object 路径校验（class 未知）**：

1. ident key 重复检查
2. 字段值降级时 TyCtx 为 None

**Dict 路径**：

1. 推断 key 类型（核心版本只支持 string key）
2. 推断 value 类型：同构推具体类型，异构推 `Any`
3. 顶层配置定义不允许"无标注 dict"，必须显式标注 `dict[K, V]`

### 脱糖规则

| 源构造 | HIR |
| --- | --- |
| `until c { body }` | `While { cond: Unary(Not, c), body }` |
| `for i in start..end { body }` | `ForRange { var, start, end, inclusive: false, body }` |
| `for i in start..=end { body }` | `ForRange { var, start, end, inclusive: true, body }` |
| `for x in expr { body }` | `ForIn { item, iter: expr, body }` |
| `0 < x <= 100`（x 无副作用） | `AndChain([0 < x, x <= 100])` |
| `0 < f() <= 100`（中间项有副作用） | `Local(tmp, f()); AndChain([0 < tmp, tmp <= 100])` |
| `name ??= rhs` | `If { cond: Eq(Local(name), Const(Null)), then: [Assign(name, rhs)] }` |
| 整数字面量 `1_000` | `Const(Int(1000))` |
| 字符串字面量带转义 | `Const(String(decoded))` |

不脱糖的情况：

| 源构造 | HIR | 原因 |
| --- | --- | --- |
| `a ?. b` | `OptField` | codegen 单条短路指令 |
| `a ?? b` | `NullCoalesce` | codegen 单条短路指令 |
| `a and b` / `a or b` | `Binary(And/Or)` | 同上，统一在 codegen 处理 |
| `loop { ... }` | `Loop` | 与 While 语义不同 |

### 错误恢复

1. 名字解析失败 → `HirExpr::Error`，记 Diagnostic，继续降级。
2. 类型不兼容 → 不改 HIR 形态（保留原表达式），记 Diagnostic。
3. RecordLiteral 在 None 上下文混用 ident/string key → `HirExpr::Error`，记 Diagnostic。
4. `HirExpr::Error` 参与类型检查时短路（任何 T ← Error 和 Error ← T 都通过），避免错误传染。
5. 一个文件内尽量收集所有错误再返回，不在第一个错误处中止。

## P3 配置求值

输入：P2 产出的 `HirModule`。
输出：填充 `HirModule.config_values` 与 `HirModule.config_eval_order`。

### 步骤

**1. 配置依赖图收集**

遍历每个 `HirGlobal::Config.value`，收集对其它 `Config` 全局的引用：

- `HirExpr::Global(id)` 指向 Config → 加入依赖
- `HirExpr::Global(id)` 指向 Var → 报错"配置不能依赖 var"
- 不进入 `Closure` 节点的函数体（函数值是值，不递归求值）
- class 字段默认值、enum 不需要求值（已在 P1/P2 解析）

**2. 拓扑排序**

依赖图无环检查 → 拓扑排序填入 `config_eval_order`。有环报"配置循环依赖"。

**3. 按拓扑序求值**

求值器是 `HirExpr` 解释器，对每个允许的节点产出 `Value`：

| HirExpr | 配置阶段 |
| --- | --- |
| `Const(v)` | `v` |
| `Global(id)` | 已求值的 config_values[id]，或 enum 变体常量，或闭包值 |
| `Variant(...)` | `Value::EnumVariant(...)` |
| `Closure(fn_id)` | `Value::Closure { fn_id, upvalues: [] }`（顶层闭包无 upvalue） |
| `SelfField(name)` | check 块外不允许；check 内绑定到当前对象字段 |
| `Unary` / `Binary` | 按值计算（仅算术、比较、字符串拼接、位运算） |
| `Array` | 元素递归求值，产出 `Value::Array` |
| `Object` | 字段递归求值，spread 展开，产出 `Value::Object` |
| `Dict` | entry 递归求值，产出 `Value::Dict` |
| `Range` | `Value::Range` |
| `NullCoalesce` | 短路求值 |
| `OptField` / `Field` / `Index` | 已求值对象的字段/索引访问（受深只读保护） |
| `TypeGuard` | 检查类型，失败报"配置类型不匹配" |
| **`Call`** | **报错"配置不允许调用普通函数"** |
| **`AndChain`** | **报错（暂不允许配置中链式比较）** |
| `Local` / `Upvalue` | **不可能出现在顶层 Config 表达式中**（已被名字解析阶段保证） |
| `Error` | 短路，跳过求值 |

**4. class 类型校验**

带 `Class(c)` 类型标注的 Config 求值出 `Value::Object` 后：

1. 必填字段存在性检查
2. 字段类型递归校验（嵌套 Object/Array/Dict 递归）
3. 不允许多余字段（HIR 层应已拦下，这里兜底）

**5. 执行 check 块**

类型校验通过后，对每个 `HirCheckArm` 求值：

1. 绑定 `SelfField` 上下文到当前对象
2. 求 `cond`：必须是 bool，否则报错
3. cond 为 false → 求 `message` → 抛"配置校验失败"
4. cond 为 true → 通过，进入下一条

**6. 标记深只读**

求值产物全部冻结。运行时对 `Value::Object/Array/Dict` 任何字段写入触发"修改只读配置"运行时错误（这一条由 VM 实现，HIR/求值器只负责标记）。

### 配置求值的边界

求值器**不调用**任何函数体。`Value::Closure` 携带 `fn_id`，运行时被调用时才进 VM。

求值器**不接触**宿主 API。所有内建函数（`error` / `range` / `print`）在配置阶段拒绝调用——这些是运行时函数。

`error("...")` 在配置阶段不能出现：throw 是运行时语句，配置场景应该用 check 块。

## 输出形态

最终给后续阶段提供：

```rust
pub struct SemaOutput {
    pub hir: HirModule,
    pub diagnostics: Vec<Diagnostic>,
}

pub enum Diagnostic {
    Lex(LexErrorKind, Span),
    Parse(ParseErrorKind, Span),
    Sema(SemaErrorKind, Span),
}

pub enum SemaErrorKind {
    DuplicateTopLevel,
    DuplicateField,
    DuplicateVariant,
    UndefinedName,
    AssignToReadonly,
    BreakOutsideLoop,
    ContinueOutsideLoop,
    YieldOutsideIterFn,
    ReturnValueInIterFn,
    SelfOutsideCheck,
    CheckBlockSideEffect,
    LocalTypeLeak,
    TypeMismatch,
    UnknownType,
    RecordKeyMixed,
    DictWithoutAnnotation,
    InvalidLiteral,
    NumberOverflow,
    InvalidEscape,
    ConfigDependsOnVar,
    ConfigCircularDependency,
    ConfigNonConstant,
    ConfigCheckFailed,
    ConfigTypeMismatch,
}
```

诊断与 lex/parse 的诊断合并为同一个 `Diagnostic` 枚举，外部消费方只看一个列表。

## 实现顺序

1. 建立 `hir.rs`，定义 `Ty`、`Value`、`HirModule`、`HirFunction`、`HirStmt`、`HirExpr` 骨架。
2. 实现 `Diagnostic` 与 `SemaErrorKind`，并把 lex/parse 错误统一适配进来。
3. 扩展 `ast::TypeExpr` 与 parser，加入函数类型语法 `fn(T1, ..., Tn) -> R`。
4. 实现 `sema::collect`（P1），跑通顶层声明收集 + 重复检查 + enum 自动编号 + 内建符号注入。
5. 实现 `sema::lower::scope`：作用域栈 + 名字查找 + upvalue 链。
6. 实现 `sema::lower::expr` 与 `sema::lower::stmt`：先不做类型检查，跑通 AST → HIR 形态转换。
7. 实现字面量规范化（数字解码、字符串转义）。
8. 实现上下文检查（break/continue/yield/return/self/loop_depth/check 块副作用）。
9. 实现 `Ty` 解析（含 `FunctionSig`）与 `sema::lower::tycheck` 兼容性规则。
10. 实现 fn 签名计算（按必填参数）与函数子类型不变规则。
11. 实现 lambda 上下文推断与调用点参数检查。
12. 实现 TypeGuard 插入（参数入口、return、var 赋值、Any/Function 收窄）。
13. 实现 `sema::lower::record`：Object/Dict 消歧 + class 字段校验 + 顶层 dict 强制标注诊断。
14. 实现 `sema::config_eval`（P3）：依赖图 + 拓扑 + 求值器 + class check 执行。
15. 公开 API local 类型泄露检查。
16. 用核心文档示例建立 sema fixture 测试。

阶段 1-8 是骨架，能跑出"无类型版"HIR；阶段 9-12 加类型校验和函数签名；阶段 13 是 RecordLiteral 消歧；阶段 14 是配置求值闭环；阶段 15-16 是收尾。

## 测试清单

测试分为正例 HIR 形态、诊断、配置求值结果三类。

### P1 声明收集

1. 6 类顶层声明都能被收集。
2. 同名顶层重复定义报 `DuplicateTopLevel`。
3. class 字段重复报 `DuplicateField`。
4. enum 变体重复报 `DuplicateVariant`。
5. enum 自动编号：`{ a, b = 5, c }` → `a=0, b=5, c=6`。
6. 内建符号（error / iter / range）已注入。

### 名字解析与作用域

1. 顶层名字可前向引用（fn 体里调用后定义的 fn）。
2. 局部 var 块级作用域：内层声明不污染外层。
3. 同块内 var 重复声明报错。
4. 参数与同名外层名字：参数遮蔽。
5. 闭包捕获外层 local：local 标记 `is_captured`，Closure 的 upvalues 列表正确。
6. 多级捕获：`outer → middle → inner` 的 `inner` 引用 `outer` 的 local，中间层正确建立 upvalue 链。
7. 未定义名字报 `UndefinedName`。
8. enum 变体访问 `Rarity.common` → `HirExpr::Variant`。
9. 给 import 别名/enum 变体/顶层 config/class 名/fn 名赋值报 `AssignToReadonly`。

### 上下文检查

1. 函数外 `break` / `continue` 报错。
2. 跨函数边界 `break` 报错（fn 内 break 不能跳出外层 loop）。
3. 普通 fn 中 `yield` 报错。
4. iter fn 中 `return value` 报错；不带值 `return` 合法。
5. check 块外 `self` 报错。
6. check 块内赋值 / 调用 fn 报 `CheckBlockSideEffect`。
7. 顶层普通语句报 `ExpectedItem`（parser 已拦，sema 兜底）。

### 字面量规范化

1. `1_000` → `Value::Int(1000)`。
2. `0xff` / `0b1010` / `0o755` 解码正确。
3. 整数溢出 i64 报 `NumberOverflow`。
4. 字符串 `"a\nb"` → `"a\nb"`（解码后）。
5. 原始字符串 `r"a\nb"` → `"a\nb"`（保留原文）。
6. 多行字符串保留换行。
7. 非法转义 `\x` / `\u` 报 `InvalidEscape`。

### 类型解析与检查

1. 基础类型 / array / dict / class / enum 都能解析。
2. 未知类型名报 `UnknownType`。
3. `var x: int = "s"` 报 `TypeMismatch`。
4. `var x: int = some_fn()` 插入 `TypeGuard`。
5. 函数参数有类型注解时入口插 guard。
6. 函数 `-> T` 时每个 `return` 包 guard。
7. 配置 `sword: Weapon = { id: "x", damage: 10 }` 类型检查通过。
8. 配置 `sword: Weapon = { id: 1 }` 报 `TypeMismatch`。
9. 配置缺少必填字段报 `TypeMismatch`。
10. 配置含多余字段报 `TypeMismatch`。
11. 公开 fn 参数类型引用 local class 报 `LocalTypeLeak`。
12. 公开 fn 返回类型 / 公开 class 字段类型 / 公开 var / 公开 config 引用 local 类型都报 `LocalTypeLeak`。

### 函数类型与签名

1. `fn(int, int) -> int` 解析为 `FunctionSig { params: [Int, Int], return_ty: Int }`。
2. `fn` 单独使用解析为 `Ty::Function`（顶类型）。
3. `fn(string)` 无返回类型 → `return_ty: Any`。
4. `fn add(a: int, b: int) -> int` 签名为 `FunctionSig { [Int, Int], Int }`。
5. `fn spawn(name: string, hp: int = 100)` 签名为 `FunctionSig { [String], Any }`（默认参数不进 params）。
6. `fn unknown(a, b)` 签名为 `FunctionSig { [Any, Any], Any }`。
7. `iter fn` 调用结果类型固定为 `Iterator`，不接受 `-> T` 标注。
8. `var f: fn(int) -> int = fn(x: int) -> int => x * 2` 通过。
9. `var f: fn(int) -> int = fn(x: string) -> int => 0` 报 `TypeMismatch`（参数不变规则）。
10. `var f: fn(int) -> int = fn(x: int) -> string => "a"` 报 `TypeMismatch`（返回不变规则）。
11. `var f: Function = fn(x: int) -> int => x` 通过（FunctionSig 是 Function 子类型）。
12. `var f: fn(int) -> int = some_dynamic_fn()` 在赋值点插 TypeGuard。
13. lambda `(x) => x * 2` 在 `Expect(fn(int) -> int)` 上下文下推断 `x: int` 与返回 `int`。
14. lambda 参数有显式注解但与上下文冲突时报 `TypeMismatch`。
15. callee 是 `FunctionSig { [Int, Int], Int }`，调用 `f(1)` 报"必填参数不足"。
16. callee 是 `FunctionSig`，调用 `f("a", 1)` 报参数类型不兼容。
17. callee 是 `Function` 顶类型，调用不做静态检查。
18. class 字段是 `apply: fn(any, any)`，配置赋值 lambda 时正确推断与校验。

### 对象/字典消歧

1. `{ a: 1, b: 2 }` 无上下文 → Object。
2. `{ "a": 1, "b": 2 }` 无上下文 → Dict<string, int>。
3. `{ "a": 1, "b": "x" }` 无上下文 → Dict<string, any>。
4. `{ a: 1, "b": 2 }` 无上下文 → 报 `RecordKeyMixed`。
5. `Foo = { a: 1 }` 在 `Foo: dict[string, int]` 上下文 → Dict。
6. `Foo: Weapon = { id: "x", damage: 10 }` → Object with class。
7. 顶层 dict 无类型标注报 `DictWithoutAnnotation`（按文档要求）。

### 脱糖

1. `until c { body }` → `While(Not(c), body)`。
2. `for i in 0..10` → `ForRange { inclusive: false }`。
3. `for i in 0..=10` → `ForRange { inclusive: true }`。
4. `0 < x <= 100`（x 是 Local）→ `AndChain([Lt(0, x), LtEq(x, 100)])`。
5. `0 < f() <= 100` → `Local(tmp, f()); AndChain(...)`，且 tmp 只求值一次。
6. `name ??= rhs` → `If(Eq(name, Null), Assign(name, rhs))`。

### 配置求值

1. 字面量 / 数组 / 对象 / 字典字面量直接产出 Value。
2. 配置引用配置：按拓扑序求值。
3. 配置依赖 var 报 `ConfigDependsOnVar`。
4. 配置循环依赖报 `ConfigCircularDependency`。
5. 配置中调用普通函数报 `ConfigNonConstant`。
6. 函数值作为配置：求值产出 `Value::Closure`，函数体不执行。
7. class check 通过的配置：求值成功。
8. class check 失败的配置：报 `ConfigCheckFailed`，message 为 check 的 message。
9. 字段默认值参与配置：缺省字段时填默认。
10. enum 值作为配置常量。

### 错误恢复

1. 一个文件多个未定义名字：全部报告，不止第一个。
2. `HirExpr::Error` 参与类型检查不传染。
3. 降级失败的子表达式不破坏整体 HIR 结构。

## 已确定决策

以下是从前期讨论中沉淀的决议，作为本计划的边界约束：

1. **跨模块支持**：第一版只支持单文件占位。`import` 语法在 P1 注册到 `imports` 表，引用 import 别名访问 `alias.name` 时 P2 报 `UnsupportedNotImplemented`，AST 不破坏。完整跨模块加载、循环 import 检查、跨模块类型引用、跨模块配置依赖图作为下一阶段独立任务。
2. **error 内建对象**：`error` 作为内建函数注入 `BuiltinSymbols`，调用 `error("msg")` 返回内置 error 对象（带 `.message` `.stack`）。`throw` 的实参形态在运行时由 VM 校验，HIR 阶段不强制（避免污染用户类型空间）。
3. **字符串 Unicode 转义**：第一版不支持 `\u{HEX}` 与 `\uHHHH`。需要 emoji 直接写字面量（标识符与字符串都允许 Unicode）。后续作为非破坏性扩展加入。
4. **顶层 dict 强制类型标注**：纯字符串 key 的字面量出现在顶层配置位置且无类型标注时，报 `DictWithoutAnnotation`。ident key 仍按对象处理，不受此规则影响。
5. **配置中的链式比较**：配置求值器不支持，遇到 `AndChain` 报 `ConfigNonConstant`。check 块内仍允许（check 在配置求值阶段执行但语义上是表达式判断）。
6. **HIR 节点 NodeId**：不加。诊断与回链使用 `Span` 做 key。后续若需要外挂边表（debugger / 优化 pass）再统一补 NodeId。
7. **公开 / local 类型可见性**：所有 class / enum 默认公开，`local` 才私有。公开 API（公开 fn 参数类型 / 返回类型、公开 class 字段类型、公开 var 类型、公开 config 类型标注）引用 local 类型时报 `LocalTypeLeak`，不自动降级可见性。
8. **函数类型完整签名**：`Ty` 区分 `Function`（顶类型）与 `FunctionSig { params, return_ty }`（具名签名）。
   - `TypeExpr` 扩展函数类型语法 `fn(T1, ..., Tn) -> R`
   - 函数声明按"必填位置参数"计算签名，默认参数和具名参数不进签名
   - 第一版函数子类型采用**不变规则**（参数与返回都精确匹配），不实现 PEC
   - lambda 在带类型上下文下做参数与返回类型推断
   - `Function ← FunctionSig` 自然成立；`FunctionSig ← Function` 需 TypeGuard 在赋值点运行时检查
   - 调用点：callee 是 `FunctionSig` 时静态检查参数数量与类型；callee 是 `Function` / `Any` 时不检查，运行时分派
