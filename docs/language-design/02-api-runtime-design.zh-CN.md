# C# API 与 Runtime 设计

> 状态：内部设计契约
>
> 范围：生成代码边界、`Coflow` 生命周期、Module、编译发布、强类型数据、Host 和 C# / VM 边界。

基础语言语义见 `01-language-design.zh-CN.md`，VM 的物理执行结构见
`03-vm-design.zh-CN.md`。对外用法只记录在
`website/docs/docs/reference/07-codegen/01-csharp.md`。

## 1. 总体模型

`Coflow.Runtime` 提供稳定的顶层 API；`Coflow`、Module 句柄、编译结果、表、诊断和执行 fault
均由 Runtime 定义，不由业务代码生成。生成代码只包含特定 Schema 的强类型声明、Schema 描述和
Runtime codec。

典型流程为：

```csharp
var coflow = Schema.Create();

var core = coflow.LoadModule(coreSources);
var gameplay = coflow.LoadModule(gameplaySources);

coflow.Bind(new LoggingHost(environment, log));
var result = coflow.Compile();

coflow.ReplaceModule(gameplay, replacementSources);
var next = coflow.Compile();

coflow.RemoveModule(core);
```

一个 `Coflow` 表示一个可修改、可重复编译的运行环境。加载、Host 绑定、编译、查询和执行都通过该实例
完成。同一个 `Coflow` 的查询、修改、编译和执行由调用方串行化。

## 2. Schema 与生成边界

生成类型直接位于 C# global namespace。仅供生成代码实现 Runtime 协议的类型位于
`Coflow.Runtime.CompilerServices`，并通过编辑器隐藏属性避免成为应用调用面。

每个 Schema 生成一份静态描述：

```text
Schema
├── type / field / function IDs
├── 类型关系与默认值
├── 物理布局
│   ├── integer / float / reference lane 数量
│   ├── 字段 offset
│   ├── collection layout
│   └── flattened ABI
├── function 参数与结果 layout IDs
├── 每个生成类型一个 TypeCodec<T>
└── Host binding codec
```

Schema 注册声明和签名中的 closed 类型布局；Module 编译器在发布候选前补全函数体引入的组合布局。
Runtime 不在执行阶段推导布局，也不按字段生成 reader。`TypeCodec<T>` 直接访问强类型
字段，在 C# 值和物理布局之间整体读写；primitive 不经过 `object` 或装箱。Schema 描述本身不包含
CFD 数据，也不创建 `Coflow`。

实例通过 `Schema.Create(CoflowOptions?)` 创建。执行限制属于 `Coflow` 实例配置；同一实例的每次
顶层调用创建独立预算，同步 VM -> Host -> VM 重入继续使用该顶层调用的预算。

## 3. Coflow 状态与发布

`Coflow` 同时维护工作状态和最近一次成功发布的快照：

```text
Coflow
├── WorkingState
│   ├── Module definitions
│   ├── Host bindings
│   └── revision / dirty state
└── PublishedSnapshot?
    ├── API data graph
    ├── VM data arena
    ├── tables / singletons
    ├── linked functions
    └── source maps
```

`LoadModule`、`ReplaceModule`、`RemoveModule` 和 `Bind` 只修改 `WorkingState`。这些修改必须等下一次
`Compile` 成功后才生效。存在未编译修改时，查询和执行仍使用上一次成功的 `PublishedSnapshot`；首次
成功编译前查询或执行会明确报告尚无可用快照。

`Compile()` 构造完整候选快照并返回 `CoflowCompileResult`。预期的源码、类型、链接和数据诊断通过
结果返回，不用异常表达。失败保留工作状态和旧快照；成功以一次引用替换发布候选快照。

执行期间禁止修改工作状态或发起编译。同线程 VM -> Host -> VM 重入允许，并共享同一执行预算链。

## 4. Module

Module 是源码加载、替换、移除和增量编译的管理单位。`LoadModule` 接受一个或多个带逻辑路径的 CFD
源码并返回不透明句柄。Module 不创建命名空间；所有 Module 共享 Schema 的全局符号空间。

Module 之间允许普通数据和函数引用，也允许相互引用。数据记录先统一创建并注册对象外壳，再填充字段，
因此支持自引用、同 Module 引用环和跨 Module 引用环。加载顺序不影响解析结果。Module 身份不进入语言
名称，只用于后续替换和移除。

每个成功解析和检查的 Module 可形成可复用的 `ModuleUnit`：

```text
ModuleUnit
├── normalized declarations
├── unresolved symbolic references
├── typed function IR / program templates
└── source maps
```

替换只使受影响的 ModuleUnit 重新分析和编译；每次 `Compile` 都重新建立全局符号目录并完成全局链接。
跨 Module 调用只记录符号或稳定链接槽，调用方 program 不嵌入目标 Module 的可变函数对象，也不进行
会让调用方依赖被调用方实现的跨 Module 优化。因此只修改函数实现时不要求重新生成无关调用方 program。

ModuleUnit 同时记录类型检查期间使用的全局绑定依赖。复用前以当前候选符号目录重新验证这些依赖；
解析结果仍唯一且指向同一声明时复用模板，声明新增、移除或歧义变化时重新分析对应 ModuleUnit。
记录引用和延迟 Schema 常量在链接期解析为候选快照对象，未链接模板不保存 `CoflowValueId`、
`ProgramIndex` 或任何已发布快照对象。

移除 Module 是合法工作状态修改。下一次编译若仍有引用指向被移除的声明，则候选快照产生链接诊断，
旧快照保持可用。

## 5. 编译与全局链接

一次编译按以下顺序构造候选：

```text
WorkingState snapshot
  -> parse/check dirty Modules
  -> build global symbol catalog
  -> resolve cross-Module data and functions
  -> assign value IDs and Arena rows
  -> build API data graph
  -> build VM data arena
  -> link VM and Host call sites
  -> validate candidate
  -> publish Snapshot
```

`Compile` 总是完成数据实体化、所有函数编译和全局链接，不存在仅发布数据的中间状态。候选中的对象、
Arena、表、单例、program 和链接表在验证完成前均不可见。

函数调用点使用当前候选的整数 `ProgramIndex`、`FunctionSetIndex` 或 `NativeCallIndex`。重新链接不修改
已发布 program；新索引只属于新快照。

## 6. 双数据表示

每个快照保存两份独立且不可变的数据：

```text
API data graph                 VM data arena
├── generated classes         ├── primitive columns
├── generated readonly structs├── value IDs / record edges
├── direct CLR references     ├── flattened fixed values
└── read-only collections     ├── immutable struct lanes
                               ├── collection stores
                               └── function-set indexes
```

两份表示由同一个经过验证的规范化候选构建，并在发布前交叉验证。发布后两者均不可修改，因此不存在
视图同步。API 查询直接返回自然的 C# 对象和真正的 struct；VM 执行直接使用列式 Arena，不通过 API
对象读取字段。

生成 class 和 struct 都携带紧凑的内部值 ID。值 ID 由快照代际和 `ValueIndex` 组成，`ValueIndex`
定位具体类型、Arena 行和函数集。ID 不参与字段相等性或 hash。

旧快照取得的 API 值始终可以读取。新快照发布后，其旧 ID 不能用于当前 `Coflow` 执行；使用错误
`Coflow` 或过时代际调用函数会产生明确 fault。

## 7. Struct、集合与逸出值

CFT `@struct` 生成真正的 C# `readonly struct`，并支持普通字段、集合、引用和函数字段等全部语言能力。
生成类型不通过继承共享 Runtime 行为。struct 相等与 hash 只比较语义数据字段；边界必须显式检查 CLR
`default(T)` 是否满足必填字段。

生成 struct 在 VM 布局中始终按 Schema 布局递归展开，不因字段数量切换为间接对象表示。集合始终使用
独立物理存储，导入时复制并冻结，不把调用方可变集合直接放入快照或执行上下文。

VM 计算产生并返回给应用的新值需要获得当前代际 ID。Runtime 将其物理数据保存在当前快照关联的有界
逸出值存储中，同时实体化一份 API 值。新快照发布时整个逸出存储失效；API 值仍可读取，但不能继续调用
函数。逸出对象引用的调用期集合随可达值图冻结到同一快照，后续调用通过该快照解析集合句柄。逸出值、
closure 和冻结集合共同受独立数量与内存预算约束。

## 8. Host

Host 是由应用提供的特殊单例定义，生命周期和普通单例共同参与快照发布。允许同时存在多个不同 Host
类型，但每种 Host 类型在工作状态中最多有一个当前绑定。

生成 Host 是可直接构造的不可变对象：

```csharp
coflow.Bind(new LoggingHost(environment, log));
```

再次绑定同一 Host 类型表示替换。`Bind` 不生成按类型命名的方法，不存在 `Configure`、`HostState` 或
逐函数字段绑定入口。绑定只修改工作状态，下一次成功 `Compile` 后才生效；旧快照继续使用旧 Host。
Runtime 不修改应用传入的绑定对象；每个候选快照创建并持有独立的 Host 表示，编译失败时随候选整体
丢弃。因此同一绑定对象可以用于不同 `Coflow` 实例，各实例的 Host 身份彼此隔离。

未绑定 Host 不阻止编译。访问未绑定 Host 的数据或实际调用其函数时才产生明确 fault。已发布快照直接
保存强类型 native adapter 和 Host 常量，不在热路径查询工作状态。普通生成值不能注入外部 delegate；
外部函数只能通过 Host 进入 Runtime。

## 9. C# 与 VM 边界

### 9.1 应用传入值

生成实例函数显式接受 `Coflow`：

```csharp
var result = value.Calculate(coflow, argument);
```

边界处理分为：

- 当前快照的有效 ID：验证代际和类型后直接复用 Arena 位置。
- 应用主动构造、ID 无效的 class 或 struct：由 `TypeCodec<T>` 导入调用期 Arena，不修改原值。
- collection：复制、验证并冻结到调用期 collection arena。
- 普通外部 delegate：拒绝。

导入外部值时，codec 同时创建该类型的函数集。普通函数字段使用 Schema 中的默认函数实现并填入当前
快照的 `ProgramIndex`；没有默认实现的槽写入 `Missing`，仅在实际调用时 fault。CFD 某条记录提供的
函数覆盖只属于具有该记录有效 ID 的 Runtime 值，不能由无记录身份的外部值推断。

函数默认实现以导入后的调用期 `ValueId` 为接收者，因此可以读取传入 class/struct 的普通字段，也可以
调用全局链接后的其他 Module 函数。

### 9.2 VM 调用 Host

primitive 直接传递；struct 由 codec 重建；快照记录复用 API 图中的现有对象；临时值实体化为新的 API
值；集合生成只读 C# 副本；函数值保持为精确签名的 `CoflowFunction<...>`。每个 Host 函数生成精确 adapter，
调用过程不使用 `object[]`。

Host 返回值先验证并冻结，再写入调用期 Arena。Host 返回的普通外部值与应用参数使用相同的导入规则，
包括函数默认实现填充、必填 null / default 检查、集合复制和预算计费。只有 Runtime 为 VM callable
创建的 `CoflowFunction<...>` 可以作为函数值返回；外部 delegate 只作为 Host 绑定实现存在，不能进入 VM 数据。

所有一等函数类型统一生成为 `CoflowFunction<T1, ..., TResult>`，`unit` 结果也显式使用 `Unit`。句柄内部只
保存 `CoflowFunctionId` 和环境 `CoflowValueId`，不保存 `Coflow`、delegate 或任意对象。应用和 Host
调用句柄时必须显式传入 `Coflow`：

```csharp
var scaler = scenario.MakeScaler(coflow, 3);
var value = scaler.Invoke(coflow, 4);
```

`CoflowFunctionId` 携带全局唯一的发布快照身份，并编码 `program`、`native`、`closure` 或 `missing`
以及目标索引。环境 ID 表示 receiver 或 closure 捕获环境。默认句柄允许作为值存在，只在实际调用时报告
缺失；旧快照句柄或传入错误 `Coflow` 时报告 stale。

## 10. 查询、诊断与生命周期

表和单例从当前发布快照查询。表 key 类型由 Schema 固定，不做字符串与 enum 的隐式转换；集合和表均
保持确定顺序。缺少普通单例或表项使用对应的显式可选结果；未绑定 Host 使用专门状态，不能伪装成普通
缺失数据。

编译结果包含是否发布、诊断和成功快照的代际信息。执行 fault 包含函数身份、来源路径、source span、
精简 Coflow 调用栈和底层异常，但不公开寄存器、Arena offset 或 descriptor。

`Coflow` 不建立按源码内容增长的全局缓存。工作状态、快照、ModuleUnit、执行上下文和逸出值存储均由
所属 `Coflow` 管理；池化执行上下文归还前必须清空引用槽。

## 11. 固定不支持

- 并发修改、编译、查询或执行同一个 `Coflow`。
- 运行时读取 CFT，或在快照执行阶段生成物理布局、按字符串发现字段和函数。
- 普通生成值携带或接受应用 delegate。
- 修改已发布 API 对象、Arena、集合或 program。
- 暂停、异步恢复或持久化 VM 执行状态。
- 跨 Module 内联等会把调用方 program 绑定到被调用方实现的优化。

## 12. 验证要求

测试必须覆盖：

- Module 任意加载顺序、跨 Module 和循环引用、替换、移除与失败原子性。
- 首次编译、脏工作状态、失败后旧快照继续执行及成功后的代际失效。
- class 与真实 readonly struct 的双表示、相等性、`default(T)` 和递归展开布局。
- API 图与 VM Arena 对同一规范化候选的一致性。
- 外部 class/struct 导入、默认函数字段填充、缺失函数延迟 fault 和外部 delegate 拒绝。
- 多 Host 类型、同类型换绑、未绑定延迟 fault 和 Host -> VM 重入。
- direct、indirect、closure、Host 和跨 Module 调用的全局重链接。
- instruction、frame、register、collection、导入和逸出值预算。
- stale ID、错误 `Coflow`、Host 异常和 source-mapped fault。
