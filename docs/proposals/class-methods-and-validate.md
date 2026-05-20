# 提案：class方法

核心版本的`class`声明字段结构和`validate`块。

`validate`已作为关键字进入核心版本（见`1-core-types.md`）。本提案只讨论class方法。

## class方法

```coflow
class Player {
  hp: int

  fn damage(self, amount: int) {
    self.hp -= amount
  }
}
```

## 待解决问题

1. class方法是否允许访问模块顶层变量。
2. 是否支持隐式`self`。
3. 方法调用是否支持链式调用。
4. 配置对象（只读）上调用方法的语义。

