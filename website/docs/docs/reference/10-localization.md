# 本地化与维度

维度为同一个字段提供多组变体值。配置声明变体和目录：

```yaml
dimensions:
  language:
    variants: [en, zh]
    out_dir: data/dimensions/language
```

字段通过 `@localized` 使用 `language` 维度，也可以通过 `@dimension("维度名")` 指定其他维度：

```cft
type UiText {
  @localized text: string;
}
```

基础文件保存默认值：

```cfd
welcome: UiText { text: "Welcome" }
```

维度文件使用 `__coflow_<维度名>_<类型名>_<字段名>` 作为记录类型，记录 key 与基础记录一致：

```cfd
welcome: __coflow_language_UiText_text {
  en: "Welcome",
  zh: "欢迎",
}
```

每份 `.cfd` 维度文件可包含多个变体。普通类型的文件位于维度目录中的 `<bucket>_<字段名>.cfd`，未指定 bucket 时使用类型名；singleton 使用 `<类型名>.cfd`，每行的 key 是字段名，记录类型仍包含对应字段名。继承字段使用声明该字段的类型名。

`coflow build` 生成或更新维度文件，其中 `default` 展示基础字段值；修改默认值应编辑基础记录。`__coflow_` 是系统保留的名称前缀。

C# 中将基础文件和维度文件作为 `CoflowSource` 加载，再调用 `Compile`：

```csharp
var coflow = Schema.Create();
coflow.LoadModule(
    new CoflowSource("base.cfd", baseText),
    new CoflowSource("translations.cfd", translationsText));
var result = coflow.Compile();
if (!result.Success)
    throw new CoflowLoadException(result.Diagnostics);
var welcome = coflow.Table(UiText.Table).Get("welcome").Value;
var original = welcome.text.Default;
var translated = welcome.text.For("zh");
```

`For(variant)` 返回对应变体；变体缺失、显式为 `None` 或名称未知时返回基础字段值。C# 调用方负责读取并传入文件内容，文件路径不决定字段归属。维度记录不作为业务表暴露。

维度记录类型必须与目标字段对应的辅助类型一致。重复记录、未知变体、值类型错误和缺失的目标记录会报告诊断。
