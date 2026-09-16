# 本地化与维度

维度为同一记录字段提供基础值和任意数量的变体值。字段使用 `@localized` 绑定
`language` 维度，或使用 `@dimension("name")` 绑定其他维度。

```cft
table Item {
  @localized
  name: string;

  @dimension("region")
  price: int;
}
```

维度值直接写在业务 CFD 中：

```cfd
sword: Item {
  name: dimension {
    default: "Sword",
    zh: "剑",
    ja: "剣",
  },
  price: dimension {
    default: 100,
    cn: 90,
  },
}
```

维度字段必须使用 `dimension { ... }`，其中 `default` 必填。其他字段名即变体名，
值类型与原字段一致；只有 `T?` 字段的变体可以显式写 `None`。

变体从项目加载的全部 CFD 动态汇总，不需要在 `coflow.yaml` 中声明。缺少某个变体时
继承 `default`，未知变体查询同样返回 `default`；显式 `None` 是可选字段的有效覆盖。
读取基础值使用 `.default()`，读取指定变体使用 `.for("zh")`，`.variants()` 返回当前
维度的全部有效变体值。

C# 生成字段使用 `RuntimeDimension<T>`，提供对应的 `Default()`、`For(...)` 和
`Variants()` 读取方式。
编辑器的维度展开视图直接编辑所属业务记录，不会生成辅助类型、记录或 CFD 文件。
