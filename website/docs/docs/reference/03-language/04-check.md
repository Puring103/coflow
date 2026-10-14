# Check 校验

check 为编辑器提供只读数据校验规则。CFT 定义规则，编辑器在已加载数据上执行并展示业务问题、计算诊断和完成状态。

```cft
table Item {
  price: int;
  tags: [string];
  check {
    price >= 0: "价格不能为负数";
    tags.isUnique(): "标签不能重复";
  }
}
check ItemPrices {
  all item in records(Item) {
    item.price >= 0: f"物品 {item.id} 的价格无效";
  }
}
```

类型中的匿名 check 位于全部字段之后，顶层规则必须命名。顶层规则使用 `records(TypeName)` 获取 table 或 singleton 的记录集合；data 不提供记录集合。每条条件返回 bool；可选消息使用普通字符串或 fstring。`when condition { ... }` 控制内部规则，`all`、`any`、`none` 对集合逐项验证；空集合满足 all 和 none，不满足 any。

`is Some(value)` 解包一层 Option；`is None` 判断空态；`is TypeName` 判断实际类型。条件可以使用字段路径、索引、算术、比较、逻辑、位运算及固定检查操作，如 len、contains、isUnique 和 matches。逻辑条件短路求值，越界、溢出和除零产生计算诊断。

维度字段使用 `.default()`、`.for("ja")` 和 `.variants()` 明确选择检查值。遍历、操作及消息生成计入检查预算，耗尽时报告检查未完成。

CLI check 校验声明与 CFD 数据，build/codegen 生成业务类型。生成的 C# 源码与 Coflow.Loader 提供数据加载；check 规则由编辑器执行。
