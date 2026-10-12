# C# 生成类型与加载器

## 声明与目标类型

CFT 使用 table、data、singleton 三类声明。table 与 singleton 生成 partial class，保留 abstract、sealed 和继承约束；data 默认生成 partial class，`@struct sealed data` 生成 partial struct。enum 使用 long 基础类型。CoflowData 为 sealed partial class。

字段中的 `&TableName` 对应记录对象引用，data 名称对应内联数据。singleton 不作为字段类型。table 和 singleton 具有 string id，data 不提供 id。继承限定 data 继承 data，table 与 singleton 继承 table；singleton 本身封闭。

int/float 生成 long/double，可选层生成 Option<T>，集合生成 IReadOnlyList/IReadOnlyDictionary，维度字段生成 DimensionValue<T>。属性公开读取、internal 赋值，构造入口 internal。业务扩展与生成代码在同一程序集及 namespace 中组成 partial 类型。

## 生成与构造

生成器输出业务类型、枚举、字段描述、默认值与常量节点，以及类型明确的构造回调。公共解析、默认值读取、字段校验、引用连接、维度合成、插值及诊断由 Coflow.Loader 提供。

table 和 singleton 使用 create(key)、fill(object, reader) 两阶段构造，先建立全部记录身份，再读取字段与连接引用，允许跨文件引用及引用成环。data 使用 construct(reader) 一次性构造完整内联值。abstract 声明只提供字段及继承元数据，由具体子类型构造。

CfdTypeKind 贯穿注册与加载：data 不能作为顶层记录，table 不能构造内联值，singleton 只产生唯一实例。构造描述中的 CLR Type 只用于类型注册查找；字段读取与赋值使用生成的泛型回调，不通过反射访问业务成员。

## 数据访问

CoflowData.Load(sources) 返回 LoadResult<CoflowData>。成功结果保存注册后的业务表与单例；失败返回来源诊断，不交付部分数据。

CoflowData.Table<T>() 返回共享的 Table<T>，支持 string 索引、TryGetValue、ContainsKey、Count、Keys、Values 和遍历。父类型表包含子类型记录，表与引用共享对象。Singleton<T>() 只访问注册的单例。data 与 singleton 不进入 table 注册；错误类型访问抛出 InvalidOperationException。

@idAsEnum 为 table 提供稳定编号枚举。每个适用的具体或父表生成接收者为 Table<业务类型> 的 Get(enum) 和 TryGet(enum, out record) 扩展，通过静态 enum->CFD key 映射查询。未知枚举值的 Get 抛出 KeyNotFoundException，TryGet 返回 false。映射包含当前有效记录 key，稳定编号保存在项目 enum lock 中，不通过 enum.ToString 推断。

CoflowData 不生成业务类型同名的字典属性。Table<T>() 缓存创建后的只读表，多次调用返回同一表；结果不保留 parser AST、来源索引、字段读取缓存或第二份通用记录模型。

## 可选值、维度与模板

Option<T> 独立保存 IsSome/IsNone/Value，默认节点保留 Some 包裹。None、Some(None)、Some(Some(value)) 均保持不同状态，强类型存在节点允许有限递归结构体。

维度覆盖使用 `dimension[variant] { key: Table { field: value } }`，同一维度、变体、记录、字段重复覆盖报错。覆盖只替换明确字段，未覆盖和未知变体回退基础值。DimensionValue<T> 提供 Default、For(variant)、Variants；显式 None 保持为空。

fstring 在加载时生成文本，只读取字段路径和可转文本的基础末端，循环依赖产生来源诊断。self 绑定模板所在的记录或 data 对象；数组、字典和 Option 传递所属对象，嵌套 data 重新绑定。id 只属于顶层记录，data 不提供 id。

编辑器 cell 输入验证模板语法并保存源码，完整模型在写入后统一求值；同一求值器负责默认值、集合、可选值和维度覆盖。显式引用的目标类型保留到记录链接校验阶段，错误保留来源和可修复的记录状态。float 文本保持与 Rust 十进制表示一致。值解析和依赖深度上限为 256，所有递归构造在进入前计入预算。

## 验证与工具边界

Rust 和生成 C# 共同消费 parser/runtime parity 夹具，覆盖声明边界、partial 扩展、枚举主键、继承表、单例、64 位值、可选层、引用身份及覆盖，并核对嵌套模板所属对象、记录 id、显式子类型引用与重复内联字段。语言服务、格式化器、编辑器和 VS Code 使用同一声明分类。编辑器 DTO 在 core 导出，宿主插件 DTO 在 Tauri 导出；固定 CFD loader 测试保持 runtime 私有。
