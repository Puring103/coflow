# coflow核心运行时

核心运行时围绕函数调用，模块初始化，Iterator协议，`for in`和`co fn`展开。

## 模块阶段

模块有三个阶段：

1. 声明收集。
2. 配置常量求值和配置校验。
3. 运行时模块变量初始化。

顶层配置不能依赖顶层`var`。

```coflow
var scale = host.get_scale()

damage = scale * 10 // 错误
```

顶层`var`在运行时模块初始化阶段初始化。

```coflow
var cache = host.create_cache()
```

顶层禁止普通运行时语句。运行时代码从宿主调用公开函数开始。

```coflow
fn main() {
  print("start")
}
```

## Iterator协议

核心版本只有一个迭代协议。

Iterator对象提供`next()`方法。

```coflow
var step = it.next()
```

`next()`返回对象：

```coflow
{
  done: bool,
  value: any,
}
```

当`done`为`false`时，`value`是本次产出的值。

当`done`为`true`时，迭代结束，`value`固定为`null`。

已完成的Iterator再次`next()`仍返回：

```coflow
{ done: true, value: null }
```

## iter

`for in`使用内建`iter(value)`获取Iterator。

规则：

1. 如果`value`已经是Iterator，直接返回。
2. 如果`value`是数组，返回数组Iterator。
3. 如果`value`是字典，返回字典Iterator。
4. 否则运行时报错；静态可确定时提前诊断。

## for in

```coflow
for item in items {
  print(item)
}
```

语义等价于：

```coflow
var it = iter(items)

while true {
  var step = it.next()
  if step.done {
    break
  }

  var item = step.value
  print(item)
}
```

字典默认迭代entry对象。

```coflow
for entry in scores {
  print(entry.key)
  print(entry.value)
}
```

核心版本不支持`for key, value in dict`。该语法放入解构提案。

## 标准库range

核心语言不内置范围语法。标准库提供`range`函数，返回可迭代值。

```coflow
range(0, 10)
range(1, 11)
```

`range(start, end)`表示从`start`到`end`的左闭右开整数序列。

`range`不生成数组，按需迭代。

```coflow
for i in range(0, 10) {
  print(i)
}
```

## co fn

`co fn`是Iterator工厂。

```coflow
co fn counter() {
  yield 1
  yield 2
}
```

调用`co fn`不会立即执行函数体，而是返回Iterator。

```coflow
var c = counter()

c.next() // { done: false, value: 1 }
c.next() // { done: false, value: 2 }
c.next() // { done: true, value: null }
```

`yield value`产出一个值。

```coflow
yield 1
```

`co fn`中禁止使用`return`。

自然执行到函数末尾表示迭代结束。

提前结束使用`yield break`。

```coflow
co fn numbers(limit) {
  var i = 0

  while true {
    if i >= limit {
      yield break
    }

    yield i
    i += 1
  }
}
```

`yield break`是特殊控制流语法，只能出现在`co fn`中。

## 自动委托

如果`yield`的值是Iterator或可迭代值，则自动委托。

```coflow
co fn child() {
  yield 1
  yield 2
}

co fn parent() {
  yield child()
  yield 3
}
```

`parent()`依次产出：

```coflow
1
2
3
```

委托只转发子Iterator产出的值。子Iterator结束时的`done`结果不向外产出。

因为核心版本中`yield iterator`自动委托，所以不能直接把Iterator对象作为普通yield值产出。若未来需要该能力，放入提案。

## 错误

Iterator或`co fn`执行期间抛错时，错误从`next()`传播。

异常终止后的Iterator进入dead状态。dead状态再次`next()`抛运行时错误。
