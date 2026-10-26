# 设计维度和覆盖

先确定哪些顶层字段随哪个维度变化，以及哪些变体必须填写。同一字段绑定一个维度；多个字段可分别属于语言、地区或平台。

```cft
table Item {
  @localized
  name: string;
  @dimension("region")
  price: int;
  @localized
  label: string = f"购买 {self.name}";
}
```

```cfd
sword: Item { name: "铁剑", price: 90 }
language[ja] { sword: Item { name: "剣" } }
region[cn] { sword: Item { price: 80 } }
```

@localized 等价于绑定 language，维度字段属于 table/singleton。需要覆盖嵌套 data 的某些内容时，对拥有该对象的顶层字段设计完整覆盖；不向 data 字段直接添加记录维度绑定。

## 替换与回退

覆盖目标记录必须已存在。覆盖替换完整顶层字段，对象、数组和字典不逐项合并，对象内部仍补默认值。同一维度/变体/记录/字段的重复覆盖报错；文件顺序不表示优先级。

未覆盖或未知变体使用基础表达式，显式 None 是有效覆盖。基础模板在所选上下文重新求值：上例日语 label 是“购买 剣”，但 label 没有显式日语覆盖。

## 变体发现和文件组织

变体从全部输入中的覆盖块发现，空 `language[ja] {}` 也声明变体。YAML dimensions 配置目录与显示名称，不是固定变体清单。

覆盖可与基础数据同文件，也可在独立目录，通过 data 输入或 dimensions.<名称>.dir 纳入项目。已有覆盖写回原来源；新增覆盖文件不能与已存在覆盖重复。

编辑器推荐目录保留基础文件相对项目根的路径，例如 data/items.cfd 对应 dimensions/language/data/items.cfd；项目内新建覆盖时先配置 dir。路径细节见[项目配置](project-config.md)。

## 翻译完整性规则

将下面规则放在含 name 的类型字段之后：

```cft
check Translations {
  name.hasVariant("ja"): "必须填写日语名称";
  !name.for("ja").isBlank(): "日语名称不能为空";
  all variant, text in name.variants() {
    name.hasVariant(variant): f"缺少 {variant} 名称";
    !text.isBlank(): f"{variant} 名称不能为空";
  }
}
```

variants 返回已发现变体的有效值；未知必需变体要显式检查。hasVariant 不能代替内容质量校验，空字符串和同值覆盖也算已填写。

## 改变绑定

新增绑定时基础字段仍需满足类型与默认值约束。删除或重命名维度、改变字段归属或类型时，同步调整所有已有覆盖、规则中的 for/hasVariant 及依赖模板。使用文件候选一起校验，不能只改 CFT 后留下无法加载的覆盖。

用 dimension inspect/get/missing 和 check --rules 验证绑定、显式覆盖与规则；它们各自回答不同问题。C# 应用按用途选择 For 视图，不通过全局切换修改基础对象。
