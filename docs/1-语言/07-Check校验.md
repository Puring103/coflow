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

- 记录类型的 check 位于字段之后，一个记录类型最多一个匿名 check，也可以有多个命名 check。
- 字段与命名 check 在同一类型及其继承链中禁止同名。
- 父类型 check 同样用于子类型，按父到子执行。
- check 只针对记录，使用只读 self 访问该记录的字段及 string 类型的 self.id。
- data 类型不声明 check。检查执行器不根据字段、集合、记录引用或维度关联自动遍历其他值。
- 检查 data 内容或关联记录的业务条件时，在记录 check 中显式读取、调用普通函数或编写循环。
- check 名称用于选择检查和定位报告，不是普通调用目标。
- check 不接受用户参数、不返回业务值，也不能被 CFD 覆盖或作为值传递。
- check 本体不能使用 return 或可选传播；它调用的普通函数遵循自身返回规则。

一次检查请求可以选择记录范围，检查入口对范围内的记录执行对应规则。
记录字段里包含 data、集合、其他记录或维度关联，不会因此自动增加检查目标。
顶层 check 保留，用于显式查询记录和表达跨记录规则；自动选取记录范围与递归检查字段结构是两件事。

## 2. require

`require` 是 `Coflow::Check` 提供的普通 Host 函数，使用前导入或写全限定名。
函数签名为 `fn(condition: bool, message: string) -> ()`，调用写作 `require(condition, message)`。

```cft
use Coflow::Check::require;

table Item {
  price: int;

  check priceValid {
    require(self.price >= 0, f"价格不能为负：{self.price}");
  }
}
```

- condition 必须为 bool。
- 检查入口使用的 require 实现在 condition 为 false 时向本次报告记录诊断，正常返回后继续执行。
- condition 和 message 按普通调用规则从左到右立即求值，message 必须产生 string。
- condition 为 true 时仍会求值 message；任一参数求值失败时，本次 Host 调用不发生。
- require 失败不提供类型收窄或可选解包保证。
- require 可以作为普通函数值保存、传递和返回，也可以在普通函数及闭包中调用。
- require 的报告接收由宿主绑定实现负责，调用它的函数和闭包遵循普通生命周期规则。

check 调用普通函数时，函数内部的 require 通过同一 Host 绑定提交报告。
普通入口调用 require 同样执行所绑定的宿主实现；语言和 VM 不因调用位置在 check 之外而拒绝调用。
Host 调用、函数值绑定和错误处理见[Host 函数](./12-Host函数.md)。

## 3. 调用普通函数

check 可以调用任何函数，包括 CFD 实现、回调和 Host 函数。
需要记录的函数通过参数显式接收记录，不隐式继承 check 的当前对象。

```cft
use Coflow::Check::require;
use Coflow::Check::records;

table Item {
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

- Type 是静态 table 或 singleton 类型。
- 返回 `[Type]`，包含该类型及子类型的记录，不包含 data 对象。
- 按实际类型限定名、record key 稳定排序。
- 记录的 id 是 string；记录自身的 check 可以直接使用 self.id，显式取得的记录使用 record.id。
- 数组单绑定读取元素，双绑定读取索引和值；字典必须双绑定，按原始 CFT/CFD 声明顺序读取 key 和 value。

维度值通过 `.for(variant)` 显式选择，或通过 `.variants()` 返回的名称到有效值字典显式遍历。
一条 check 不会因为读取了维度字段而自动切换变体、重复执行。

## 5. 执行错误和预算

check 按语句顺序执行，局部变量与普通函数一致，不按根语句分别调度。

| 情况 | 行为 |
| --- | --- |
| require 的宿主实现记录问题并正常返回 | 继续当前 check |
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
