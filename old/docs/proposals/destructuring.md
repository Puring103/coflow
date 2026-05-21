# P05 提案：解构绑定与解构赋值

本提案覆盖数组、对象、字典 entry 和迭代位置的解构。

## 动机

coflow 的配置和脚本会频繁处理对象、数组、字典 entry。没有解构时，小型数据拆包会产生大量临时变量和重复字段访问。

## 语法

```coflow
var [x, y] = point;
var [head, ...tail] = items;
var { id, damage } = weapon;
var { x: px, y: py } = position;
```

字典entry解构：

```coflow
for { key, value } in scores {
  print(key);
  print(value);
}
```

也可以讨论简写：

```coflow
for key, value in scores {
  print(key);
  print(value);
}
```

## 建议分期

第一期只支持“绑定位置”的解构：

```coflow
var { id, damage } = weapon;
for { key, value } in scores {
  print(key);
}
```

暂不支持普通赋值目标解构：

```coflow
[x, y] = point;      # 暂缓
{ id } = weapon;     # 暂缓
```

## 语义

- 数组解构按索引读取。
- 对象解构按字段名读取。
- 解构失败是运行时错误；若静态类型可证明失败，则提前诊断。
- `...rest` 只允许出现在数组解构末尾。
- 对象简写 `{ id }` 等价于 `{ id: id }`。
- 解构声明引入的变量遵守普通 `var` 的作用域和重复声明规则。

## 与推导式的关系

若本提案进入核心，数组 / 字典推导式可以使用解构绑定：

```coflow
var keys = [key for { key, value } in scores];
```

## 实现成本

中等。

- AST：新增 `Pattern`，用于 `var`、`for in`、未来 `match`。
- Parser：`var` 和 `for in` 的绑定名从 `Ident` 扩展为 `Pattern`。
- Sema：模式绑定、重复声明、类型检查。
- Runtime：数组长度、字段缺失、字典 entry 形状的错误诊断。

## 待解决问题

1. 是否支持嵌套解构。
2. 解构失败时返回`null`，诊断，还是运行时错误。
3. `...rest`是否只允许数组解构末尾。
4. 解构是否只用于绑定，还是也用于赋值。
5. `for key, value in dict` 是否作为字典 entry 解构的专用简写。

