# coflow核心运行时

核心运行时围绕函数调用，模块初始化，Iterator协议，`for in`和`iter fn`展开。

## 模块阶段

模块有三个阶段：

1. 声明收集。
2. 配置常量求值和配置校验。
3. 运行时模块变量初始化。

顶层配置不能依赖顶层`var`。

```coflow
var scale = host.get_scale()

damage = scale * 10  # 错误
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
4. 如果`value`是Range，返回Range Iterator。
5. 否则运行时报错；静态可确定时提前诊断。

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

## Range

Range字面量产生可迭代的整数序列。

```coflow
0..10     # [0, 10)，不含10
0..=10    # [0, 10]，含10
```

Range可以直接用于`for in`。

```coflow
for i in 0..10 {
  print(i)
}
```

Range也可以用于成员判断。

```coflow
if hp in 1..=100 {
  # hp在[1, 100]范围内
}
```

## 标准库range

标准库提供`range`函数，与Range字面量互补，支持步长参数。

```coflow
range(0, 10)
range(1, 11)
range(0, 10, 2)
```

`range(start, end)`表示从`start`到`end`的左闭右开整数序列。

`range(start, end, step)`指定步长。`step`默认为1，不能为0。

`range`不生成数组，按需迭代。

```coflow
for i in range(0, 10) {
  print(i)
}

for i in range(0, 10, 2) {
  print(i)  # 0, 2, 4, 6, 8
}
```

## iter fn

`iter fn`是Iterator工厂。

```coflow
iter fn counter() {
  yield 1
  yield 2
}
```

调用`iter fn`不会立即执行函数体，而是返回Iterator。

```coflow
var c = counter()

c.next()  # { done: false, value: 1 }
c.next()  # { done: false, value: 2 }
c.next()  # { done: true, value: null }
```

`yield value`产出一个值。

```coflow
yield 1
```

`iter fn`可以捕获外层作用域的局部变量，遵守与普通函数相同的闭包规则。

```coflow
fn make_sequence(start) {
  var i = start

  return iter fn() {
    while true {
      yield i
      i += 1
    }
  }
}
```

自然执行到函数末尾表示迭代结束。

提前结束使用`return`（不带值）。

```coflow
iter fn numbers(limit) {
  var i = 0

  while true {
    if i >= limit {
      return    # 提前结束迭代
    }

    yield i
    i += 1
  }
}
```

`iter fn`中禁止使用`return value`（带值的return）。

## yield from

`yield from`将子Iterator的所有值委托产出。

```coflow
iter fn child() {
  yield 1
  yield 2
}

iter fn parent() {
  yield from child()
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

`yield value`始终产出`value`本身。如果`value`是Iterator对象，它作为普通值产出，不会自动展开。

```coflow
iter fn wrap() {
  yield child()        # 产出Iterator对象本身
  yield from child()   # 展开子Iterator，产出1, 2
}
```

## 错误

Iterator或`iter fn`执行期间抛错时，错误从`next()`传播。

异常终止后的Iterator进入dead状态。dead状态再次`next()`抛运行时错误。
