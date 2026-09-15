# Check 校验

> 状态：语言设计
>
> 范围：可选检查、函数调用和检查报告。

## 1. 检查入口

check 是特殊函数，使用普通函数的表达式、局部变量、控制流和执行机制。
它只在调用方要求检查时执行；不执行 check 也能正常使用 Runtime。
检查不通过或执行出错只产生报告，不改变 Runtime 的可用状态。

```text
check-block := "check" [ identifier ] block
check-decl := "check" identifier block
```

- 类型 check 位于字段之后，一个类型最多一个匿名 check，也可以有多个命名 check。
- 字段与命名 check 在同一类型及其继承链中禁止同名。
- 父类型 check 同样用于子类型，按父到子执行。
- 类型 check 检查对象内容，包括记录内容和内联对象；使用只读 self 访问字段。
- check 名称用于选择检查和定位报告，不是普通调用目标。
- check 不接受用户参数、不返回业务值，也不能被 CFD 覆盖或作为值传递。
- check 本体不能使用 return 或可选传播；它调用的普通函数遵循自身返回规则。

## 2. require

`require` 来自 `Coflow::Check`，使用前导入或写全限定名。
语法是 `require(condition, message)`，结果为 unit。

```cft
use Coflow::Check::require;

type Item {
  price: int;

  check priceValid {
    require(self.price >= 0, f"价格不能为负：{self.price}");
  }
}
```

- condition 必须为 bool。
- condition 为 false 时记录诊断，然后继续当前 check。
- message 必须产生 string，只在条件失败时求值。
- require 失败不提供类型收窄或可选解包保证。
- require 是具有延迟消息参数的检查内建，不作为普通函数值传递。
- require 用于 check 及其内部校验回调；普通函数通过返回值表达结果。

## 3. 调用普通函数

check 可以调用任何函数，包括 CFD 实现、回调和 Host 函数。
需要记录的函数通过参数显式接收记录，不隐式继承 check 的当前对象。

```cft
use Coflow::Check::require;
use Coflow::Check::records;

type Item {
  price: int;
}

const validPrice: fn(item: Item) -> bool =
  fn(item: Item) -> bool { item.price >= 0 };

check ItemPrices {
  for item in records(Item) {
    require(validPrice(item), f"物品 {item.id} 的价格无效");
  }
}
```

调用普通函数字段时，其 self 仍然是该函数所属对象。
Host 函数可以产生外部副作用；检查入口不会改变这种正常调用行为。

## 4. records 与遍历

`records(Type)` 来自 `Coflow::Check`，只用于顶层 check。

- Type 是静态对象类型。
- 返回 `[&Type]`，包含该类型及子类型的顶层记录，不包含内联对象。
- 按实际类型限定名、record key 稳定排序。
- 引用上的 id 是 string。依赖记录身份的规则写在顶层 check 中。
- 数组单绑定读取元素，双绑定读取索引和值；字典必须双绑定，按原始 CFT/CFD 声明顺序读取 key 和 value。

维度值通过显式选择或维度遍历内建检查。
一条 check 不会因为读取了维度字段而自动切换变体、重复执行。

## 5. 执行错误和预算

check 按语句顺序执行，局部变量与普通函数一致，不按根语句分别调度。

| 情况 | 行为 |
| --- | --- |
| require 条件为 false | 记录问题，继续当前 check |
| 条件、消息或被调用函数发生执行错误 | 记录错误，结束当前 check，继续其他 check |
| 本次检查的总预算耗尽 | 停止检查，报告未完成 |
| 检查结束 | 返回报告，Runtime 继续可用 |

一次检查及其直接调用、间接调用、fstring 读取和 Host 同步重入共用检查预算。
检查预算独立于后续普通调用。预算不能强行终止正在阻塞的 Host 函数。

## 6. 报告

报告包含规则名称、错误码、消息、数据与 CFT 位置及相关调用位置。
require 的消息不覆盖计算本身的错误。

每次请求检查都实际执行所选 check，不使用增量 check，也不复用过去的检查结果。
解析和编译结果可以复用；检查结果不作为运行时数据有效性的前置条件。
