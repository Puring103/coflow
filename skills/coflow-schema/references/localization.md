# 本地化与维度

`@localized` 将字段绑定 language 维度，`@dimension("name")` 绑定指定维度。同一字段保存基础值和任意数量的变体值。

```cft
type Item {
  @localized
  name: string;
  @dimension("region")
  price: int;
}
```

```cfd
sword: Item { name: "Sword", price: 100 }
language[ja] { sword: Item { name: "剣" } }
region[cn] { sword: Item { price: 90 } }
```

覆盖块定位已有记录，只替换明确写出的维度字段。字段必须绑定外层维度，覆盖值符合字段声明类型；同一维度、变体、记录和字段重复覆盖时报错。

变体从全部所选 CFD 的覆盖块汇总，文件顺序不提供优先级。未覆盖或未知变体使用基础值；Option 字段的显式 None 是有效覆盖，保留当前层空态。

编辑器检查使用 `.default()`、`.for("ja")` 和 `.variants()`。C# 字段使用 `DimensionValue<T>`，通过 `Default`、`For("ja")` 和 `Variants` 在同一份加载结果中读取多个变体。

已有覆盖修改后写回来源文件，首次添加覆盖由用户选择目标 CFD 文件。基础记录重命名或删除时同步更新覆盖目标。
