# C# Module 与 VM 核心设计

## 1. 目标

本设计统一 C# 生成代码、Module 数据和 VM 的类型语义，并保持运行时结构简单：

- 生成的普通 C# 对象是配置数据的唯一长期表示。
- Module 可以独立加载、校验、替换和卸载。
- ModuleSet 只组合 Module，不复制或重新编译其中的数据。
- VM 使用静态类型的虚拟寄存器，不使用 `object` 保存数值。
- 公开 API 使用生成类型、泛型 Table 和精确 Delegate，不暴露 VM 存储细节。
- 优化发生在编译器 IR 和通用执行结构中，不增加面向特定代码模式的指令。

本设计暂不包含：

- 二进制模块格式和 format version。
- 数据段、offset、segment、relocation、import 或 export table。
- 跨 Module 的记录、常量或函数引用。
- JIT、IL 生成、`unsafe` 和依赖内存布局的优化。
- Host 绑定的线程安全、只绑定一次或 generation gate。

## 2. 总体结构

运行时提供两个相近的只读查询入口：

```text
CoflowModule
├── Dictionary<Type, CoflowTable> Tables
├── Dictionary<Type, object> Singletons
└── CoflowFunctionEntry[] Functions

CoflowModuleSet
├── CoflowModule[] Modules
├── Dictionary<Type, CoflowTable> CombinedTables
├── Dictionary<Type, object> Singletons
└── CoflowFunctionEntry[] Functions
```

`CoflowModule` 表示一个自包含的编译模块。`CoflowModuleSet` 表示若干 Module 的组合视图。
两者尽量提供相同的 `Table`、`Singleton` 和函数查询 API。

只有实际存在的数据类型、单例和函数才进入这些集合。查询不存在的表时返回共享的强类型空表，
不在 Module 中预先创建所有可能的表；查询不存在的单例时返回 `Option<T>.None`。

## 3. Module 边界

每个 Module 必须独立完成解析、构造和校验：

- 记录引用只能指向同一个 Module 内的记录。
- 函数引用和闭包捕获只能依赖同一个 Module 内的程序、常量和对象。
- Module 不声明对其它 Module 的 import，也不在组合时重定位引用。
- Module 加载失败时不发布部分对象。

ModuleSet 组合时只检查全局冲突：

- 同一记录类型下不能出现重复记录键。
- 同一生成类型不能出现多个 singleton。
- 同一函数身份不能出现多个函数入口。
- 所有 Module 必须使用同一份生成契约程序集，避免名称相同但 CLR `Type` 不同。

组合不会修改已有 Module。增加、移除和替换 Module 时，ModuleSet 先在临时索引中完成全部
校验，成功后再发布新的组合视图。

替换 Module 不会使旧对象失效。此前取得的记录、表或函数入口继续属于旧快照；新的查询使用
替换后的 ModuleSet。独立 Module 的重新加载也不会隐式修改已经存在的 ModuleSet。

## 4. 数据表示

配置数据直接保存在生成的普通 C# 类型中：

```csharp
public sealed class Item
{
    public ItemId Id { get; internal set; }
    public long Price { get; internal set; }
    public Category Category { get; internal set; }
    public Item? Upgrade { get; internal set; }
    public IReadOnlyList<Item> Materials { get; internal set; } = Array.Empty<Item>();
}
```

表示规则如下：

- scalar 使用普通 C# 字段或属性。
- enum 使用生成的 enum。
- 记录引用直接保存目标生成对象。
- list 和 dictionary 使用正常的只读泛型集合。
- 嵌套对象使用生成的 C# 类型，并保持普通递归泛型关系。
- 对外可见的对象在加载完成后只读；加载期 setter 不作为公开修改 API。

不再保留第二份 record slot、heap、record id layout 或 encode/decode 数据。加载过程中允许存在
短生命周期的解析节点、键索引和未解析引用，但发布 Module 前必须全部解析或报告错误。

## 5. Table

Table 使用非泛型基类作为 Module 的统一内部容器：

```csharp
public abstract class CoflowTable
{
    public abstract Type RecordType { get; }
    public abstract int Count { get; }
}
```

字符串键和 enum 键由两个不同的强类型类表示：

```csharp
public sealed class CoflowStringTable<T> : CoflowTable
    where T : class
{
    private readonly T[] _records;
    private readonly Dictionary<string, T> _index;
}

public sealed class CoflowEnumTable<T, TKey> : CoflowTable
    where T : class
    where TKey : struct, Enum
{
    private readonly T[] _records;
    private readonly Dictionary<TKey, T> _index;
}
```

Table 自己的索引已经包含全部键，因此不再保存单独的 `Keys`。ModuleSet 的组合表引用各个
Module 的源表并建立组合索引；记录对象和源记录数组不复制。组合索引中的值仍然是原始生成对象。

C# 没有关联类型，无法让 `Table<T>()` 根据 `T` 自动推导键类型。生成器因此为每种记录生成
强类型 table token：

```csharp
module.Table(Item.Table); // CoflowEnumTable<Item, ItemId>
module.Table(Rule.Table); // CoflowStringTable<Rule>
```

token 同时携带记录类型、键类型和空表实例，使 Module 与 ModuleSet 使用同一个查询入口，且不需要
返回 `object` 后再由调用者转换。

## 6. Singleton 与 Host

Singleton 按生成类型索引：

```csharp
public Option<T> Singleton<T>() where T : class;
```

没有某个 singleton 是合法状态。ModuleSet 只在两个 Module 同时提供同一 singleton 类型时拒绝
组合，不要求每个 Module 都包含全部 singleton。

Host 是一个可选 singleton，遵循相同的缺失和重复规则。Host 配置可以重复修改：

```csharp
host.Configure(environment, log);
host.Configure(newEnvironment, newLog);
```

运行时不提供 bind-once、`Volatile`、锁、generation gate 或自动状态迁移。调用方如果跨线程修改
Host，需要自行协调。Host 函数实现也可以重新配置，不因一次绑定永久锁定。

## 7. 加载过程

不为每条数据生成完整 `ModuleFactory`。生成器只按 schema 生成类型和类型描述；生成代码量与
类型、字段数量相关，不与记录数量线性增长。

通用加载器采用两阶段构建：

1. 解析输入，检查结构、字段和值类型，并收集记录键。
2. 为所有记录创建生成对象，建立 `(记录类型, 键) -> 对象` 索引。
3. 写入 scalar、enum、嵌套对象和集合字段。
4. 将记录引用连接到第一阶段已经创建的目标对象。
5. 构建强类型 Table、singleton 和函数入口。
6. 完成全部校验后一次性发布 CoflowModule。

先创建对象再连接引用，可以处理前向引用以及合法的循环引用。跨 Module 引用、缺失目标或类型不兼容
在当前 Module 加载时直接失败。

类型描述可以包含生成的构造和字段写入委托。初始化描述时允许少量反射或加载期转换，但必须缓存，
并且不能进入已发布 Module 的数据表示或 VM 热路径。优先生成按类型复用的静态描述，不生成逐记录
赋值源码，也不使用 `unsafe` 写入对象字段。

## 8. 函数入口

`CoflowFunctionSlot` 更名为 `CoflowFunctionEntry`。公开函数入口保留精确 Delegate 类型：

```csharp
public sealed class CoflowFunctionEntry<TDelegate>
    where TDelegate : Delegate
{
    public TDelegate Function { get; }
}
```

例如：

```csharp
CoflowFunctionEntry<Func<Item, long>> Price;
CoflowFunctionEntry<Action<string>> Log;
```

公开调用不使用 `params object?[]`。生成的泛型适配器负责把 Delegate 参数写入 VM 对应的寄存器
类别，并从返回寄存器读取精确类型。Host Delegate 也通过同样的静态签名适配，不在每次调用时使用
反射或创建参数数组。

编译函数入口不可变；可配置的 Host 实现是入口持有的当前 Delegate。更新 Host 实现只做普通赋值，
不引入额外并发协议。

## 9. VM 类型与寄存器

生成类型是唯一语义类型系统。VM 不定义可继承的 `CoflowValue`，也不维护运行时 `ValueKind`。
编译器为每个虚拟寄存器静态确定语义类型和存储类别。

VM 使用三个寄存器区：

```csharp
long[] integerRegisters;
double[] floatRegisters;
object?[] referenceRegisters;
```

类型映射为：

| 生成/CFT 类型 | VM 存储 |
| --- | --- |
| integer | integer register |
| boolean | integer register，规范值为 0 或 1 |
| enum | integer register，边界按生成 enum 转换 |
| float | float register |
| string | reference register 中的 `string` |
| 生成记录或嵌套对象 | reference register 中的实际生成对象 |
| list / dictionary | reference register 中的实际泛型集合 |
| Delegate / closure | reference register 中的实际引用对象 |

`object?[]` 在这里仅作为 CLR 引用存储，不保存 primitive，因此不会产生 primitive 装箱。生成的
复合对象使用引用类型；需要传播的 option/result 等静态复合值由编译器拆成 tag 寄存器和对应类别的
payload 寄存器，在公开边界重新构造精确泛型类型。

每个 Program 静态保存三类寄存器数量：

```text
CoflowProgram
├── Instructions
├── IntegerConstants: long[]
├── FloatConstants: double[]
├── ReferenceConstants: object?[]
├── IntegerRegisterCount
├── FloatRegisterCount
├── ReferenceRegisterCount
├── ParameterLayout
└── ReturnLayout
```

三个 Count 由编译器根据最大寄存器索引生成，执行时不扫描指令计算。它们用于给每次调用分配连续
寄存器窗口，特别是保证递归调用不会覆盖调用者寄存器。

## 10. 指令集

指令表达静态类型语义并直接指定源、目标寄存器：

```text
LoadIntegerConstant destination, constant
LoadReferenceConstant destination, constant
MoveInteger destination, source
LoadField destination, instance, accessor
AddInt destination, left, right
LessFloat destination, left, right
JumpIfFalse condition, target
Call destination, callSite
TailCall callSite
Return source
```

`AddInt`、`LessFloat` 等由语言静态类型决定的基础指令是合理的。以下针对局部指令序列的模式特化
不进入最终指令集：

```text
LocalInt
StoreLocalInt
JumpIfLocalIntNotLessArgument
AddLocalIntsAndStore
AddLocalIntConstantAndStore
```

算术、比较和转换直接在 opcode 分支中完成，不通过 `Func<long, long, long>` 一类委托再次分派。
整数溢出、除零、无效转换和浮点行为必须保留语言定义的语义。

字段访问器按结果存储类别提供少量固定签名：

```csharp
delegate long ReadIntegerField(object instance);
delegate double ReadFloatField(object instance);
delegate object? ReadReferenceField(object instance);
```

生成访问器只进行一次记录引用转换，然后直接读取强类型字段。数值字段不会经过 `object`。

## 11. 调用帧与执行上下文

寄存器 VM 仍然需要保存函数返回现场，但 Frame 不保存参数或局部变量数组：

```csharp
internal struct CoflowFrame
{
    internal CoflowProgram Caller;
    internal int ReturnPc;
    internal int IntegerBase;
    internal int FloatBase;
    internal int ReferenceBase;
    internal CoflowRegister ReturnTarget;
}
```

执行上下文使用池化连续数组：

```text
CoflowExecutionContext
├── IntegerRegisters
├── FloatRegisters
├── ReferenceRegisters
├── Frames
└── FrameCount
```

普通调用把调用者现场写入 `Frame[]`，然后按被调用 Program 的三个 RegisterCount 扩展寄存器窗口。
返回时恢复 base 和 pc，并把结果写入调用者目标寄存器。尾调用直接替换当前 Program 和窗口，不增加
Frame。Frame 使用 struct 和数组，不使用每次调用分配的 `Stack<Frame>` 或 locals 数组。

CallSite 静态保存参数寄存器到被调用函数 `ParameterLayout` 的映射。调用时直接复制对应类别寄存器，
不创建 `object?[]`。间接调用也先验证 Delegate/函数签名，再使用相同布局。

## 12. 闭包和高阶函数

闭包按存储类别保存捕获值：

```csharp
internal sealed class CoflowClosure
{
    internal long[] IntegerCaptures;
    internal double[] FloatCaptures;
    internal object?[] ReferenceCaptures;
}
```

调用闭包时把捕获值复制到新寄存器窗口的静态位置，不拼接参数数组。高阶函数通过静态 collection
adapter 读取实际泛型集合，并通过正常 VM 调用帧调用 callback；迭代过程不为每个元素创建
`object?[]`。

闭包对象和最终结果集合属于必要分配。临时调用参数、单步传播结果和每次迭代 callback 参数不应
产生托管分配。

## 13. 编译与优化

编译器先产生静态类型 IR，再降低到寄存器指令。IR 负责：

- 控制流和分支目标。
- 每个值的生成类型及三类存储映射。
- 虚拟寄存器分配。
- 参数、返回值和闭包 capture layout。
- source span 到最终指令的映射。

通用优化在 IR 层完成：

- 常量折叠。
- 死代码删除。
- 无效 move 删除。
- 分支简化。
- 公共子表达式消除（确认有收益后）。
- 尾调用识别。

不通过增加面向某个循环或固定四指令序列的 opcode 来优化。指令压缩、direct threading、JIT 和
`unsafe` 只有在基准测试证明 switch dispatch 或指令带宽成为主要瓶颈后才考虑。

## 14. Program 验证

Program 在发布前一次性验证：

- opcode 合法且 operand 完整。
- 三类寄存器索引不越界。
- 指令读取和写入的寄存器类别正确。
- 常量索引与常量类别匹配。
- 跳转目标是合法指令边界。
- 所有控制流路径的返回类型一致。
- CallSite 参数、返回值和函数签名一致。
- 闭包 capture layout 与目标 Program 一致。

验证通过后，执行循环不再进行重复的动态类型判断。运行时仍保留：

- 指令预算。
- 最大调用深度和寄存器数量限制。
- 整数溢出与除零检查。
- source map、当前函数和调用栈诊断。
- 池化引用数组归还前的必要清理。

## 15. 测试与基准

正确性测试至少覆盖：

- 所有 scalar、enum、字符串和 null/option 边界。
- 嵌套对象、list、dictionary 和多层组合。
- 前向引用、合法循环引用、缺失引用、错误引用类型和跨 Module 引用。
- 字符串键和 enum 键 Table，以及缺失 Table 的共享空结果。
- ModuleSet 增加、移除、替换和冲突失败不改变旧视图。
- singleton 缺失、重复 singleton、Host 缺失和重复配置。
- 算术溢出、除零、比较、短路、分支、循环和返回。
- 普通调用、递归、尾调用、间接调用、闭包和高阶函数。
- Host Delegate 参数与返回值、异常包装和重新配置。
- 指令、调用深度、寄存器数量和集合工作量限制。
- source span 和跨调用错误栈。

基准至少包含：

- 整数与浮点紧密循环。
- 多层函数调用与尾递归。
- 记录字段读取。
- list map/filter/fold。
- 闭包捕获和调用。
- VM 与 Host Delegate 往返。
- Module 首次加载和 ModuleSet 替换。

性能验收优先观察每次调用和每次循环的托管分配。热路径应消除 primitive 装箱、临时参数数组和
Frame/locals 分配；加载期的少量缓存初始化和类型转换不作为零分配目标。

## 16. 实施顺序

1. 引入新的 Table、singleton 和 `CoflowFunctionEntry<TDelegate>` API，并保持生成对象为唯一数据。
2. 实现 CoflowModuleSet 的组合索引、冲突校验和显式替换语义。
3. 删除 slot/heap/generation storage 和已发布对象的 encode/decode 路径。
4. 编译器引入静态类型 IR、三类常量区和虚拟寄存器布局。
5. VM 改为三类寄存器窗口、struct Frame 和无数组分配的调用。
6. 改造字段访问、Host Delegate、闭包和高阶函数的强类型边界。
7. 删除旧 `object?[]` 栈、模式特化 opcode、bind-once Host 和相关并发状态。
8. 完成正确性测试、分配基准和 CI，再根据测量结果决定后续低层优化。

每个阶段都应保持加载失败不发布半成品、ModuleSet 组合失败不修改旧视图，并保持诊断和执行限制。
