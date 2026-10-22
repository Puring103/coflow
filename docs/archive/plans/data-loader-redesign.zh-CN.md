# 数据加载器实现边界

> 历史方案归档。当前设计见 [文档索引](../../README.md)。

## 最终职责

- CFT 供编辑器声明校验及代码生成使用；C# 不读取 CFT。
- C# `Coflow.Loader` 读取调用方提供的 CFD 内容，直接构造生成类型。
- 生成代码提供业务类型、静态加载描述及类型明确的构造入口；共享库提供公共加载算法。
- table 与 singleton 保存记录身份并生成 partial class，data 保存内联值并生成 partial class 或 @struct partial struct。
- CoflowData 使用 Table<T>()、Singleton<T>()，枚举主键使用对应表的 Get/TryGet 扩展。
- int/float 使用 long/double，Option<T> 每层独立保存 None/Some 状态。
- 维度覆盖采用 `dimension[variant] { key: Type { field: value } }`；文件来源不决定优先级，重复字段覆盖报错。
- 维度字段保留基础值和全部覆盖，使用 `Default`、`For(variant)` 和有效变体集合读取。
- 只有加载期间保留解析、索引与构建材料；结果为一份生成类型组成的业务数据。
- fstring 只提供字段路径、基础值转文本和花括号转义；在加载时求值，循环依赖报错。
- check 仅由编辑器专用求值器执行；采用条件语句、when 与 all/any/none，不进入 C#。
- 加载契约由生成类型、类型描述、默认值节点、Option 和维度值组成。

## 维度读取

维度字段使用 `DimensionValue<T>`。加载时构造默认值和全部变体，调用方通过 `For(variant)` 选择读取的变体。同一份数据可以读取多个变体，无须重新加载。

```csharp
var defaultName = sword.Name.Default;
var japaneseName = sword.Name.For("ja");
var englishName = sword.Name.For("en");
```

变体中未覆盖的字段使用默认值；未知变体也使用默认值。显式 `None` 覆盖保留为空值。`Variants` 提供该维度各变体对应的有效字段值。

## 验证范围

- Rust 工作区检查与测试通过。
- C# 加载器覆盖生成代码接入、结构体、引用成环、默认值、跨文件覆盖、For 回退、显式空覆盖、基础插值和诊断。
- 编辑器覆盖来源写入、目标文件选择、重命名和删除联动。
- 生成的代码与 C# 加载器联调通过，普通数据和维度值均直接落在生成类型中。
- Rust 与生成的 C# 共同消费 parser/runtime parity 夹具，验证同一数据的接受结果、数值精度、可选层次、引用身份与覆盖。
- 内部文档、公开参考、技能引用、构建脚本和测试工程使用同一语言与加载契约。
