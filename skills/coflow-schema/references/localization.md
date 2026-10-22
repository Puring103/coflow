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

Check 中直接读取字段得到基础值，`.for("ja")` 读取有效变体，`.hasVariant("ja")` 判断该字段是否有显式覆盖。`.variants()` 返回所属维度全部已发现变体的有效值，包含没有填写覆盖的变体。

```cft
check TranslationComplete {
  all variant, text in name.variants() {
    name.hasVariant(variant): f"尚未填写 {variant} 名称";
    text.len() > 0: f"{variant} 名称不能为空";
  }
}
```

此规则放在包含 `name` 字段的类型中。要强制要求某个变体，即使整个项目尚未出现它，也要明确检查 `name.hasVariant("ja")`。

C# 直接访问 `item.name` 得到基础字符串。按需使用 `item.For("language", "ja").name`，不同用途可同时持有不同视图；`item.HasVariant("name", "ja")` 判断显式覆盖，`item.Variants("language")` 列出维度已知变体。多维组合可以链式调用 `For`，例如 `item.For("language", "ja").For("region", "cn")`。

模板读取依赖时使用当前维度上下文。假设 `title` 是 language 字段，基础表达式为 `f"购买 {self.name}"`，日语只覆盖 `name = "剣"`，那么 `title.for("ja")` 得到 `"购买 剣"`，而 `title.hasVariant("ja")` 仍为 `false`。缺少覆盖时回退基础表达式，再在当前上下文中求值；其他维度的同名变体不会自动被选中。

## 查询覆盖是否填写

CLI 可以分别查询覆盖状态和有效值：

```powershell
coflow dimension get --type Item --id sword --field name --dimension language --variant ja --json
coflow dimension missing --dimension language --variant ja --json
```

`dimension get` 的 `explicit` 表示该字段是否存在显式覆盖，`override` 是显式覆盖值，`effective` 是读取结果。`variant_exists` 表示整个维度中是否已存在该变体，与单个字段是否填写分别判断。显式 `None` 和等于基础值的覆盖都算已填写。

`dimension missing` 列出缺少显式覆盖的字段，适合检查翻译完整性。`dimension clear` 清除覆盖并恢复基础值读取；清除后的字段重新计入缺失项。

## 文件与编辑

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
