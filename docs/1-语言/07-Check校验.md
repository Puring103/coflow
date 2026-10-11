# Check 校验

> 范围：现行检查块语法、执行入口、require 与检查报告。

## 1. 检查块

```text
check-block := "check" [ identifier ] check-body
check-decl := "check" identifier check-body
```

记录内可以声明匿名或命名 check，顶层 check 必须命名：

```cft
use Coflow::Check::require;
use Coflow::Check::records;

table Item {
  price: int;

  check {
    require(self.price >= 0, "价格不能为负");
  }

  check priceLimit {
    require(self.price <= 10000, "价格超过上限");
  }
}

check ItemPrices {
  for item in records(Item) {
    require(item.price >= 0, f"物品 {item.id} 的价格无效");
  }
}
```

- 记录 check 位于全部字段之后；每个记录类型最多一个匿名 check，可有多个命名 check。
- 字段与命名 check 在同一类型及其继承链中禁止同名。
- 父类型规则也用于子类型，按父到子执行。
- 记录 check 使用只读 self；顶层 check 显式查询记录。
- data 不声明 check，字段、集合与引用不触发自动递归检查。
- check 名称用于选择规则和定位报告；check 不接受用户参数、不返回业务值。
- check 由 CFT 声明，CFD 提供被检查数据。

检查体中的现行局部变量、条件、运算与遍历见[校验表达式](./06-校验表达式.md)。

## 2. require 的现行形式

使用 `Coflow::Check::require` 全限定名，或通过 use 导入后调用：

```cft
require(condition, message);
```

condition 必须为 bool，message 必须产生 string。
条件为 false 时向本次报告记录问题，然后继续当前 check。
条件和消息从左到右立即求值；条件为 true 时消息也会求值。
任一参数求值失败时报告计算错误，不提交该条业务消息。
require 失败不提供类型收窄或可选解包保证。

## 3. records 与维度遍历

`Coflow::Check::records(Type)` 只用于顶层 check，Type 为静态 table 或 singleton 类型。
结果包含该类型及其子类型的记录，按实际类型限定名与记录 key 稳定排序。
数组、字典的绑定与遍历顺序见[校验表达式](./06-校验表达式.md)。

维度值通过 `.for(variant)` 显式选择，或通过 `.variants()` 返回的名称到有效值字典显式遍历。
读取维度字段不会自动切换变体或重复执行 check。

## 4. 执行与报告

check 只在调用方请求时执行；不执行也能正常使用 Runtime。
一次请求可以选择记录范围、规则名称及是否包含顶层规则。
每次请求实际执行所选规则，不复用过去的检查结果。

| 情况 | 行为 |
| --- | --- |
| require 条件不满足 | 记录问题，继续当前 check |
| 表达式或消息求值出错 | 记录错误，结束当前 check，继续其他规则 |
| 总预算耗尽 | 停止检查，报告未完成 |
| 检查完成 | 返回报告，Runtime 继续可用 |

本次检查中的遍历、内建操作与文本生成共同计入执行预算。
报告保留规则名称、错误码、消息、被检查记录及 CFT/CFD 来源位置。
检查报告与声明或加载诊断分别展示，不改变已构建 Runtime 的可用状态。
