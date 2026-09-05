# C# VM 设计

> 状态：当前内部设计契约
>
> 范围：函数 IR lowering 之后的 program、寄存器、指令、调用 ABI、frame、closure、fault、执行限制、
> 验证和性能约束。

当前实现已经覆盖 register lowering、program verifier、三类寄存器、普通/尾调用、closure、source-mapped
fault 和 thread-local context pool。第 13 节的执行限制尚未实现，是本文唯一仍未闭合的安全契约；因此
当前 VM 不能作为不可信脚本 sandbox，死循环也不会由 VM 自行终止。第 15、16 节包含需要持续验证的
约束，不表示每一项均已达到。

语言语义见 `01-language-design.zh-CN.md`；Module、Host 和生成 ABI 见
`02-api-runtime-design.zh-CN.md`。VM 只执行 `LoadAndCompile` 产生的内部 program，不接受外部 bytecode。

## 1. 目标与边界

VM 的目标是以静态类型、低分配的执行结构承载 CFD 函数语义，同时保留准确 source span 和 Coflow
调用栈。

固定约束为：

- 生成类型是唯一语义类型系统，VM 不建立动态 value hierarchy。
- primitive 存放在专用寄存器区，不装箱到 `object`。
- program、常量池、调用布局和 source map 在 Module 发布后不可变。
- 普通调用使用显式 frame，不用 CLR 递归栈表达 Coflow 调用栈。
- 尾调用不增加 frame depth。
- 执行限制是安全契约，不能依赖 trace、profile 或宿主超时。
- 当前 VM 不保存可恢复状态，不支持暂停、异步或后台调度。

执行结构为：

```text
CoflowProgram
  -> CoflowExecutionContext
     ├── register banks
     ├── current program and pc
     ├── current register bases
     ├── call frames
     └── execution limits (planned)
```

## 2. 编译与 lowering 边界

VM 不执行源码类型检查。编译管线在进入 VM 前完成：

```text
typed function AST
  -> typed control-flow representation
  -> capture analysis
  -> call target analysis
  -> virtual register allocation
  -> register instruction lowering
  -> source-map lowering
  -> program validation
  -> immutable CoflowProgram
```

typed representation 负责语义类型、控制流、返回路径和 callable 签名；register lowering 负责三类物理
寄存器、常量区、参数窗口、返回窗口和 descriptor 索引。内部 program 不序列化，不定义持久格式版本。

优化只能保持语言语义和 source span：常量折叠、死代码删除、分支简化、无效 move 删除、寄存器复用和
尾调用识别在 lowering 前后完成。不能用局部代码模式专用 opcode 替代通用语义指令。

## 3. 值形状与寄存器区

每个静态值具有物理形状：

```text
CoflowValueShape
├── semantic type
├── shape kind
│   ├── unit
│   ├── scalar
│   ├── option
│   └── result
├── integer width
├── float width
├── reference width
└── nested payload shape(s)
```

执行上下文使用三个连续寄存器区：

```text
CoflowExecutionContext
├── IntegerRegisters: long[]
├── FloatRegisters: double[]
├── ReferenceRegisters: object?[]
├── Frames: CoflowFrame[]
├── current bases
├── current tops
└── high-water marks
```

类型映射为：

| 语义类型 | 物理存储 |
| --- | --- |
| `int` | integer register |
| `bool` | integer register，规范值为 0 或 1 |
| enum | integer register |
| `float` | float register |
| `string` | reference register |
| 生成记录或 object | reference register |
| list / dictionary | reference register |
| function / Delegate / closure | reference register |
| unit | 零宽布局 |
| Option / Result | integer tag 加递归 payload 布局 |

reference register 只保存 CLR 引用。Option/Result 的 inactive payload 不参与语义读取；复制和传播按照
静态 shape 的各寄存器宽度执行，不能将复合值临时装箱。

## 4. Program 布局

每个 program 静态保存：

```text
CoflowProgram
├── function identity
├── source path and function span
├── instructions[]
├── instruction source spans[]
├── integer constants[]
├── float constants[]
├── reference constants and descriptors[]
├── parameter layouts[]
├── return layout
├── integer register count
├── float register count
└── reference register count
```

寄存器 count 由 lowering 根据实际布局生成，执行时不扫描指令重新计算。常量按照物理类别保存；复合
常量携带预编码 shape 与目标布局。descriptor 可以描述 field access、native adapter、CallSite、closure
template 和复合值 transfer，但不能把可变执行状态放入 program。

## 5. 指令模型

指令是固定宽度的静态操作描述：

```text
RegisterInstruction
├── opcode
├── operand A
├── operand B
├── operand C
└── parallel source-span entry
```

opcode 类别包括：

- 常量加载和同类寄存器 move。
- 复合值构造、payload 读取、tag 读取和传播。
- 强类型 field access 与 native adapter。
- 显式数值转换和类型测试。
- 整数、浮点、字符串、位和比较运算。
- 条件跳转和无条件跳转。
- direct/indirect call、direct/indirect tail call 和 return。
- closure 构造。

算术和比较直接在 dispatch 分支执行，不通过每条指令 Delegate 二次分派。整数运算必须遵循语言的
checked 语义；常量折叠和运行时 opcode 必须对溢出、除零、移位、转换和浮点特殊值保持一致。

field descriptor 按结果物理类别提供固定读取入口。数值字段不能经过 `object` 装箱；生成 reader 只做
一次接收者类型转换并读取强类型字段。

## 6. Program verifier

Program 在 Module 发布前验证一次。verifier 必须拒绝：

- 未知 opcode 或不完整 operand。
- 越界的寄存器、常量、descriptor、argument、local 和 jump target。
- operand 静态类型与寄存器类别不一致。
- 读取未赋值 local 或同一 local 在路径间改变类型。
- 控制流合并点的 value stack/layout 不一致。
- 返回值类型、宽度或路径终止行为不一致。
- CallSite arity、参数、返回值或目标签名不一致。
- 间接 callable 的 Delegate 签名不匹配。
- closure capture 数量、顺序、类型或目标参数布局不匹配。
- Option/Result 构造、传播或 reinterpret 的 shape 不兼容。

验证后的执行循环不重复做静态类型检查。当前执行时检查 null receiver、Host 返回、间接目标和算术
fault；执行限制按第 13 节仍待实现。

## 7. 参数、返回值与 CallSite

静态值位置由三类 base 和 shape 组成：

```text
CoflowValueRegister
├── shape
├── integer base
├── float base
├── reference base
├── tag location?
├── first payload layout?
└── second payload layout?
```

CallSite 保存调用者实参位置到目标 parameter layout 的映射：

```text
CoflowCallSite
├── target function entry
├── argument layouts[]
├── target parameter layouts[]
├── copy requirements[]
├── callee window bases
└── result layout
```

direct call 在进入目标 program 前准备参数。若目标窗口可以安全复用调用者预留区，则直接复制；存在
覆盖风险时先写入静态 scratch 区。调用过程中不创建 `object[]` 或 per-call 参数 descriptor。

return 按静态 shape 把被调用函数返回布局复制到调用者 result layout。unit 返回是零宽操作。

## 8. Frame 与寄存器窗口

Frame 只保存返回现场：

```text
CoflowFrame
├── caller program
├── return pc
├── integer base
├── float base
├── reference base
└── return target layout
```

非尾调用步骤为：

```text
save caller frame
  -> choose callee bases
  -> reserve callee register counts
  -> copy arguments
  -> switch current program and pc
  -> execute
  -> copy return value
  -> clear released references
  -> restore caller frame
```

Frame 存在池化连续数组中，不使用 `Stack<T>`，也不为每个调用分配 locals 数组。递归调用通过独立窗口
保护调用者值。

尾调用不 push frame。参数先进入不会被旧窗口覆盖的区域，再压缩到当前 frame 的 bases，清理不再使用
的 reference slot，最后替换 program 和 pc。相互尾调用必须同样保持窗口有界。

## 9. 间接调用与 Host

间接调用目标可以是 function entry、VM closure 或受支持的 CLR Delegate adapter。所有目标先匹配
静态签名，再进入相同参数和返回布局。

```text
IndirectCallable
├── CompiledFunctionEntry
│   └── enter program
├── HostFunctionEntry
│   └── invoke configured adapter
├── VmClosure
│   └── enter program with captures
└── AdaptedDelegate
    ├── VM callable target?
    └── native adapter?
```

Host adapter 从当前 context 的参数布局读取强类型参数，并把结果写回 result layout。primitive 不装箱，
复合值由 boundary codec 递归读写。Host 未绑定、签名不匹配、返回无效值或抛出异常时进入当前调用位置
的 fault 边界。

VM -> Host -> VM 同步重入的 context、预算和调用栈归属必须显式定义并测试，不能由 ThreadStatic pool
是否恰好存在空闲 context 决定。执行限制落地时，默认要求整条同步调用链共享工作量预算和诊断链。

## 10. Closure

closure 按值捕获并按物理类别保存：

```text
CoflowClosure
├── target program
├── capture layouts[]
├── integer captures[]
├── float captures[]
└── reference captures[]
```

小 capture 数量可以使用内联字段，大 capture 数量使用数组，但语义和索引顺序一致。构造 closure 时从
当前寄存器复制静态 capture layout；调用时把显式参数与 capture 写入目标 parameter layout。

closure 对象是必要分配。capture、参数和 callback 调用过程不能额外创建临时 `object[]`。外部持有的
Delegate adapter 必须保持 closure 及其所属 Module 可达，释放后不进入全局增长缓存。

## 11. 高阶集合执行

map、filter、fold、find、any 和 all 被 lowering 为显式循环与正常 indirect callback call。运行时 adapter
只负责强类型集合读取和必要的结果 builder。

```text
HigherOrderLoop
├── source collection
├── callback callable
├── count
├── index
├── current item
├── callback result
└── accumulator or builder
```

数组直接按 `IReadOnlyList<T>` 读取，不预复制元素。dictionary 在进入循环时通过 `ToArray()` 建立一次稳定
枚举快照；执行限制落地后，必须把该分配和元素工作量纳入操作成本。每个 callback 通过正常 frame
执行，不为每个元素创建参数数组。

find/any/all 在满足结果后立即短路；fold 从显式初值开始；map/filter 只为最终结果 builder 分配。

## 12. Fault 与 source map

每条发射指令关联源表达式 span。执行循环在 dispatch 前保存当前 program 和 pc，使 CLR 异常能够转换为
准确的 `CoflowFaultException`。

```text
CoflowFault
├── function identity
├── source path
├── source span
├── Coflow call stack[]
├── message
└── inner exception?
```

调用栈由显式 frame 和当前 program 构造，并限制展示深度。fault 不暴露寄存器、常量池或 descriptor。
已经是 Coflow fault 的异常跨调用边界传播时必须保留已有诊断链，同时按重入契约保留外层调用关系。

## 13. 执行限制

执行限制是尚未完整实现的强制契约。每个最外层同步调用至少维护：

```text
ExecutionLimits
├── remaining instruction work
├── maximum frame depth
├── maximum integer registers
├── maximum float registers
├── maximum reference registers
└── maximum linear collection work
```

每条 VM 指令消耗工作量；native 线性集合操作按实际扫描元素额外消耗工作量。push frame、reserve window
和数组扩容前检查对应上限。超限产生 source-mapped fault，不能继续扩容到 CLR OOM，也不能让死循环
永久占用线程。

Host 同步调用本身计入调用工作量；Host 回调 VM closure 继续使用同一最外层预算。预算不提供语言级
捕获或恢复入口。

在限制实现前，VM 只适合执行受信任、可控的 CFD 函数。

## 14. Context pool 与清理

execution context 按线程池化以复用寄存器和 frame 数组。扩容使用连续数组，并保留 high-water mark：

```text
ContextLifecycle
├── rent or allocate
├── reset scalar state
├── reserve initial program
├── execute
├── clear used reference registers
├── clear used frames
└── return to thread-local pool
```

归还前清理所有曾写入的 reference slot 和 frame 引用，避免延长对象、Module、Delegate 或 closure 生命周期。
primitive 数组不要求清零以保证 GC，但 reset 后不得读取未赋值位置。

重入中的活动 context 不能同时进入 pool。pool 只负责复用，不定义调用链或预算语义。

## 15. 性能约束

热路径必须避免：

- primitive 装箱。
- per-call 参数数组、locals 数组和 frame 对象。
- per-instruction Delegate 分派。
- per-item callback 参数分配。
- 每次调用的反射、泛型构造和 descriptor 建立。

允许的必要分配包括 closure、最终集合结果、首次扩容和加载期缓存。JIT、IL 生成、`unsafe`、direct
threading 或压缩 bytecode 只有在 Release 基准证明 dispatch 或指令带宽是主要瓶颈后才考虑。

## 16. 验证与基准矩阵

VM 正确性测试至少覆盖；其中执行限制相关项目在第 13 节实现后补齐：

- int、float、bool、string、enum、unit、Option、Result 和嵌套复合布局。
- 常量、move、field、算术、比较、短路、跳转和 return。
- checked overflow、除零、非法转换、移位边界和 source span。
- direct/indirect call、非尾递归、尾递归和相互尾调用。
- Host 参数/返回、异常、重配置与 VM/Host/VM 重入。
- closure capture、返回 Delegate 和多次 Host 往返。
- 高阶集合空值、短路和结果；工作量收费随执行限制补齐。
- verifier 的所有非法 opcode、operand、layout、control-flow 和 signature 情况。
- 待补：指令、frame、寄存器、集合工作量的边界与首个超限值。
- context 清理、旧 Module 可回收和 warmed hot path 分配。

基准至少包含整数与浮点循环、函数调用链、尾递归、非尾递归、字段读取、集合 pipeline、closure、Host
往返和重入。结论只使用 Release 构建的吞吐量与 managed allocation 数据。

## 17. 固定不支持的 VM 能力

VM 不支持：

- 外部 bytecode、bytecode artifact 或持久格式兼容。
- 不可信 bytecode loader 和通用 verifier sandbox。
- coroutine、暂停、异步 Host、completion queue 或 scheduler。
- 动态类型寄存器、通用 `object` value stack 或运行时 opcode 扩展。
- trace/profile 对正常执行语义或限制判断的依赖。
