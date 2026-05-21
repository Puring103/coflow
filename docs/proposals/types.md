# P01 提案：联合类型与可空类型

核心版本保持简单动态类型。

## 动机

coflow 已经有 `null`、`?.`、`?[]` 和 `??`，但类型层没有表达“这个值可以为空”的方式，只能退回到 `any`。这会让 `any` 同时承担“动态未知”和“可为空”两种语义，降低配置校验和编辑器补全的价值。

本提案把可空类型定义为联合类型的一个特例：

```coflow
var name: string | null = null;
var target: Player | null = find_player(id);
```

并提供可空语法糖：

```coflow
var name: string? = null;      # 等价于 string | null
var target: Player? = null;    # 等价于 Player | null
```

## 语法

联合类型：

```coflow
var id: int | string
var maybe_name: string | null
```

可空类型：

```coflow
var name: string?
var weapon: Weapon?
```

等价关系：

```coflow
T? == T | null
```

联合类型可以出现在变量、参数、返回值、class 字段和配置类型标注中：

```coflow
fn find(id: string) -> Weapon? {
  return weapons?[id];
}

class Drop {
  item: Item?;
}
```

## 语义

- `T | U` 表示值可以是 `T` 或 `U` 中任意一种类型。
- `T?` 只是 `T | null` 的语法糖，不引入独立类型。
- `null` 只能赋给 `null`、`any`、或包含 `null` 的联合类型。
- `T | T` 归一化为 `T`。
- 联合类型顺序不重要：`int | string` 与 `string | int` 是同一类型。
- 推荐把 `null` 放在最后：`string | null`，但格式化器可以统一改写为 `string?`。

## 与 `any` 的关系

`any` 仍表示完全动态值。`T?` 表示“类型已知，但允许为空”。

```coflow
var a: any = null;       # 合法，但失去静态信息
var b: string? = null;   # 合法，保留 string 信息
var c: string = null;    # 错误
```

若联合中包含 `any`，整体归一化为 `any`：

```coflow
any | null    == any
any | string  == any
```

## 与可选访问的关系

可选访问产生可空结果：

```coflow
var name: string? = player?.name;
var score: int? = scores?["alice"];
```

`??` 可用于从可空类型中取出非空默认值：

```coflow
var display: string = player?.name ?? "unknown";
```

## 实现成本

中等。

- Parser：类型表达式中增加 `|` 和后缀 `?`。
- AST/HIR：类型节点增加 `Union(Vec<Ty>)` 或 `Nullable(Box<Ty>)`，建议 HIR 只保留归一化后的 `Union`。
- Sema：赋值兼容、函数调用、返回值、class 字段校验需要识别联合类型。
- Config：配置类型校验需要按联合分支尝试匹配。

## 当前约束与分期

核心版本没有泛型，因此Iterator协议不写成`Iterator<T>`。

第一期建议只支持：

1. `T | null`
2. `T?`
3. 少量基础联合：`int | string`
4. `is` / `is not` 对联合类型做类型收窄

暂缓：

1. 带结构的代数数据类型。
2. 复杂联合的穷尽性分析。
3. 参数名属于函数类型的一部分。

## 待解决问题

1. `T | U` 是否允许包含 class 类型、enum 类型和函数类型。
2. 联合类型上的字段访问是否必须先经 `is` 收窄。
3. `match` 是否对 enum 联合或字面量联合做穷尽检查。
4. `any` 和结构化对象之间的静态诊断边界。

