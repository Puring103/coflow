# P07 提案：类 LINQ 标准库

## 动机

配置生成、敌人筛选、掉落表计算和 UI 数据准备都需要集合流水线。C# LINQ 的核心价值不是 query syntax，而是统一的集合算子和惰性组合模型。

coflow 已有 Iterator 和 `iter fn`，因此更适合先把 LINQ 能力做成标准库，而不是引入完整查询语法。

## 建议方向

第一期作为标准库 API，不加入核心语法：

```coflow
var ids = enemies
  .where(fn(e) => e.hp > 0)
  .order_by(fn(e) => e.threat, desc: true)
  .map(fn(e) => e.id)
  .to_array();
```

也可以提供函数式版本，便于未来和管道操作符组合：

```coflow
var ids = enemies
  |> where(fn(e) => e.hp > 0)
  |> order_by(fn(e) => e.threat, desc: true)
  |> map(fn(e) => e.id)
  |> to_array();
```

管道操作符不属于本提案第一期目标。

## 标准库算子候选

惰性算子：

```coflow
map(fn(value) -> any)
where(fn(value) -> bool)
filter(fn(value) -> bool)      # where 的别名是否保留待定
flat_map(fn(value) -> iterable)
take(n: int)
skip(n: int)
take_while(fn(value) -> bool)
skip_while(fn(value) -> bool)
order_by(fn(value) -> any, desc: bool = false)
then_by(fn(value) -> any, desc: bool = false)
distinct()
group_by(fn(value) -> any)
```

终止算子：

```coflow
to_array()
to_dict(fn(value) -> key, fn(value) -> val)
first()
first_or_null()
any(fn(value) -> bool = null)
all(fn(value) -> bool)
count(fn(value) -> bool = null)
sum(fn(value) -> number)
min(fn(value) -> any = null)
max(fn(value) -> any = null)
```

## 语义

- 默认惰性：`map`、`where`、`take` 等返回 Iterator。
- 终止算子立即消费 Iterator。
- `order_by` 需要收集全部元素后排序，因此不是纯惰性算子，应在文档中标记。
- 字典迭代仍产出 `{ key, value }` entry 对象。
- 算子回调中的异常向调用方传播。

## 为什么不直接搬 C# query syntax

暂不建议加入：

```coflow
from e in enemies
where e.hp > 0
orderby e.threat descending
select e.id
```

原因：

1. 会引入大量新关键字和解析规则。
2. 与 coflow 显式分号和块结构风格不一致。
3. 标准库 API 已能覆盖大多数需求。
4. query syntax 应在 API 形态被实际验证后再考虑。

## 未来可能的 query 块

如果链式 API 被证明高频且冗长，可以后续加入语法糖：

```coflow
var result = query enemies as e {
  where e.hp > 0;
  order_by e.threat desc;
  select e.id;
};
```

该语法可以降级为标准库调用，但不作为第一期目标。

## 实现成本

第一期中等，主要在运行时和标准库：

- Runtime：Iterator 适配器。
- Stdlib：集合算子实现。
- Sema：方法存在性和回调参数检查可先保持动态。
- Parser：无变化。

## 开放问题

1. 算子作为方法、全局函数，还是两者都提供。
2. `where` 和 `filter` 是否二选一，避免同义词过多。
3. 排序比较规则如何定义，尤其是跨类型值。
4. `group_by` 返回字典还是 entry 数组。
