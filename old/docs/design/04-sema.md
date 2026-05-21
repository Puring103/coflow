# coflow 语义分析设计

语义分析（sema）是 coflow 编译流水线中 parser 之后、codegen 之前的阶段。它以 AST 为输入，输出 HIR 和诊断信息列表。

---

## 设计目标

- **单遍模型**：三个子阶段顺序执行，无回溯，诊断信息在各阶段就地收集
- **错误容忍**：lex / parse / sema 错误统一收集，不在首个错误处中断
- **配置求值**：Config 全局变量在编译期完成常量折叠与类型验证
- **名称消解**：符号引用转为稳定 ID，消除后续阶段对名称字符串的依赖

---

## 公开接口

```rust
fn analyze_source(source: &str) -> SemaOutput
fn analyze_module(module: &Module) -> SemaOutput

struct SemaOutput {
    hir: HirModule,
    diagnostics: Vec<Diagnostic>,
}
```

`analyze_source` 从源码字符串开始，内部调用 lexer + parser，再调用 `analyze_module`。`analyze_module` 从已有 AST 开始，适合测试和增量场景。

**错误不中断策略**：lex 错误和 parse 错误转换为 `Diagnostic` 收集起来，不立即终止分析。唯一的中断点是 parse 返回 `None` 模块（源码完全无法解析），此时跳过语义分析，返回空 HIR。

---

## 诊断类型

```rust
enum Diagnostic {
    Lex(LexErrorKind, Span),
    Parse(ParseErrorKind, Span),
    Sema(SemaErrorKind, Span),
}
```

三类错误统一在一个 `Vec<Diagnostic>` 中，调用方无需区分来源即可遍历所有问题。每条诊断携带 `Span`，可通过 `diagnostic.span()` 统一取出。

---

## `SemaErrorKind` 完整列表

| 错误码 | 触发场景 |
|---|---|
| `DuplicateTopLevel` | 顶层名称重复声明（含用户声明与内置名称冲突） |
| `DuplicateField` | 类字段名重复 |
| `DuplicateVariant` | 枚举变体名重复 |
| `DuplicateLocal` | 同作用域内局部变量名重复 |
| `UndefinedName` | 引用未声明的名称 |
| `AssignToReadonly` | 对非 Var 全局变量赋值 |
| `BreakOutsideLoop` | `break` 出现在循环体外 |
| `ContinueOutsideLoop` | `continue` 出现在循环体外 |
| `YieldOutsideIterFn` | `yield` / `yield from` 出现在非迭代函数中 |
| `ReturnValueInIterFn` | 迭代函数中 `return` 携带值 |
| `SelfOutsideCheck` | `self` 关键字出现在 check 块外 |
| `CheckBlockSideEffect` | check 块中出现有副作用的操作 |
| `LocalTypeLeak` | `local` 声明的类型在公开接口中泄露 |
| `TypeMismatch` | 类型标注与实际类型不符 |
| `UnknownType` | 类型标注中出现未知类型名 |
| `RecordKeyMixed` | 对象字面量中字符串键与标识符键混用 |
| `DictWithoutAnnotation` | 动态键字典缺少类型标注无法消歧 |
| `InvalidLiteral` | 字面量格式不合法 |
| `NumberOverflow` | 整数或浮点数字面量超出范围 |
| `InvalidEscape` | 字符串转义序列不合法 |
| `ConfigDependsOnVar` | Config 的初始化表达式引用了运行时 Var |
| `ConfigCircularDependency` | Config 之间存在循环依赖 |
| `ConfigNonConstant` | Config 初始化表达式包含非常量操作 |
| `ConfigCheckFailed` | Config 的 check 块断言在编译期失败 |
| `ConfigTypeMismatch` | Config 求值结果与类型标注不符 |
| `UnsupportedNotImplemented` | 语法上合法但尚未实现的特性 |

---

## 三阶段处理流程

`analyze_module` 顺序调用三个子阶段，每个阶段的输出是下一阶段的输入。

```
AST
 │
 ▼  P1: collect
ModuleSymbols + diagnostics
 │
 ▼  P2: lower
HirModule (无 config_values) + diagnostics
 │
 ▼  P3: config_eval
HirModule (含 config_values + config_eval_order) + diagnostics
 │
 ▼
SemaOutput { hir, diagnostics }
```

---

## P1：符号收集（`collect.rs`）

**职责**：扫描 AST 顶层 `Item` 列表，为每个名称分配 ID，建立 `ModuleSymbols`。不进入函数体内部。

**输出结构：**

```rust
struct ModuleSymbols {
    globals: Vec<(String, GlobalEntry)>,
    classes: Vec<ClassInfo>,
    enums: Vec<EnumInfo>,
    imports: Vec<ImportInfo>,
    builtins: BuiltinSymbols,
}
```

`globals` 按声明顺序排列，`GlobalId` 等于其在 `globals` 中的下标。`ModuleSymbols::get_global(name)` 线性查找返回条目引用。

**`GlobalEntry` 变体：**

| 变体 | 含义 | 关键字段 |
|---|---|---|
| `Config` | 编译期常量声明 | `id`, `ty: Option<TypeExpr>`, `ast_index` |
| `Var` | 运行时全局变量 | `id`, `ty: Option<TypeExpr>`, `ast_index` |
| `Function` | 顶层函数 | `id`, `fn_id`, `is_iter`, `ast_index` |
| `Class` | 类声明 | `id`, `class_id`, `ast_index` |
| `Enum` | 枚举声明 | `id`, `enum_id`, `ast_index` |
| `Import` | 导入声明 | `id`, `import_id`, `ast_index` |
| `Builtin` | 内置函数 | `id`, `builtin_id` |

`ast_index` 指向原始 `Module::items` 列表中对应节点的位置，lower 阶段据此回访 AST 节点。`GlobalEntry::readonly()` 对除 `Var` 以外的所有变体返回 `true`，赋值检查依赖此方法。

**内置函数追加策略**：四个内置函数（error / iter / range / print）在用户声明收集完毕后追加到 `globals` 末尾。若用户已声明同名符号，则报 `DuplicateTopLevel` 并跳过追加。这保证内置名称不会静默覆盖用户声明。

**`FunctionId` 占位**：collect 阶段函数条目的 `fn_id` 设为 `FunctionId(usize::MAX)`，是占位值。真实 `FunctionId` 在 lower 阶段函数体降级时分配并回填。

**类和枚举的子元素检查**：`collect_class_info` 检测重复字段名（报 `DuplicateField`），`collect_enum_info` 检测重复变体名（报 `DuplicateVariant`），并计算枚举变体的整数值（自动递增，支持显式指定）。

---

## P2：AST 降级（`lower.rs`）

**职责**：遍历 AST，借助 `ModuleSymbols` 完成名称消解，将所有节点降级为 HIR。

**关键上下文结构：**

```rust
struct LowerCtx {
    symbols: &ModuleSymbols,
    module: HirModule,
    fn_stack: Vec<FnCtx>,      // 嵌套函数栈
    diagnostics: &mut Vec<Diagnostic>,
    top_level_function_ids: HashMap<usize, FunctionId>,
    in_check_block: bool,
    checking_config_without_ty: bool,
}

struct FnCtx {
    fn_id: FunctionId,
    is_iter: bool,
    return_ty: Option<Ty>,
    scopes: Vec<Scope>,        // 块作用域栈
    loop_depth: usize,
    locals: Vec<HirLocal>,
    upvalues: Vec<UpvalueDesc>,
    upvalue_index: HashMap<UpvalueDesc, UpvalueId>,
}
```

**名称查找顺序**：块作用域（内层优先）→ 函数参数 → 模块作用域（`ModuleSymbols`）。跨函数边界的引用触发 upvalue 捕获链构建。

**Upvalue 捕获链**：当一个名称在内层函数中引用到外层函数的局部变量时，lower 阶段沿 `fn_stack` 向外遍历，在经过的每一层函数的 `upvalues` 列表中注册捕获描述。最终的 `UpvalueDesc` 是：
- `Local(LocalId)`：直接来自父函数的局部变量
- `Upvalue(UpvalueId)`：来自更外层，通过父函数的 upvalue 转发

**类型标注解析**：`TypeExpr`（AST 中的类型语法节点）在此阶段解析为 `Ty`。无法识别的类型名报 `UnknownType`，结果为 `Ty::Error`。

**字面量处理**：
- 整数和浮点数在 AST 中保留 `raw: String`，lower 阶段解析为 `i64` / `f64`，溢出报 `NumberOverflow`
- 普通字符串处理转义序列，非法转义报 `InvalidEscape`；原始字符串（`r"..."`）直接使用 raw 内容不做处理

**对象与字典消歧**：对象字面量（`{ key: val }`）根据键的形式和上下文判断：
- 所有键为标识符 → `HirExpr::Object`
- 所有键为动态表达式 → `HirExpr::Dict`
- 混用 → 报 `RecordKeyMixed`
- 仅有标识符键但无类型上下文时可能报 `DictWithoutAnnotation`

**脱糖规则：**
- `until cond { body }` → `while not cond { body }`（条件取反）
- `for i in start..end { body }` → `ForRange { var, start, end, inclusive: false, body }`（range 特化）
- 链式比较 `a < b < c` → `AndChain([a < b, b < c])`（中间变量只求值一次）

**Check 块约束**：在 `in_check_block = true` 状态下，lower 阶段拒绝有副作用的表达式（报 `CheckBlockSideEffect`），并允许 `SelfField` 节点存在。check 块外出现 `self` 报 `SelfOutsideCheck`。

**循环和 yield 上下文**：`loop_depth` 跟踪循环嵌套深度，`break` / `continue` 在 `loop_depth == 0` 时报错。`is_iter` 标记当前函数是否为迭代函数，`yield` 在非迭代函数中报错，迭代函数中的 `return value` 报 `ReturnValueInIterFn`。

---

## P3：Config 求值（`config_eval.rs`）

**职责**：对 HIR 中所有 Config 全局变量按拓扑序求值，计算编译期常量值，执行类型验证和 check 断言。

**处理流程：**

1. **依赖收集**：遍历每个 Config 的 `HirExpr`，收集其中引用的其他 Config ID，构建依赖图。若发现引用了 `Var`，报 `ConfigDependsOnVar`（Var 是运行时变量，不能在编译期求值）。
2. **拓扑排序**：对依赖图做 DFS，检测环路。有环路时报 `ConfigCircularDependency`，跳过相关 Config。排序结果写入 `HirModule::config_eval_order`。
3. **逐一求值**：按拓扑序对每个 Config 的 `HirExpr` 解释执行。求值是一个受限的表达式解释器，只允许常量子集（字面量、其他 Config 引用、基本算术/逻辑运算等）。遇到不允许的操作报 `ConfigNonConstant`。
4. **类型验证**：若 Config 有类型标注，将求值结果与 `Ty` 对比。类型不符报 `ConfigTypeMismatch`。带 `Class` 类型标注时验证对象的字段结构是否与类定义匹配。
5. **Check 执行**：若对应类有 check 块，在当前对象值的上下文中执行每条 check 断言。断言求值为 `false` 时报 `ConfigCheckFailed`。
6. **结果存储**：求值成功的 Config 以 `(GlobalId, Value)` 追加到 `HirModule::config_values`。

**关键约束**：Config 只能依赖其他 Config，不能依赖 Var。这是静态性保证：Config 的值在编译期完全确定，不受运行时输入影响。

---

## 设计决策汇总

| 决策 | 理由 |
|---|---|
| 三阶段串行而非单遍 | collect 需要在 lower 开始前建立完整的顶层名称表，否则前向引用无法解析 |
| 诊断统一列表 | 调用方（IDE、CLI）无需分层处理，统一按 span 排序展示 |
| parse 失败时跳过语义分析 | 模块为 None 时无法建立任何符号表，强行继续只会产生大量虚假错误 |
| 内置名称追加而非预置 | 允许用户代码覆盖内置名称时给出明确错误，而不是静默影子遮蔽 |
| `ast_index` 保留 AST 回访能力 | lower 阶段需要访问 AST 节点的完整信息（函数体等），通过索引而非指针避免生命周期问题 |
| Config 求值在独立阶段 | 依赖图构建需要完整 HIR，无法在 lower 阶段内联完成 |
