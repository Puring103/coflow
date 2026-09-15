# 本地化与维度

维度为同一记录字段提供多组变体值。项目配置声明变体和输出目录：

```yaml
dimensions:
  language:
    variants: [en, zh]
    out_dir: data/dimensions/language
```

记录字段使用 `@localized` 绑定 language 维度，或用 `@dimension("name")` 绑定其他维度。
data 不声明维度字段。

```cft
table UiText {
  @localized
  text: string;
}
```

业务 CFD 写基础值，维度 CFD 写覆盖值：

```cfd
welcome: UiText { text: "Welcome" }
welcome: UiText_text_language { en: None, zh: "欢迎" }
```

`UiText_text_language` 是生成的维度记录类型短名，只在维度记录类型位置特殊解析；
有歧义时使用生成类型的完整限定名。覆盖记录 key 与业务记录一致。

`coflow build` 按字段生成或更新维度文件。table 和 singleton 都使用每字段一份文件，
文件名为 `<bucket>_<字段名>.cfd`；未指定 bucket 时使用类型名。
继承字段按声明类型归属。修改基础值应编辑业务记录。

读取基础值使用 `.default()`，指定变体使用 `.for("zh")`。
覆盖为 None、未提供或变体名未知时回退基础值。没有隐式的当前语言。
未知变体数据不加载；更新维度文件时清理已删除业务记录的覆盖。

C# 包装提供 `Default()`、`For("zh")`，使用方式见 [C# 代码生成](./07-codegen/01-csharp.md)。
将业务和维度 CFD 一起提交构建器后再构建运行时。
模板和函数覆盖保留原业务对象绑定，当前版本不执行模板求值、语言内部方法或 check。
