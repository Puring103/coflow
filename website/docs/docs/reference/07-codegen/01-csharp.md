# C# 代码生成

C# generator 生成 schema declarations、显式 CFD bindings 和数据库入口。生成目录只包含 `.cs`，不包含数据副本。

```csharp
public static IReadOnlyList<string> SourceFiles { get; }
public static GameConfig Load(
    ICfdTextLoader loader,
    CfdLoadOptions? options = null);
public static GameConfig Load(
    Func<string, string?> readText,
    CfdLoadOptions? options = null);
public static GameConfig Load(
    IEnumerable<CfdSource> sources,
    CfdLoadOptions? options = null);
```

`Coflow.Cfd.Runtime` 负责 CFD tokenizer/parser、span、文本读取、继承域引用缓存和资源限制；生成 binding 为每个可实例化类型声明 CFT source name、可赋值 ancestor，并生成显式构造函数调用。reader 使用 `CfdValueReader` 完成值转换，在字段缺失时应用 CFT 默认值；没有默认值的字段保持必填。生成代码不使用反射，runtime 不加载 CFT，也不会根据目录或文件名猜测类型。

布尔值只接受 CFD 小写字面量 `true` 和 `false`。enum、flag 和多态对象按 CFT source name 解析，不依赖 C# 标识符重命名后的名称。

source 清单区分普通项目 CFD 与维度 overlay。生成 loader 对物理路径去重，并按 source type、字段和 singleton 字段 key 规范化 overlay；`Localized<T>` 的 variants 直接从这些 CFD 记录构造，不需要外部 localization provider，也不将内部 variant 类型暴露为数据库 table。缺失、`null` 或未知语言均回退到基础值。

Rust 项目加载与 C# direct loader 都拒绝不可赋值的 overlay declared type、重复 record identity、缺失引用和引用环。引用环分别以稳定的 Rust `REF-003` 与 C# `CFD-REF-CYCLE` 诊断报告。

目标语言选项写在 `codegen` target 中。C# 目标只接受 `namespace`；CFT `int` 固定映射为 C#
`long`，`float` 固定映射为 C# `double`。新增其他语言只需实现同一个 codegen contract 和对应
runtime binding。
