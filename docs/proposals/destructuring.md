# 提案：解构

本提案覆盖数组，对象和迭代解构。

## 候选能力

```coflow
var [x, y] = point
var [head, ...tail] = items
var { id, damage } = weapon
var { x: px, y: py } = position
```

字典entry解构：

```coflow
for { key, value } in scores {
  print(key)
  print(value)
}
```

也可以讨论简写：

```coflow
for key, value in scores {
  print(key)
  print(value)
}
```

## 待解决问题

1. 是否支持嵌套解构。
2. 解构失败时返回`null`，诊断，还是运行时错误。
3. `...rest`是否只允许数组解构末尾。
4. 解构是否只用于绑定，还是也用于赋值。

