# 提案：控制流表达式

本提案覆盖尚未进入核心版本的表达式级控制流。

## 候选能力

1. `if`表达式
2. `match`表达式
3. `is`类型判断与收窄
4. `not in`

## 示例

```coflow
var state = if hp <= 0 {
  "dead"
} else {
  "alive"
}
```

```coflow
var label = match rarity {
  Rarity.common => "Common",
  Rarity.rare => "Rare",
  _ => "Unknown",
}
```

## 待解决问题

1. block最后表达式规则是否只用于表达式上下文。
2. `match`是否要求穷尽检查。
3. `is`收窄是否只支持简单变量。
4. `not in`是否作为独立语法，还是使用`not (x in y)`。
