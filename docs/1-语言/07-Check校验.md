# Check 校验

## 1. 声明式规则

check 是编辑器使用的只读数据校验规则。CFT 定义规则，CFD 提供被检查的数据；规则不进入生成的 C# 代码，C# 加载器只读取 CFD。

```text
record-check := "check" check-body
check-decl := "check" identifier check-body
check-body := "{" { check-statement } "}"
check-statement := expression [ ":" message ] ";"
                 | "when" expression check-body
                 | ("all" | "any" | "none") binding [ "," binding ] "in" expression check-body
message := string-literal | formatted-string-literal
```

记录声明内使用匿名 check，顶层规则必须命名。记录规则直接读取当前记录字段；顶层规则通过 `records(Type)` 获取记录。

```cft
check ItemPrices {
  all item in records(Item) {
    item.price >= 0 : f"物品 {item.id} 的价格不能为负";
  }
}
```

每条表达式必须产生 bool。条件不满足时生成检查问题，省略消息时使用默认诊断。消息使用普通字符串或基础字段路径插值，例如 `f"价格 {item.price} 无效"`。

## 2. 条件与集合规则

`when condition { ... }` 仅在条件成立时检查内部规则。条件不成立时跳过内部规则。

```cft
check EnabledItems {
  all item in records(Item) {
    when item.enabled {
      item.price > 0 : "启用的物品必须设置价格";
    }
  }
}
```

| 规则 | 条件 |
| --- | --- |
| all | 每个元素都满足内部规则 |
| any | 至少一个元素满足内部规则 |
| none | 所有元素都不满足内部规则 |

空集合满足 all 和 none，不满足 any。数组单绑定读取元素，双绑定读取索引与元素；字典双绑定读取 key 与 value。嵌套规则的绑定仅在自己的块内可见。

## 3. 维度读取

维度字段使用 `.default()` 读取基础值，使用 `.for(variant)` 选择变体，使用 `.variants()` 获取有效变体。规则通过显式读取决定检查哪个变体，每条规则执行一次。

```cft
check {
  name.default() != "";
  name.for("ja") != "" : "日语名称不能为空";
  all variant, text in name.variants() {
    text != "" : f"变体 {variant} 的名称不能为空";
  }
}
```

C# 对应 API 为 `DimensionValue<T>.Default`、`.For(variant)` 和 `.Variants`，读取同一份已加载数据。

## 4. 编辑器执行与报告

编辑器在数据校验时执行 check。CLI 的 `check` 验证 CFT 声明和 CFD 数据；`build`、`codegen` 生成业务类型，这些命令不执行 check 规则。记录规则应用于对应记录及其子类型记录；顶层规则检查项目中已选 CFD 构成的数据集合。

检查保持数据只读。条件不满足生成业务问题；表达式或消息求值失败生成计算诊断。遍历、内建操作与文本生成计入检查预算，预算耗尽时报告检查未完成。

报告包含规则位置、错误码、消息、相关记录以及 CFT/CFD 来源位置。检查报告与声明或数据加载诊断分别展示。

条件表达式、类型判断与检查专用内建操作见[校验表达式](./06-校验表达式.md)。
