# 本地化与维度

`@localized` 将字段绑定 language 维度，`@dimension("name")` 绑定指定维度。同一字段保存基础值和任意数量的变体值。

```cft
table Item {
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

编辑器通过 `coflow.yaml` 中的 `dimensions.<维度名>.dir` 配置覆盖目录：

```yaml
dimensions:
  language:
    display_name: 本地化
    dir: dimensions/language/
  region:
    display_name: 地区
    dir: dimensions/region/
```

这些目录递归加载 CFD，无需在 `data` 中重复声明；尚未创建的目录视为空目录。编辑器首次填写覆盖值时自动创建文件，并保留基础文件相对项目根目录的完整路径。例如，`data/equipment/items.cfd` 的 language 覆盖保存到 `dimensions/language/data/equipment/items.cfd`。同一基础文件的全部记录、字段和变体按维度集中保存，文件名沿用 `items.cfd`。

这是编辑器推荐的文件组织方式。覆盖块可以存放在任意已纳入数据输入的 CFD 文件中，也可以与基础记录放在同一文件。已有覆盖修改后写回来源文件。基础记录重命名或删除时同步更新覆盖目标。

编辑器文件树分别展示类型、基础数据和各维度，维度树按实际目录和 CFD 文件提供入口。点击文件可在表格或记录视图中编辑该文件涉及的覆盖字段；不同来源的已有覆盖显示其真实来源并写回该来源。纯覆盖文件通过维度树查看和编辑，同时包含基础记录的文件保留在数据树中。

在项目内的基础文件上右键选择“编辑本地化”或相应维度，即可打开推荐文件；首次填写时创建目录和文件。使用“添加变体”添加待填列，填写覆盖值后保存变体。新增覆盖前需配置该维度的 `dir`，项目外基础文件的覆盖可在已有覆盖文件中编辑。
