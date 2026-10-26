# 必填、默认值与 Option

先分别回答两个业务问题：允许没有值吗？允许作者不填写吗？可空使用 Option，可省略需要显式默认值。

| 声明 | 可省略 | 可填写 |
| --- | --- | --- |
| `count: int;` | 否 | 整数 |
| `count: int = 0;` | 是，补 0 | 整数 |
| `note: Option<string>;` | 否 | None、Some("备注") 或 "备注" |
| `note: Option<string> = None;` | 是，补 None | 同上 |
| `nested: Option<Option<int>>;` | 否 | None、Some(None)、Some(Some(3)) |

## 默认值选择

默认值应表达真实业务语义，例如空标签列表、明确的通用倍率。关键价格、标识或必选引用无合理默认时保持必填，由数据补全，避免用 0 或空值掩盖缺失。

默认值递归补全到内联对象；即使字段是 Option、数组或字典，也不会因为类型本身看起来可空而自动允许省略。

```cft
data Stats { hp: int = 100; }
table Item {
  name: string;
  stats: Stats = Stats {};
  tags: [string] = [];
  note: Option<string> = None;
}
```

这一声明允许 CFD 只填 name。若移除 stats 的默认值，CFD 必须提供 stats，对象内部 hp 仍可省略。

## 改默认值的影响

新增必填字段需要在同一候选补齐所有受影响记录和对象。修改已有默认值会改变未显式赋值的有效值，还可能影响模板、规则及维度回退；只查显式源码不足以判断影响。

用 data get --view both 区分 explicit_base、schema_default、dimension_override。删除显式赋值会重新使用默认值；设为 None 则是显式空态，前提是 Option 类型。

## 安全读取 Option

check 中先用 `is Some(value)` 解包：

```cft
data Reward {
  count: Option<int> = None;
  check Positive {
    when count is Some(value) { value > 0; }
  }
}
```

绑定可用于条件后续的 && 以及成立时的 when 主体，不能在解包作用域外读取。嵌套 Option 逐层处理；CLI 穿过 None 的路径为不可达，修改时设置适当的父值。

递归 data 通过 Option 或集合提供终止点，例如 `children: [Node] = [];`。默认值展开必须有限，必填内联对象不能构成无终点的循环。
