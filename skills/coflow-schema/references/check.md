# Check 校验

check 为配置定义只读业务规则。`data`、`table` 和 `singleton` 可以在字段之后声明多个命名规则，项目规则可以独立声明在顶层。

```cft
data Reward {
  count: int;
  check PositiveCount: "奖励数量无效" {
    count > 0: f"数量 {count} 必须大于 0";
  }
}

table Item {
  price: int;
  tags: [string];
  reward: Reward;
  check Price { price >= 0: "价格不能为负数"; }
  check Tags { tags.isUnique(): "标签不能重复"; }
}
```

规则必须命名且至少包含一条语句。同一类型及其继承链内不能重名，无继承关系的类型可以使用相同短名。父类型规则先执行，再按声明顺序执行本类型规则。data 规则会自动检查每个内联实例，包括数组、字典和可选值内的对象。

每条断言返回 bool，可在冒号后提供普通字符串或 `f"..."` 消息。规则、when 和量词也可以在左花括号前提供消息；外层消息补充具体断言的错误内容和位置。

## 条件、集合与关联

顶层规则使用 `records(Type)` 获取 table 或 singleton 的记录集合。字段引用可以直接访问目标字段，例如 `reward.item.enabled`。

```cft
check AvailableItems {
  any item in records(Item): "至少需要一个可用物品" {
    item.price > 0 && item.tags.contains("available");
  }
}
```

| 语法 | 要求 |
| --- | --- |
| `when condition { ... }` | 条件成立时执行主体 |
| `all item in collection { ... }` | 全部元素满足主体 |
| `any item in collection { ... }` | 至少一个元素满足主体 |
| `none item in collection { ... }` | 没有元素满足主体 |

空集合满足 all 和 none，不满足 any。数组双绑定为 `item, index`，索引从零开始；字典双绑定为 `key, value`。量词绑定只在主体内可用，块前消息使用外层作用域。

`when` 条件不成立时主体不执行，该语句正常完成。在 any/none 中同时筛选和检查元素时，使用 `item.enabled && item.price > 0` 这样的布尔组合。

`is Some(value)` 解包一层 Option，`is None` 判断空态，`is TypeName` 判断实际类型。可以使用字段路径、索引、算术、比较、逻辑和位运算，以及 `len`、`contains`、`isUnique`、`matches`、`sum` 等操作。逻辑条件短路求值。

## 检查维度

直接读取字段取得基础值；`for` 选择变体；`hasVariant` 判断字段是否显式填写该变体；`variants` 遍历所属维度全部已发现变体的有效值。

```cft
table Text {
  @localized name: string;
  check TranslationComplete {
    !name.isBlank(): "基础名称不能为空";
    name.hasVariant("ja"): "必须填写日语名称";
    all variant, text in name.variants(): "翻译不完整" {
      name.hasVariant(variant): f"缺少 {variant} 的名称";
      !text.isBlank(): f"{variant} 名称不能为空";
    }
    !name.for("ja").isBlank(): "日语名称不能为空";
  }
}
```

显式 `None`、空字符串或与基础值相同的覆盖也算存在。缺少覆盖时 for 使用基础表达式；其中的模板引用继续读取所选变体。variants 包含回退值，hasVariant 可用于区分已填写和回退。尚未在项目中出现的变体应通过 `hasVariant("ja")` 这样的显式规则检查。

## 执行和诊断

```sh
coflow rule list
coflow check --rules
coflow check --rule Reward::PositiveCount --explain
```

类型规则以 `类型::规则名` 选择，顶层规则直接使用名称。`coflow check` 检查声明与 CFD 数据，`--rules` 执行全部业务规则，`--rule` 选择规则，`--explain` 展示执行解释。`coflow build` 在全部校验通过后生成代码。编辑器在编辑过程中执行规则并显示诊断。

普通断言失败会继续收集错误；除零、越界等求值错误停止当前规则，其余规则继续。报告区分通过、条件未触发、业务失败、求值错误、预算耗尽和未执行，并包含规则名、CFT 位置、CFD 字段位置及相关元素或变体。预算耗尽表示检查未完成。CLI 默认返回计数与有界诊断，`--details` 展开逐目标规则和语句状态，`--explain` 提供实际值与执行解释。

CLI 的数据、统计、引用和维度查询可以辅助查找规则涉及的记录。生成的 C# 类型与 Coflow.Loader 负责加载数据，业务规则在交付前通过 CLI 或编辑器验证。
