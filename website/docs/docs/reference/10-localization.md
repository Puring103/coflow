# 本地化与维度

维度是 CFD overlay 的 schema 级语义。配置声明变体和目录：

```yaml
dimensions:
  language:
    variants: [en, zh]
    out_dir: data/dimensions/language
```

每个变体文件都是 `.cfd`。runtime 在同一 generation 中加载基础记录和变体记录，check 对每个有效组合执行；代码 generator 根据 schema 生成目标语言的维度访问 API。

C# generator 将维度 source 清单写入 binding。加载时，生成代码按 source type、字段以及 singleton 字段 key 将 overlay 记录规范化为内部 variant 记录，再直接构造 `Localized<T>`；调用方不需要提供 localization provider。内部 variant 类型不作为数据库 table 暴露。

```csharp
Localization.CurrentLanguage = "zh";
var current = tables.UiText.Welcome.Value;
var explicitValue = tables.UiText.Welcome.For("en");
```

`For(language)` 优先返回对应非空 variant；variant 缺失、显式为 `null` 或语言未知时返回基础字段值。`Value` 等价于 `For(Localization.CurrentLanguage)`。

重复 key、未知字段和覆盖类型不匹配会报告 source span。overlay 的 declared type 必须可赋值给字段所属 source type；Rust 项目加载和生成的 C# loader 使用同一约束。
