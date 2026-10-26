# 编写和验证业务规则

规则只读，不修复或计算写回数据。先明确约束适用于内联对象、单条记录还是整个项目，再选择放置位置。

## 位置和身份

data、table、singleton 的所有字段放在 check 之前。data check 自动检查每个内联实例，包括集合和 Option 内的对象；父规则先执行，命名规则在继承链内唯一。项目规则写在顶层。

```cft
table Item {
  price: int;
  tags: [string] = [];
  next: Option<&Item> = None;
  check ValidPrice: "物品价格无效" {
    price >= 0: "价格不能为负数";
  }
  check Links {
    when next is Some(target) { target.price >= 0; }
  }
  check { tags.isUnique(): "标签不能重复"; }
}
check HasItems {
  records(Item).len() > 0: "至少需要一个物品";
}
```

规则名可省略，但至少有一条语句。命名类型规则通过 `Item::ValidPrice` 查询，顶层规则通过 HasItems 查询；匿名标识必须从 rule list 取得，源码位置变化后重查。

## 条件与量词

断言为 bool 表达式加分号，可在冒号后给普通或 f 字符串消息。规则、when、量词也可在左花括号前添加消息。

| 语法 | 含义 |
| --- | --- |
| `when condition { ... }` | 条件成立才执行主体 |
| `all item in collection { ... }` | 每项都成立；空集合成立 |
| `any item in collection { ... }` | 至少一项成立；空集合失败 |
| `none item in collection { ... }` | 没有项成立；空集合成立 |

数组双绑定为 `item, index`，字典双绑定为 `key, value`。绑定只在主体内可用，块前消息属于外层作用域。`is Some(value)` 解包，`is None` 判断空态，`is ConcreteType` 判断实际类型并收窄字段访问；逻辑 && / || 短路。

any/none 中要同时筛选并判断时写 `item.enabled && item.price > 0`。仅用 `when item.enabled { ... }` 不等于筛选，因为条件不成立的 when 也正常完成。

## 表达式与方法

支持字段、索引、算术、比较、逻辑和适用的位运算。`//` 为整数除法，`%` 为整数取余，`**` 为乘方；用括号明确混合运算。整数溢出、除零、越界、缺失字典键产生求值错误。

| 接收值 | 常用方法 |
| --- | --- |
| string | len、isBlank、contains、startsWith、endsWith、matches |
| 数组 | len、contains、isUnique、min、max、sum、isSorted、isStrictlySorted |
| 字典 | len、contains/containsKey、containsValue、keys、values |
| 支持标量元素的集合 | intersects、isDisjoint、isSubsetOf、isSupersetOf |
| 数值 | abs |
| 浮点 | isFinite、approxEqual(other, tolerance) |
| 维度字段 | for、hasVariant、variants |

方法采用 `value.len()` 形式；matches 的正则是静态字符串字面量。空数组不能求极值；可先用 when 或短路保护。records(Type) 获取记录集合，EnumName(integer) 构造枚举值。不要假设任意脚本函数或对象方法可用。

## 维度约束

直接读字段得到基础值；`name.for("ja")` 是有效变体，`name.hasVariant("ja")` 判断显式覆盖。variants() 遍历该维度已发现变体的有效值，包括回退值。要求某种语言必须填写时显式检查 hasVariant，不能只遍历尚未出现的变体。

## 验证和解释

```sh
coflow rule list --project <项目> --json
coflow check --project <项目> --rule Item::ValidPrice --explain --json
coflow check --project <项目> --rules --json
```

`--rules` 与 `--rule` 互斥。新增或调整规则后用真实记录确认含义，候选 apply --rules 可同时校验并发布。

普通业务失败继续收集断言；求值错误停止当前实例的该条规则，其余规则继续。报告区分 passed、not_triggered、failed、evaluation_error、budget_exhausted、not_executed。complete 表示检查是否完整，不能把零条可见诊断或条件未触发等同于所有业务情况均通过。

规则报错先看 help、record、field_path、related 和 contexts；需要逐目标结果时加 details。预算耗尽时改进规则遍历或缩小定位范围，完成交付前仍需执行要求的完整范围。
