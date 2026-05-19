# 提案：class方法与validate

核心版本的`class`只声明字段结构。

本提案讨论class方法和配置校验钩子。

## class方法

```coflow
class Player {
  hp: int

  fn damage(self, amount: int) {
    self.hp -= amount
  }
}
```

## validate

```coflow
class Range {
  min: int
  max: int

  fn validate(self) {
    if self.min > self.max {
      return "min must be <= max"
    }

    return null
  }
}
```

## 当前倾向

`validate`暂时不做纯度限制，由用户自行保证。

`validate`返回错误或抛错都应归为加载期配置诊断，不通过脚本`try catch`捕获。

## 待解决问题

1. `validate`是否允许调用宿主API。
2. `validate`是否允许修改外部状态。
3. `validate`错误返回值的标准格式。
4. class方法是否允许访问模块顶层变量。
5. 是否支持隐式`self`。

