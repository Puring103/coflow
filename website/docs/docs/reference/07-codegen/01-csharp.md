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

`Coflow.Cfd.Runtime` 负责 CFD tokenizer/parser、span、文本读取、引用缓存和资源限制；生成 binding 为每个可实例化类型生成显式构造函数调用，并使用 `CfdValueReader` 完成值转换。生成代码不使用反射、JSON 或 MessagePack，runtime 不加载 CFT，也不会根据目录或文件名猜测类型。

目标语言选项写在 `codegen` target 中，例如 `namespace`、`database_class`、`int_32` 和 `float_32`。新增其他语言只需实现同一个 codegen contract 和对应 runtime binding。
