# Check 校验

check 在 Runtime 构建成功后通过显式请求执行。每次请求实际运行选中的记录规则和顶层规则，
返回业务诊断、完成状态及执行统计；构建、数据加载和代码生成不会隐式执行 check。

check 可以定义在 table、singleton 内或作为命名顶层规则：

```cft
table Item {
  price: int;
  check {
    Coflow::Check::require(self.price >= 0, "价格不能为负数");
  }
}
```

记录规则作用于记录，不会沿字段、集合或引用自动遍历内联对象；data 不定义 check。
`self.id` 可读取记录身份。

`Coflow::Check::require` 按普通调用规则立即求值。条件为 false 时记录消息并继续当前规则；
执行错误终止当前规则，后续规则继续执行。C# 通过 `Runtime.RunChecks(CheckOptions)` 选择记录、
规则名称、全局规则和执行预算。
