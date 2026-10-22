# C# 生成类型与加载器设计

## 模块与职责

`coflow-codegen-csharp` 依赖 `coflow-codegen` 契约和已编译 schema，生成业务类型、枚举、字段描述、默认值与常量节点、类型明确的构造回调。`Coflow.Loader` 负责 CFD 解析、结构和值校验、引用连接、维度选择、插值和诊断。生成代码使用泛型描述与直接属性赋值；CLR `Type` 仅作为表和单例的注册键。

项目目录发现、CFT 编译、业务 `check`、写回和发布归 `coflow-runtime` 及对应模块。C# 调用方提供 CFD 源集合，负责资源发现与生命周期。编辑器的可修复数据状态见 [容错加载设计](../editor-design/tolerant-loading.md)。

## 声明与生成类型

| CFT 声明或字段 | C# 表达 |
| --- | --- |
| `table`、`singleton` | `partial class`，保留继承、abstract、sealed |
| `data` | `partial class` |
| `@struct sealed data` | `partial struct` |
| `int`、`float`、`bool`、`string` | `long`、`double`、`bool`、`string` |
| `enum` | 基础类型为 `long` 的枚举 |
| `&TableName` | 相应记录对象引用 |
| `data` 类型字段 | 内联业务值 |
| `Option<T>` | 独立保存存在状态的 `Option<T>` |
| `[T]`、`{K: V}` | `IReadOnlyList<T>`、`IReadOnlyDictionary<K, V>` |
| 维度字段 | 字段原本的业务类型 |

记录和单例具有只读 `string id`。`data` 没有自动身份，可声明自己的 `id` 业务字段。字典继续使用 key/value 术语。生成属性公开读取、internal 赋值，构造入口为 internal；业务扩展位于同一程序集和 namespace 的独立 partial 文件。

CFT 合法名称遇到 C# 关键字时，在生成的类型、成员、枚举和 namespace 标识符中使用 `@` 转义。字段描述、CFD 名称与字符串查询保留原名。无法用转义解决的生成成员冲突在代码生成阶段报告。

## 注册、身份与构造

`CfdTypeKind` 明确区分表、内联数据和单例。`data` 不注册表，singleton 只注册单例。父表包含可赋值的派生记录，引用允许声明父类型并保留具体派生对象。

记录先按继承根与业务 id 建立身份，再用 `fill(value, reader)` 填充字段。构造中的引用可返回已登记、正在填充的对象，从而保留循环引用。`data` 使用 `construct(reader)` 构造完整内联值；抽象声明只提供类型与字段描述。

`CoflowData.Load(sources)` 返回 `LoadResult<CoflowData>`。加载验证基础值与每个已声明的单维度变体；失败只交付诊断，不交付部分数据。`Table<T>()` 缓存只读表，`Singleton<T>()` 返回已注册单例。基础表、父表和基础引用共享同一记录对象。

`@idAsEnum` 为对应表生成 `Get(enum)` 和 `TryGet(enum, out record)` 扩展。`TableIdMap<TId>` 明确映射枚举值到 CFD id；稳定编号来自项目 enum lock，输出映射只包含当前记录。未知枚举值的 `Get` 抛出 `KeyNotFoundException`，`TryGet` 返回 false。

## 可选值与默认值

`Option<T>` 只决定值是否可为空。加载器在源字段缺失时读取字段的显式默认节点；缺少默认节点即报告 `CFD-MISSING`，不根据 `Option` 自动补 `None`。此规则同时作用于记录、内联数据和嵌套构造。

默认节点保存 `Some` 的每一层，`None`、`Some(None)` 与 `Some(Some(value))` 状态不同。描述中的 `IsOptional` 用于禁止模板穿过可选值，不用于判断字段是否可省略。

`string` 统一接收普通字符串和显式 `f"..."` 源表达式。普通字符串保持字面文本，模板节点保留源结构以便按读取上下文求值。

## 独立记录视图

```csharp
var original = item.name;
var japanese = item.For("language", "ja");
var mobileJapanese = japanese.For("platform", "mobile");
var translated = japanese.name;
bool explicitTranslation = item.HasVariant(nameof(item.name), "ja");
var languages = item.Variants("language");
```

`For` 返回同一静态业务类型的独立记录视图，不修改基础对象和其他视图。链式调用替换指定维度的选择并保留其他维度；选择顺序不影响相同上下文的身份。已知维度的未知变体使用基础表达式，未知维度抛出 `ArgumentException`。

生成记录根类型显式实现 `ICfdRecord`，持有其 `CfdReader`。公共 `CfdRecordDimensions` 泛型扩展将 `For` 转发给读取器，因此派生记录调用仍返回其静态类型。视图上下文由不可变 `DimensionSelection` 表示，缓存键同时包含维度名与变体名。

加载上下文按选择组合缓存投影视图，每个组合再按原记录身份缓存对象。先登记投影对象，再填充字段，使循环引用和重复引用在同一视图内保持身份；不同视图的记录对象互相隔离。对象字段、数组、字典、Option 内的引用和内联数据都继承当前选择。

基础源、覆盖节点、来源定位及读取上下文随加载结果存活，供后续视图求值。已求值字段和已构造视图缓存到该加载结果的生命周期结束；重新加载创建独立上下文。视图构造通过上下文锁串行化，已构造的业务值只读访问。

## 显式覆盖与维度遍历

`record.HasVariant(field, variant)` 查询源文件是否明确赋值，不比较有效值。显式 `None`、空字符串和与基础相同的值均返回 true；仅由回退或模板产生结果返回 false。字段绑定决定所属维度。未知字段或未绑定维度的字段抛出 `ArgumentException`。

`record.Variants(dimension)` 返回项目中该维度全部已发现变体，按名称排序，包含当前记录没有显式赋值的变体。查询来源与执行视图相互独立，同一记录的不同视图返回相同的显式存在结果。

覆盖的唯一位置为维度、变体、记录 id、字段；重复位置报告来源错误。显式 `None` 是已提供值，不触发回退。编辑器的覆盖发现和写回见 [维度文件设计](../editor-design/dimensions.md)。

## 模板与上下文传播

模板只读取字段路径，末端允许 string、数字、bool 和枚举。静态描述拒绝 Option 末端、穿过 Option 的路径和复合末端。`self` 绑定模板所属记录或 data；数组、字典和 Option 保持所属对象，嵌套 data 重建自己的 self。

字段读取先依据其绑定维度选择源节点，再使用当前完整上下文求值。缺少覆盖时选取基础表达式并重新求值，不能重用基础上下文的渲染结果。例如 `title = f"购买 {self.name}"`，只有 name 提供日文覆盖时，日文视图中的 title 仍包含日文 name。

显式记录路径、普通引用路径及内联对象路径都传播上下文。同名变体在不同维度下不相互匹配。基础访问上下文为空，所以 `item.name` 始终读取基础值。

每个视图有独立字段缓存和循环检测集合。直接或跨记录模板环报告 `CFD-INTERPOLATION-CYCLE`，构造和读取深度限制为 256。已声明单维度变体的错误在加载时报告；首次选择新的多维度组合时若产生组合依赖错误，`For` 抛出携带来源诊断的 `CfdLoadException`。

## 验证边界

Rust 与 C# 共同消费 parser/runtime parity 夹具，覆盖声明、默认值、可选层、继承、单例、记录身份、普通字符串、模板和维度。生成集成项目实际编译生成源码，验证 partial 扩展、结构体、枚举 id、循环引用及直接基础字段读取。

C# 单元测试另验证显式覆盖存在性、独立视图、多个维度的组合和隔离、跨引用与内联模板、集合模板、Option 省略条件以及仅变体出现的依赖环。CFT 业务规则由 `coflow-checker` 执行，C# 描述不携带规则执行器。

仓库门禁以 [AGENTS.md](../../AGENTS.md) 为准。
