# Check 校验

当前版本支持声明和保存 check 源码，尚未实现函数编译、虚拟机及 check 执行。
项目存在检查规则时，执行检查返回 `EXEC-001`，不会把未执行的规则报告为通过。
声明检查、静态数据加载和代码生成可独立使用。

check 可以定义在 table、singleton 内或作为命名顶层规则：

```cft
table Item {
  price: int;
  check {
    validator.require(self.price >= 0, "价格不能为负数");
  }
}

@Host
singleton validator {
  require: fn(condition: bool, message: string) -> ();
}
```

记录规则作用于记录，不会沿字段、集合或引用自动遍历内联对象；data 不定义 check。
`self.id` 可读取记录身份。

`require` 是宿主提供的普通函数，参数按普通调用立即求值。
Host 声明与绑定见 [C# 代码生成](../07-codegen/01-csharp.md#host-服务)。
