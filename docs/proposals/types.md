# 提案：类型系统扩展

核心版本保持简单动态类型。

## 候选能力

联合类型：

```coflow
var id: int | string
```

函数类型：

```coflow
var f: fn(int, int)
```

带参数名的函数类型：

```coflow
var f: fn(x: int, y: int)
```

可空语法糖：

```coflow
var name: string?
```

## 当前约束

核心版本没有泛型，因此Iterator协议不写成`Iterator<T>`。

## 待解决问题

1. 是否支持联合类型。
2. 是否需要函数类型。
3. 参数名是否属于函数类型。
4. 是否需要可空语法糖。
5. `any`和结构化对象之间的静态诊断边界。

