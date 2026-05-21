# 提案：命名参数与默认参数

核心版本只支持位置参数。

## 候选能力

```coflow
fn spawn(id: string, x: float = 0, y: float = 0) {
  host.spawn(id, x, y)
}

spawn("slime")
spawn("slime", y: 10)
spawn(id: "slime", x: 1, y: 2)
```

## 待解决问题

1. 参数名是否是函数类型的一部分。
2. 默认值是否跟随函数对象。
3. 默认值是否必须是配置常量表达式。
4. 位置参数和命名参数混用规则。
5. 宿主绑定函数如何暴露参数名。

