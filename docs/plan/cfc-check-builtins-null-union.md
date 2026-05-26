# CFC check builtins, null, and union design

## 背景

CFC 当前定位是纯数据配置语言，`check` 块用于表达加载后的数据约束。现有表达式已经支持字段访问、索引、算术、比较、逻辑短路和 `all` 量词，但缺少常见集合校验能力，例如集合长度、唯一性、聚合和安全查 key。

本方案目标是补齐高频配置校验能力，同时避免把 `.cfc` 扩展成通用脚本语言。第一阶段能力应限制在 check-only、纯函数、无副作用、可静态诊断的范围内。

## 设计原则

1. CFC 仍是纯数据语言，不引入用户自定义函数、变量声明、赋值或控制流。
2. 新增集合能力优先作为 `check` 表达式内建，而不是完整标准库。
3. 聚合函数只处理明确、无歧义的集合类型；遇到空集合、类型不匹配或溢出时报告 check eval error。
4. `null` 是显式值，不等价于字段缺失。
5. union 使用名义类型语义，不引入 TypeScript 式结构类型。
6. 完整 union 优先采用显式分支类型和 `is` 窄化，避免裸 union 的对象字面量歧义。

## Check-only builtins

第一批建议加入以下内建：

```cfc
len(value) -> int
contains(collection, value) -> bool
unique(array) -> bool
min(array) -> value
max(array) -> value
sum(array) -> int | float
```

示例：

```cfc
type LootTable {
  drops: [Drop];
  weights: [int];
  tags: [string];
  scores: {string: int};

  check {
    len(drops) <= 10;
    unique(tags);
    sum(weights) == 100;
    min(weights) >= 0;
    max(weights) <= 100;
    contains(scores, "boss") && scores["boss"] > 0;
  }
}
```

### `len`

`len(value)` 返回集合大小。

第一版支持：

- `[T]`
- `{K: V}`

第一版不支持 `string`。字符串长度涉及 UTF-8 字节数、Unicode scalar、grapheme cluster 等语义选择，暂不放入核心校验能力。

### `contains`

`contains(collection, value)` 判断集合是否包含目标值。

规则：

- 对数组，判断元素是否等于 `value`。
- 对 dict，判断 key 是否等于 `value`。
- dict 不检查 value，避免 `contains` 语义歧义。

`contains` 与 `&&` 短路配合，可以安全访问 dict：

```cfc
check {
  contains(scores, "alice") && scores["alice"] >= 0;
}
```

### `unique`

`unique(array)` 判断数组元素是否全部唯一。

第一版支持元素类型：

- `int`
- `bool`
- `string`
- 同一 enum 类型

第一版不支持 `float`、object、array、dict：

- `float` 涉及 NaN、-0.0、精度等相等语义。
- object 需要决定按 identity 还是按结构比较。
- array/dict 的深比较成本和错误语义更复杂。

### `min` / `max`

`min(array)` 和 `max(array)` 返回数组中的最小/最大元素。

第一版支持：

- `[int]`
- `[float]`
- 同一 enum 类型数组

空数组报告 eval error：

```text
min() requires a non-empty array
```

不直接支持 dict。需要先用 `keys(dict)` / `values(dict)` 转成数组后再组合：

```cfc
min(values(scores)) >= 0;
```

`keys` / `values` 已纳入当前实现。

### `sum`

`sum(array)` 对数字数组求和。

规则：

- `[int] -> int`
- `[float] -> float`
- 整数溢出报告 eval error
- 空数组返回 `0`

空数组返回 `0` 符合聚合恒等元直觉，但需要在 spec 中明确，避免与 `min/max` 的空集合错误混淆。

## 量词扩展

现有 `all` 只能表达“全部满足”。建议补充：

```cfc
any item in collection {
  ...
}

none item in collection {
  ...
}
```

示例：

```cfc
check {
  any drop in drops {
    drop.rarity >= Rarity.rare;
  }

  none tag in tags {
    tag == "";
  }
}
```

规则：

- `any` 对空集合为 false。
- `none` 对空集合为 true。
- 与 `all` 一样支持 array 和 dict。
- dict 迭代绑定 entry 对象，包含 `.key` 和 `.value`。

`none` 可以被视为 `!any` 的语法糖，但由于当前量词是语句而不是表达式，保留独立语法更直接。

## `null` 与 nullable 类型

建议引入 `null` 作为显式值，并先支持 nullable 类型：

```cfc
type Drop {
  item: Item | null = null;
}
```

语义：

1. 字段缺失仍然是结构错误。
2. `null` 是显式值，只有类型允许 `null` 时才能填入。
3. `any` 可以接受 `null`。
4. `null == null` 为 true，`null != value` 为 true。
5. 对 `null` 做字段访问、索引访问、大小比较、算术和聚合都报告 eval error。
6. 通过逻辑短路表达安全访问：

```cfc
check {
  item != null && item.id != "";
}
```

### Nullable 语法

推荐标准语法：

```cfc
Item | null
```

可选后续语法糖：

```cfc
Item?
```

当前实现支持 `T | null`，也支持 `T?` 语法糖。

## Union 设计方向

完整裸 union 暂缓，不建议第一阶段支持：

```cfc
reward: Item | Currency | Exp;
```

裸 union 的主要问题是对象字面量分支消歧：

```cfc
reward: Item | Currency = {
  id: "coin",
  value: 10,
};
```

如果多个分支都匹配，或者多个分支都失败，错误信息和实际类型选择都会变复杂。

推荐路线是 TypeScript 风格表面语法，但保持 CFC 名义类型：

```cfc
type ItemReward {
  item: Item;
}

type CurrencyReward {
  amount: int;
}

type ExpReward {
  value: int;
}

type Reward = ItemReward | CurrencyReward | ExpReward;
```

这里 `Reward` 是 union alias，不是普通 object type。分支必须是已命名 `type`，第一版不支持匿名 object union。

### 显式分支对象

union 配置值使用显式分支类型，而不是污染配置字段：

```cfc
reward: Reward = CurrencyReward {
  amount: 100,
};
```

导入类型同样显式带上模块别名：

```cfc
reward: lib.Reward = lib.CurrencyReward {
  amount: 100,
};
```

如果已有命名节点的实际 nominal type 是 union 分支之一，也可以直接赋给 union：

```cfc
coin: CurrencyReward = { amount: 10 };
reward: Reward = coin;
```

普通对象字面量直接赋给 union 会报错：

```cfc
reward: Reward = {
  amount: 100,
};
```

错误语义是“union object must specify branch type”。不做按字段集合推断分支，因为这会把分支选择变成隐式结构匹配，和 CFC 的名义类型方向冲突。

## Literal types

string literal type 仍然是独立类型能力，可用于普通字段约束，但不再作为 union 内置 discriminator：

```cfc
type CurrencyReward {
  category: "currency" = "currency";
  amount: int;
}
```

当前实现支持 string、int、bool literal type，后续如有需要再评估 float literal type。

规则：

- 字段类型 `"currency"` 只接受字符串值 `"currency"`。
- 默认值可省略时，如果字段有 literal type 且没有默认值，仍按普通必填字段处理。
- 不要求 union 分支拥有同名 literal 字段。

## `is` 类型判断与窄化

`is` 用于 check 表达式中的类型判断：

```cfc
check {
  reward is CurrencyReward;
  reward is CurrencyReward && reward.amount > 0;
  item is null;
}
```

规则：

- `expr is TypeName` 判断实际 nominal type 是否为 `TypeName`。
- `expr is null` 判断值是否为 `null`。
- 对 union 值，`is` 判断当前分支 payload 的 nominal type。
- `is` 不做结构匹配。

### 窄化

在 `&&` 右侧可以使用左侧 `is` 的窄化结果：

```cfc
reward is CurrencyReward && reward.amount > 0;
```

这里右侧允许把 `reward` 视为 `CurrencyReward`。第一版只需要支持最常见的局部窄化形式：

- `name is Type && name.field ...`
- `name != null && name.field ...`
- 括号不改变窄化语义：

```cfc
(reward is CurrencyReward) && reward.amount > 0;
```

不建议第一版支持复杂控制流窄化、跨语句窄化或 `||` 分支合并。

## Value model 影响

`null` 需要新增 value 变体：

```rust
CfcValue::Null
```

union alias 可以有两种实现路线。

### 路线 A：不新增 union value（已废弃）

构建 `Reward` 时，实际保存分支 object：

```rust
CfcValue::Object {
  type_name: Some(CurrencyReward),
  fields: ...
}
```

优点：

- value model 改动小。
- 宿主 API 直接看到真实分支类型。

缺点：

- 无法直接知道某个字段声明的是 `Reward`，只能看实际值。
- 如果未来需要保留 union alias 信息，需要额外 metadata。

### 路线 B：新增 union wrapper（当前实现）

```rust
CfcValue::Union {
  union_type: CfcNominalType,
  value: CfcValueRef,
}
```

优点：

- 宿主可以明确知道这是一个 union 字段。
- 后续序列化、编辑器提示和诊断更直接。

缺点：

- 对象图遍历、check、API 访问都要识别 wrapper。
- identity 和默认值复制规则更复杂。

当前实现采用路线 B：union alias 在运行时保存 wrapper metadata，同时字段访问、索引、check 遍历和 `is` 判断对 wrapper 透明访问内部实际值。

## 分阶段计划

### Phase 1：check builtins 和量词

- `len`
- `contains`
- `unique`
- `min`
- `max`
- `sum`
- `keys`
- `values`
- `any`
- `none`

这一阶段不改变数据 value model。

### Phase 2：`null` 和 nullable

- `null` token/value
- `T | null` 类型
- `T?` 语法糖
- check 中 `is null`
- `!= null && ...` 短路安全访问

这一阶段需要新增 `CfcValue::Null`，并更新类型匹配、默认值复制、value signature、check 运行时。

### Phase 3：nominal union alias

- `type Reward = A | B | C;`
- string/int/bool literal type
- 显式分支对象：`reward: Reward = CurrencyReward { ... };`
- `expr is TypeName`
- union wrapper metadata

这一阶段暂不支持匿名 object union、完整模式匹配和跨语句窄化。

当前实现已覆盖 `type Reward = A | B | C;`、string/int/bool literal type、显式分支对象、union wrapper metadata、`null` / nullable，以及 check 中的 `expr is TypeName` / `expr is UnionAlias` / `expr is null`。

## 开放问题

1. `sum([])` 是否固定返回 `0`，还是为了发现错误也报 eval error。
2. `unique([float])` 是否永远不支持，还是后续定义严格浮点相等规则。
3. 是否允许 `Reward.CurrencyReward { ... }` 这类命名空间式分支构造语法。
4. object 字面量构建 union 时，是否永远禁止按唯一可匹配分支推断。
5. `is` 窄化是否只在 check 表达式内生效，还是未来也服务 `.cfs`。
