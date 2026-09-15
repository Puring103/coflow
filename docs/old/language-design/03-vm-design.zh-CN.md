# C# VM 设计

> 状态：内部设计契约
>
> 范围：物理布局、Arena、program、指令、调用 ABI、closure、Host 边界、验证、预算和性能。

语言语义见 `01-language-design.zh-CN.md`；Module、快照和生成 codec 边界见
`02-api-runtime-design.zh-CN.md`。VM 只执行 `Coflow.Compile()` 产生并验证的内存内 program，不接受
外部 bytecode。

## 1. 目标与边界

VM 使用 Schema 预生成的静态布局和低分配寄存器执行 CFD 函数，同时保留精确 source span 和 Coflow
调用栈。VM 数据与应用可读的 C# 对象是同一规范化候选生成的两份不可变表示；执行不通过 C# 对象读取
配置字段。

固定约束为：

- primitive 不装箱，复合值按静态 layout 传递。
- value ID 保存在 integer lane，verifier 根据静态 layout 区分普通整数和 ID。
- 普通调用使用显式 frame；尾调用不增加 frame depth。
- program、Arena、descriptor 和链接表在快照发布后不可变。
- 执行限制属于语义边界，不能依赖宿主超时。
- VM 同步执行，支持同线程 Host -> VM 重入，不支持暂停和异步恢复。

## 2. 预生成物理布局

生成 Schema 提供声明类型和其签名中已知 closed 类型的固定 `Layout`。Module 编译器根据已解析的
函数语义类型补全函数体引入的 closed 组合布局：

```text
Layout
├── TypeId / LayoutId
├── kind
├── integer / float / reference widths
├── field offsets
├── nested layouts
├── collection layout
├── flattened ABI
└── function-set requirement
```

布局补全只在 Module 编译作用域内发生，并在候选快照发布前结束。执行、Host 边界和调用期导入只能
按 closed type key 查询已有布局，不能创建布局。生成 struct 始终递归展开。Option 和 Result 使用
integer tag 加静态 payload layout；inactive payload 不参与语义读取。
集合只在 collection store 中保存，寄存器和 Arena 保存其稳定索引。

集合编码先预留外层的连续列空间，再递归写入字段和嵌套集合。struct writer 与普通值编码器直接写入
目标列，集合元素不创建独立的 encoded 列数组。嵌套集合追加到预留空间之后。

复合值相等通过布局和只读列视图执行，不将集合还原为 CLR 容器。Option/Result 只比较活动分支，
struct 比较字段并跳过身份列，record 保持对象身份相等，dictionary 按 key 匹配而不依赖排列顺序。
浮点值沿用语言的数值相等规则，包括 NaN 不等于自身。

ExecutionSession 在建立调用时确定 Schema runtime，并在归池前清除该引用。codec 读写从 session
取得 runtime；VM 和 codec 的标量读写直接访问 RegisterStorage，不经 session 逐项转发。

## 3. VM Data Arena

每个发布快照拥有不可变 Arena：

```text
SnapshotArena
├── integer columns
├── float columns
├── reference columns
├── ValueEntries
│   ├── concrete TypeId
│   ├── row / layout
│   └── FunctionSetIndex
├── list / dictionary stores
└── FunctionSets
```

记录和 object 边使用 `ValueId`，不保存 CLR 对象引用。字符串等必要 CLR 值位于 reference column。
`ValueId` 同时携带快照代际和 `ValueIndex`；调用边界验证代际，VM 内部在 verifier 保证的路径上只传递
索引。

外部参数和 Host 返回值写入调用期 Arena。调用期 Arena 使用相同 layout，但生命周期受一次顶层执行及其
重入链约束。需要返回应用的临时值复制到当前代际的有界逸出存储，其值图可达的集合 Arena 同步冻结并
由当前快照持有。

## 4. 编译和链接

编译前端产生 typed function IR，并保留跨 Module 的符号调用点：

```text
typed AST
  -> control-flow IR
  -> capture and call analysis
  -> optimization
  -> virtual register allocation
  -> fixed instruction lowering
  -> global call-site linking
  -> program verification
```

ModuleUnit 缓存 typed IR 或未链接 program template。全局链接将直接调用解析为 `ProgramIndex`，Host 调用
解析为 `NativeCallIndex`，实例函数字段调用解析为 receiver 的 `FunctionSetIndex + SlotIndex`。调用方不
持有可变 function entry，也不内联其他 Module 的实现。

一次候选编译只创建一个全局链接上下文，顶层函数和嵌套 closure 共享该上下文。链接生成新的可执行
program，不修改 ModuleUnit 中的符号模板，也不修改旧快照中的 program。

允许的优化包括常量折叠、死代码删除、分支简化、无效 move 删除、寄存器复用和尾调用识别。所有优化
必须保留 checked 算术、求值顺序、fault span 和调用栈语义。

## 5. Program 与 descriptor

指令采用紧凑固定宽度表示，目标布局为 16 字节：

```text
Instruction
├── ushort Opcode
├── ushort Flags
├── int A
├── int B
└── int C
```

源码位置保存在平行压缩 source-map 中。program 分开保存同构数据：

```text
Program
├── Instructions[]
├── integer / float / reference constants
├── CallSites[]
├── NativeCallSites[]
├── ShapeTransfers[]
├── CollectionSites[]
├── ClosureTemplates[]
├── source map
└── register counts / parameter and result layouts
```

不得使用异构 `object[] Operations` 或每条指令一个 delegate。descriptor 数组只保存不可变的静态操作
信息，不保存执行状态。

## 6. 寄存器与指令

执行上下文使用三个连续寄存器区：

```text
ExecutionContext
├── long[] IntegerRegisters
├── double[] FloatRegisters
├── object?[] ReferenceRegisters
├── Frame[] Frames
├── invocation Arena
└── shared Budget
```

主要 Arena 指令包括：

- `LoadArenaInt`、`LoadArenaFloat`、`LoadArenaRef`。
- `LoadValueId`、`LoadCollectionId`、`LoadFunctionSet`。
- 同类 move 和静态 `ShapeTransfer`。
- Option / Result tag 与 payload 操作。
- checked 数值、字符串、位、比较和显式转换。
- branch、direct/indirect call、tail call、return。
- closure construction、collection operation 和 native call。

field load 根据预生成 layout 直接计算 column 和 offset。数值字段不经过 reader delegate、`object` 或
运行时类型转换。

## 7. 调用 ABI

每个参数和结果由 `LayoutId` 与三个 lane base 描述。`CallSite` 保存调用者位置到被调用者参数窗口的
静态传输计划；调用时不创建 `object[]`、参数 descriptor 或临时装箱值。

直接调用使用链接后的 `ProgramIndex`。实例函数字段调用先从 receiver 的 `ValueEntry` 取得函数集，再按
固定 slot 取得 `ProgramIndex` 或 `Missing`。外部构造值导入时已经使用 Schema 默认实现填充函数集，
所以与快照值共用同一调用路径。

间接函数值由两个 integer lane 表示：

```text
Callable
├── CoflowFunctionId
│   ├── SnapshotId
│   ├── kind: program / native / closure / missing
│   └── target index
└── CoflowValueId: receiver / closure environment
```

closure program 作为快照目标只链接一次，环境按 capture layout 存在调用期或逸出存储中。closure
返回应用时只提升可达环境、捕获值和集合。closure 的静态签名在编译和 verifier 阶段确定。

## 8. Frame、尾调用与重入

Frame 只保存恢复现场：调用者 program、返回 pc、三个寄存器 base 和结果目标。非尾调用保留调用者窗口、
复制参数并切换 program；返回时复制静态结果 layout、清空释放的 reference lane 并恢复现场。

尾调用在完成可能重叠的参数传输后复用当前 frame。递归不使用 CLR 调用栈表达 Coflow frame。

Host callback 再次调用同一个 `Coflow` 时建立嵌套执行上下文，但沿用顶层调用的预算对象和当前快照。
嵌套期间禁止绑定、Module 修改和编译。池化上下文只能在线程局部复用，归还前清除全部引用。

## 9. C# / Native 边界

每个生成类型只有一个强类型 codec，负责整值导入和实体化。有效快照 ID 可直接映射 Arena；无效 ID 的
外部 class/struct 被验证并写入调用期 Arena，同时创建由 Schema 默认函数填充的 FunctionSet。没有默认
实现的函数槽保持 `Missing`，只在调用时 fault。

每个 Host 函数生成精确签名的 native adapter。primitive 直接传递；struct 按 codec 重建；快照值复用
API 对象；临时值实体化；集合复制为只读副本；函数值保持为两个 ID 的 `CoflowFunction<...>`。Host 返回按相反方向
验证、冻结和导入。热路径不使用反射、`DynamicInvoke` 或 `object[]`。

## 10. Program verifier

候选快照发布前，verifier 必须拒绝：

- 未知 opcode、非法 flags 或越界寄存器、常量、descriptor 和 jump target。
- operand 的 lane 类别、语义 layout 或赋值状态不匹配。
- 控制流合并点的寄存器活性和 layout 不一致。
- 参数、结果、callable、closure capture 或 shape transfer 不兼容。
- 将普通整数当作 ValueId、CollectionId 或 FunctionSetIndex 使用。
- direct / native target 的签名与 CallSite 不一致。
- 非法 Arena offset、字段 layout 或可变 descriptor 引用。
- 可到达路径缺少返回或跨过程序边界。

验证后的执行循环不重复静态检查，但仍检查 null/default、stale ID、missing function、Host 返回值和算术
fault。

## 11. Fault

执行 fault 至少区分：

- stale / foreign `ValueId`。
- missing ordinary function 或未绑定 Host。
- null、无效 `default(TStruct)` 和 Host 返回类型错误。
- checked overflow、除零、非法转换和集合边界。
- 执行预算耗尽。
- Host 未处理异常。

fault 记录函数身份、源码路径、表达式 span 和精简调用栈。内部 opcode、寄存器内容、Arena offset 和
descriptor 不进入公共异常契约。

## 12. 执行预算

限制由 `CoflowOptions` 按实例配置。每个顶层调用创建预算链，所有直接调用、间接调用、closure、
Host 调用和同步重入共同计费：

- instruction count。
- frame depth。
- integer / float / reference register high-water mark。
- invocation value count。
- collection element count 和构造工作量。
- closure environment lane count。
- Host 调用次数和边界 lane 复制量。
- escaped value count / lane count。

检查点必须由编译器和 VM 固定，不能由源码写法绕过。预算耗尽产生可定位 fault，清理当前执行上下文，
不影响已发布快照。

## 13. 性能约束

稳态执行不得按指令、字段访问或普通调用分配对象。VM dispatch 直接处理算术和分支；调用使用整数索引
和连续数组。候选发布后不做反射发现、字典式字段查找或 WorkingState 查询。

基准至少分开测量：primitive 循环、Arena 字段读取、直接/间接/Host 调用、递归/尾递归、small/large
struct、Option/Result、collection、外部值导入和 VM -> Host -> VM 重入。只有基准证明解释器 dispatch
是主要瓶颈时才引入 IL/JIT backend；program 和 layout 允许未来增加 backend，但当前不实现第二执行器。

## 14. 验证要求

测试必须覆盖指令编码、全部 verifier 拒绝路径、三类寄存器窗口、直接和间接调用、closure、尾调用、
跨 Module 重链接、Arena/struct/collection layout、外部值函数填充、Host adapter、重入共享预算、
fault source map、上下文清理和所有预算边界。性能测试必须验证关键路径的分配次数，而不只比较总耗时。
