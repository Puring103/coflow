# C# API 与 Runtime 设计

> 状态：当前内部设计契约
>
> 范围：生成 ABI、加载、强类型数据、Module、ModuleSet、Host、函数入口、诊断与发布边界。

当前实现已经覆盖本文描述的 generated contract、`Load` / `LoadAndCompile`、不可变 Module/ModuleSet、
强类型调用面和 Host 配置边界。VM 执行限制的未完成状态单独记录在 `03-vm-design.zh-CN.md`，不由 API
层隐式补偿。

本文档定义 C# Runtime 的应用边界和模块实现。基础语言语义见
`01-language-design.zh-CN.md`；VM 执行结构见 `03-vm-design.zh-CN.md`。对外用法保留在
`website/docs/docs/reference/07-codegen/01-csharp.md`，公开 API 的机器基线由
`runtimes/csharp/Coflow.Cfd.Runtime/PublicAPI.Shipped.txt` 固定。

## 1. 职责边界

`Coflow.Cfd.Runtime` 根据构建期生成的 contract 解析 CFD、构造强类型对象，并可选地检查和编译函数。
Runtime 不读取 CFT，不扫描目录，不管理文件来源，也不加载外部 schema 或 bytecode artifact。

逻辑边界为：

```text
Application
└── generated CoflowData entry
    └── CoflowRuntime application API
        ├── generated contract ABI
        ├── CFD parser and loader
        ├── function compiler
        ├── immutable module model
        └── VM
```

公开命名空间分为：

- `CoflowRuntime`：应用 API，包括 Module、ModuleSet、Table、Option、Result、诊断和 fault。
- `CoflowRuntime.Generated`：生成代码 ABI，包括 metadata、reader、field binding、table token、function
  entry 和 delegate adapter。该命名空间跨程序集公开，但不属于应用 API。

应用不传入 schema 对象，不注册动态 provider，也不按字符串查询 type、field 或 function。

## 2. Generated contract

每个 C# codegen 目标生成一个静态 contract。它是 Runtime 的唯一静态类型来源：

```text
GeneratedContract
├── contract identity
├── type metadata[]
│   ├── canonical type identity
│   ├── runtime type
│   ├── inheritance
│   ├── fields
│   ├── defaults
│   └── annotations
├── field bindings[]
├── table tokens[]
├── function signatures[]
└── delegate adapters[]
```

contract 只描述 CFT 生成结果，不携带 CFD 数据。Runtime 不重新解析 CFT，不计算 schema hash，也不在
同一应用入口内替换 contract。

生成 ABI 可以使用加载期反射和泛型实例化建立缓存，但发布后的字段读取、函数调用和集合访问不能每次
重新发现 metadata。ABI 变更必须同步更新 API 基线并按兼容性影响选择版本号。

## 3. 加载入口与阶段

生成入口只接受一个 CFD 字符串或字符串集合。字符串内容是 CFD，不是路径；调用方负责文件系统、资源
包、网络或其他来源。

两个加载模式为：

- `Load`：解析并构造全部普通数据，识别函数 body 的结构边界，但跳过函数表达式检查和编译。
- `LoadAndCompile`：完成数据加载后，继续执行函数链接、类型检查、capture 分析和 VM program lowering。

完整管线为：

```text
CFD text[] + GeneratedContract
  -> source catalog
  -> lexical and structural parse
  -> typed record catalog
  -> allocate generated objects
  -> populate scalar and inline fields
  -> resolve record references
  -> build tables and singletons
  -> build function entries
  -> optional function compile
  -> validate candidate module
  -> publish CoflowModule
```

解析、数据转换、引用解析或函数编译失败时不返回部分 Module。能够独立发现的诊断应聚合后返回；每条
诊断包含稳定 code、来源路径和 source span。

数据模块中的普通字段可以读取。函数调用产生 `CoflowFunctionNotCompiledException`，Host function
配置也必须失败。已编译 Module 才允许调用函数和配置 Host Delegate。

## 4. 数据对象与加载

生成的普通 C# 对象是配置数据的唯一发布表示：

```text
GeneratedObject
├── scalar and enum fields
├── generated object fields
├── record-reference fields
├── IReadOnlyList<T> fields
├── IReadOnlyDictionary<K, V> fields
├── Option<T> fields
└── Result<T, E> fields
```

加载采用先分配、后连接的阶段：先创建全部记录对象和 key 索引，再填充普通字段和 inline object，最后
连接记录引用。这样允许前向引用；循环引用是否允许由当前数据模型规则决定，并在发布前统一验证。

发布后的对象不可变。加载期构造入口和 setter 属于生成 ABI，不是应用修改接口。Runtime 不同时保留
record slot、data heap、record-id layout 或另一份 encode/decode 数据。

## 5. Table 与 Singleton

Table token 同时确定记录类型、key 类型和共享空表：

```text
TableToken
├── record runtime type
├── key runtime type
├── table implementation kind
└── empty table instance

StringTable<T>
├── ordered records: T[]
└── key index: Dictionary<string, T>

EnumTable<T, TKey>
├── ordered records: T[]
└── key index: Dictionary<TKey, T>
```

字符串 key 和 enum key 不互相转换。不存在的 Table 返回 token 携带的共享空表；不存在的 key 返回
`Option<T>.None`。Table 的整数索引只表达稳定记录顺序，不是第二个 key 查询入口。

singleton 按生成 runtime type 索引。缺少 singleton 是合法状态并返回 Option；同一 ModuleSet 中出现
两个相同生成类型的 singleton 是组合冲突。

## 6. Module

每次成功加载发布一个不可变 Module：

```text
CoflowModule
├── generated contract identity
├── source catalog
├── tables by table token
├── singletons by runtime type
├── function entries by function identity
└── functions-compiled flag
```

Module 必须自包含：

- 同一次加载的多个 CFD source 可以互相引用。
- 记录、常量和函数引用不能跨 Module。
- 函数和 closure 只能持有本 Module 的 program、常量和对象。
- 普通数据、索引和 compiled program 发布后不可变。
- 加载失败不留下 Runtime 全局缓存或半发布对象。

Module 不提供原地 reload、Compile mutation、generation gate 或 `IDisposable` 生命周期。更新数据通过
重新加载新 Module 完成；旧 Module 和此前取得的对象由正常 CLR 引用保持有效。

## 7. ModuleSet

ModuleSet 是多个独立 Module 的不可变组合查询视图：

```text
CoflowModuleSet
├── ordered modules
├── combined tables by table token
├── singletons by runtime type
└── function entries by function identity
```

组合不复制记录对象或源记录数组，也不重新链接跨 Module 引用。候选组合必须检查：

- generated contract 兼容。
- 同一记录域中没有重复 key。
- 同一生成类型没有重复 singleton。
- 同一 function identity 没有重复入口。

Add、Remove 和 Replace 都先构造并验证候选索引，再返回新 ModuleSet。任何失败不能改变原视图。旧视图、
旧 Module 和旧记录继续有效，不建立 Module 到所有组合视图的反向订阅。

## 8. Host

`@Host @singleton` 由 Runtime 创建，CFD 不能声明对应记录或函数 body。生成 contract 提供完整、强类型
的配置入口。

```text
HostSingleton
├── generated data fields
└── host function entries
    ├── static signature
    └── current delegate?
```

配置可以重复执行，更新后已有生成对象读取新的字段和 Delegate。Runtime 不提供 bind-once、锁、
`Volatile`、generation gate 或自动状态迁移。应用必须保证配置不与读取和函数调用并发。

每个生成 Host 类型只提供一个完整 `Configure(...)` 入口，一次提交全部生成 data field 和 function
delegate；不存在逐函数 `BindX` API。Runtime 先更新函数 entry，再执行生成的字段赋值，并在入口成功
返回前将 Host 标记为已配置。

未编译 Module 不允许配置 Host function。已编译但尚未配置的 Host function 在调用时产生明确的未绑定
异常；Host Delegate 的未处理异常进入 VM fault 边界。

## 9. 函数入口与生成调用面

生成类型只向应用暴露按 CFT 签名生成的强类型方法。内部 function entry 保存签名与实现选择：

```text
FunctionEntry
├── identity
├── parameter types
├── result type
├── compiled program?
├── current host delegate?
└── compiled-state flag
```

普通 CFD function 的 compiled program 发布后不可替换。Host entry 的当前 Delegate 可以按 Host 配置
规则更新。直接、间接、closure 和 Host 调用共享静态函数签名；具体 VM ABI 见 VM 设计文档。

生成 adapter 必须覆盖 contract 中实际出现的精确 Delegate 类型，包括 unit 返回、Option/Result、集合、
记录、enum 和返回 closure。适配器缓存按 Delegate 类型复用，不能在每次调用时反射或构造参数数组。

## 10. 原子性与生命周期

候选 Module 的所有对象、索引、函数 entry 和 program 在发布前私有。发布只发生一次：

```text
CandidateModule
  -> parse complete
  -> data complete
  -> references complete
  -> functions complete when requested
  -> validation complete
  -> publish immutable Module
```

失败候选释放 Runtime 持有的根引用。Runtime 不建立按 CFD 内容或加载次数增长的全局 Module 缓存。
Module、记录、closure 和 Delegate 的寿命由 CLR 引用决定；context pool 在归还前必须清除其引用槽。

ModuleSet 替换同样遵循候选构造后发布。组合冲突不能使原 ModuleSet 进入部分更新状态。

## 11. 诊断与异常边界

加载阶段异常分为结构解析、contract 驱动的数据错误和函数编译错误；它们对应用表现为具体
`CfdLoadException` 子类并携带诊断集合。

函数执行边界区分：

- `Option` 和 `Result`：语言值，不是异常。
- function not compiled / Host not bound：明确的 API 状态异常。
- 算术、转换、VM 状态和 Host 未处理异常：`CoflowFaultException`。

fault 包含 function identity、source path、source span、精简 Coflow 调用栈和 inner exception。不得
公开 VM 寄存器或内部 descriptor。

## 12. 验证要求

Runtime 测试至少覆盖：

- scalar、enum、object、list、dictionary、Option 和 Result 加载。
- 前向引用、缺失引用、错误目标类型和 Module 边界。
- string/enum Table、空 Table、稳定顺序和 singleton 缺失。
- ModuleSet 增删替换、所有冲突类型和失败原子性。
- 数据模块的函数禁用状态和已编译 Module 的调用状态。
- Host 缺失、重复配置、签名边界、异常包装和配置可见性。
- 旧 Module、旧 ModuleSet、旧记录和外部 closure 的生命周期。
- 公开 API 基线与生成 ABI 表面。

## 13. 固定不支持的 Runtime 能力

当前 Runtime 不支持：

- 运行期 CFT parser、CFT check 或 schema hash。
- 动态 type/field/function 字符串 API。
- 跨 Module 引用、import、relocation 或组合时重新编译。
- 原地 reload、generation、自动 Host 状态迁移或 Module disposal。
- JSON、MessagePack、数据 artifact 或 bytecode artifact。
- 异步函数、Task、取消、scheduler 或后台执行。
