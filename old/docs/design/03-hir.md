# coflow HIR 设计

HIR（High-level Intermediate Representation）是语义分析的输出产物，也是代码生成的输入。它在结构上与 AST 相似，但已完成名称消解、脱糖和部分常量折叠，是一种比 AST 更接近执行语义的中间表示。

---

## 设计目标

- **名称消解**：所有名称引用替换为具体 ID，消除符号查找的歧义
- **脱糖**：将语法糖（`until`、链式比较等）展开为更简单的核心结构
- **类型携带**：函数签名、参数类型标注、类字段类型附着在节点上，供 codegen 使用
- **配置求值**：Config 全局变量的编译期值已计算完毕，附在模块顶层
- **错误恢复**：允许带错误的 HIR 存在，以支持继续分析和多诊断输出

---

## ID 系统

HIR 使用一组 newtype `usize` 作为各类实体的标识符，通过下标索引访问对应集合。

| ID 类型 | 索引目标 | 说明 |
|---|---|---|
| `GlobalId` | `HirModule::globals` | 模块顶层名称：config、var、fn、class、enum、import、builtin |
| `FunctionId` | `HirModule::functions` | 函数体，包含匿名函数、方法、迭代函数 |
| `ClassId` | `HirModule::classes` | 类定义 |
| `EnumId` | `HirModule::enums` | 枚举定义 |
| `VariantId` | 枚举内的 variants 列表 | 枚举变体，相对于所属 `EnumId` 索引 |
| `LocalId` | `HirFunction::locals` | 函数内局部变量和参数 |
| `UpvalueId` | `HirFunction::upvalues` | 闭包捕获的外层变量 |
| `BuiltinId` | 固定枚举 | 内置函数（error / iter / range / print） |

所有 ID 均实现 `Copy + Eq + Hash + Ord`，可直接用作集合键。访问时通过 `HirModule` 上的访问方法（如 `module.global(id)`）进行边界检查，返回 `Option<&T>`。

---

## 类型表示：`Ty`

```rust
enum Ty {
    Int, Float, Bool, String, Null, Any,
    Array(Box<Ty>),
    Dict(Box<Ty>, Box<Ty>),
    Class(ClassId),
    Enum(EnumId),
    Function,
    FunctionSig { params: Vec<Ty>, return_ty: Box<Ty> },
    Iterator,
    Error,
}
```

**设计决策：**

- **无联合类型**：类型系统是名义的、扁平的，不支持 `T | U`。`Any` 作为逃生舱，表示运行时动态类型。
- **无泛型**：`Array` 携带元素类型，`Dict` 携带键值类型，但无类型参数机制。泛型函数不在此类型系统中表示。
- **两种函数类型**：`Function` 是擦除了签名的函数类型（用于持有任意函数值），`FunctionSig` 携带完整签名（用于类型检查）。
- **`Error` 变体**：不表示运行时错误，而是编译期类型恢复占位符。当类型标注无法解析时使用，避免级联错误。
- **`Iterator`**：迭代器是独立类型，不是 `Array` 的子类型，在语义上代表惰性序列。

---

## 编译期常量值：`Value`

```rust
enum Value {
    Null, Bool(bool), Int(i64), Float(f64),
    String(Rc<str>),
    Array(Rc<Vec<Value>>),
    Dict(Rc<Vec<(Value, Value)>>),
    Object { class: Option<ClassId>, fields: Rc<Vec<(String, Value)>> },
    Range { start: i64, end: i64, inclusive: bool },
    EnumVariant(EnumId, VariantId),
    Closure(Rc<ClosureData>),
}
```

`Value` 专门用于 Config 全局变量的编译期求值，不用于运行时。

**设计决策：**

- **`Rc` 共享所有权**：字符串、数组、字典、对象都通过 `Rc` 持有，使 `Value` 可以廉价克隆（克隆只是引用计数增加）。这对 Config 求值过程中反复传递值至关重要。
- **`Object` 的 `class: Option<ClassId>`**：允许无类型标注的匿名对象存在（`None`），同时也支持类型化对象（`Some(ClassId)`），统一在同一变体中。
- **`Closure` 仅含 `fn_id`**：函数值作为编译期常量是合法的（可以赋给 Config），但函数调用结果不属于常量，不能在编译期求值。`ClosureData { fn_id: FunctionId }` 只保留足够 codegen 使用的信息。
- **`Dict` 使用有序列表**：键值对存储为 `Vec<(Value, Value)>` 而非 `HashMap`，保持插入顺序，与语言语义一致。

---

## 模块结构：`HirModule`

```rust
struct HirModule {
    globals: Vec<HirGlobal>,
    functions: Vec<HirFunction>,
    classes: Vec<HirClass>,
    enums: Vec<HirEnum>,
    config_eval_order: Vec<GlobalId>,
    config_values: Vec<(GlobalId, Value)>,
}
```

各集合通过 ID 下标直接访问：`module.globals[id.0]`。`config_eval_order` 存储拓扑排序后的 Config 求值顺序，`config_values` 存储求值成功的 Config 最终值。两者均在语义分析的第三阶段填充。

---

## 全局声明：`HirGlobal`

| 变体 | 说明 |
|---|---|
| `Config` | 编译期常量，含可选类型标注和初始化表达式 |
| `Var` | 运行时全局变量，含 `local` 可见性标志 |
| `Function` | 顶层函数，持有 `FunctionId` 引用 |
| `Class` | 类声明，持有 `ClassId` 引用 |
| `Enum` | 枚举声明，持有 `EnumId` 引用 |
| `Import` | 导入声明，持有模块名 |
| `Builtin` | 内置函数，持有 `BuiltinId` |

`HirGlobal` 统一使用 `id()` / `name()` / `span()` 方法访问公共属性。`Class` 和 `Enum` 的 `name()` 返回 `None`，因为名称存储在各自的 `HirClass` / `HirEnum` 中。

---

## 函数表示：`HirFunction`

```rust
struct HirFunction {
    is_iter: bool,
    params: Vec<HirParam>,
    return_ty: Option<Ty>,
    locals: Vec<HirLocal>,
    upvalues: Vec<UpvalueDesc>,
    body: Vec<HirStmt>,
    span: Span,
    signature: Ty,
}
```

- `locals` 包含全部局部变量（含参数），参数的 `LocalId` 在 `HirParam::local_id` 中保存。
- `upvalues` 描述闭包捕获链，每个 `UpvalueDesc` 是 `Local(LocalId)`（直接来自父函数局部变量）或 `Upvalue(UpvalueId)`（来自更外层，需要在父函数的 upvalue 列表中继续查找）。这种两级描述使 codegen 可以直接生成正确的 upvalue 捕获指令，无需重新分析作用域链。
- `signature: Ty` 是 `Ty::FunctionSig { params, return_ty }` 的实例，供调用方做类型检查。
- `HirLocal::is_captured` 标记该局部变量是否被内层闭包捕获，codegen 据此决定是否将变量提升到堆上。

---

## 语句：`HirStmt`

相对于 AST，HIR 语句层有以下差异：

| AST 节点 | HIR 节点 | 变化 |
|---|---|---|
| `Until { cond, body }` | `While { cond: Not(cond), body }` | 脱糖为条件取反的 while |
| `VarDecl` | `Local { id: LocalId, init }` | 名称替换为 `LocalId` |
| `ForIn { var, iter }` | `ForIn { item: LocalId, iter }` | 通用迭代路径，变量名替换为 ID |
| `ForIn`（range 字面量）| `ForRange { var, start, end, inclusive, body }` | range 迭代识别为专用节点 |

`ForRange` 是一个优化路径：当 `for i in start..end` 或 `for i in start..=end` 的迭代对象在语义分析时可确认为整数 range 时，直接生成此节点，使 codegen 能输出高效的计数循环，而非通用迭代器协议。

`If` 语句的 `else_` 字段是 `Option<Vec<HirStmt>>`，嵌套 `else if` 展开为嵌套 `If` 列表，与 AST 保持同构，不做额外展平。

---

## 表达式：`HirExpr`

相对于 AST，HIR 表达式层的关键差异：

| 节点 | 说明 |
|---|---|
| `Const { value: Value }` | 所有字面量统一表示为常量值，字符串转义和数值解析已完成 |
| `Local(LocalId)` / `Upvalue(UpvalueId)` / `Global(GlobalId)` | 名称引用消解为 ID，无符号查找开销 |
| `Variant { enum_id, variant }` | 枚举变体引用，与普通名称引用分离 |
| `Closure { fn_id }` | 匿名函数和命名函数引用，统一持有 `FunctionId` |
| `SelfField { name }` | check 块内的 `self.field` 访问，限定只能在 check 上下文中出现 |
| `AndChain { exprs }` | 链式比较展开：`0 < x <= 10` 转为 `AndChain([0 < x, x <= 10])`，保证子表达式只求值一次 |
| `NullCoalesce { left, right }` | `??` 运算符单独抽出，短路语义不同于普通二元运算，codegen 需特殊处理 |
| `Object { class, fields, spreads }` | 对象字面量，`spreads` 保留展开表达式列表，在 codegen 阶段处理 |
| `Dict { entries }` | 字典字面量，与 `Object` 在语义分析阶段消歧完成 |
| `TypeGuard { expr, ty }` | 运行时类型检查节点，源自带类型标注的参数或配置，codegen 生成断言代码 |
| `Error(Span)` | 错误恢复占位，在解析失败的子表达式位置插入，允许上层继续分析 |

`OptField` 和 `OptIndex` 是可选链访问（`obj?.field`、`obj?[idx]`），对象为 null 时短路返回 null，与 `Field` / `Index` 分离为独立节点。

---

## 赋值目标：`HirAssignTarget`

```rust
enum HirAssignTarget {
    Local(LocalId),
    Upvalue(UpvalueId),
    Global(GlobalId),
    Field { obj: Box<HirExpr>, field: String, span: Span },
    Index { obj: Box<HirExpr>, index: Box<HirExpr>, span: Span },
}
```

赋值目标与表达式分离为独立类型，使语义分析可以在赋值左侧静态验证只读性（`Global` 对应 `GlobalEntry::readonly()` 检查）。

---

## 错误恢复策略

HIR 允许携带错误状态存在，通过以下机制支持继续分析：

- `HirExpr::Error(Span)` 在无法降级的表达式位置插入
- `Ty::Error` 在类型无法解析时作为类型占位
- 每个错误节点都对应一条 `Diagnostic::Sema` 诊断，不会静默丢弃

这使得单个文件中的多个错误可以在一次分析中全部报告，而不是遇到第一个错误就停止。
