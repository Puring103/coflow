# Check 校验

CFT 的 `check` 块用于声明配置数据必须满足的业务规则。`coflow check` 和 `coflow build` 会在字段值、默认值和记录引用准备完成后执行这些规则。

```cft
type Monster {
  level: int;
  tags: [string] = [];

  check {
    1 <= level <= 100;
    tags.isUnique();
  }
}
```

`check` 必须位于类型的所有字段之后，一个类型只能有一个 `check` 块。父类型的规则也会应用到子类型实例，并且按继承链从根类型到实际类型依次执行。

## 执行时机与产物边界

`check` 在字段值和默认值构建完成、记录引用解析完成后执行。`coflow check`、`coflow build`、
`coflow export` 以及加载完整项目数据的 `coflow data` 查询和修改命令都会运行相关规则；
只编译 schema 的 `coflow cft check`、`coflow schema` 和 `coflow codegen` 不执行数据规则。

校验失败会作为诊断返回，并阻止 `build` 或 `export` 发布新产物。`check` 块本身不会写入导出数据，
也不会生成游戏运行时代码；运行时消费者读取的是已经通过 Coflow 校验的产物。

## 可用值

表达式可以读取当前对象及继承字段、虚拟 `id`、`const` 常量、enum 值、已解析引用对象的字段，以及量词声明的局部变量。

```cft
const MAX_LEVEL = 100;

type Monster {
  level: int;
  next: &Monster? = null;

  check {
    id != "";
    level <= MAX_LEVEL;
    next == null || next.level >= level;
  }
}
```

## 条件语句

普通校验语句是一个结果为 `bool` 的表达式，以 `;` 结束。某条条件失败后，Coflow 会继续执行其他独立语句，以便一次报告多个问题。

```cft
check {
  damage > 0;
  cooldown >= 0.1;
  0 < level <= 100;
}
```

条件后可以使用 `: message` 指定诊断消息。条件结果为 `false` 时，自定义消息完全替换自动生成的解释；错误码、严重级别、数据和 schema 位置、related locations、check/量词/dimension 上下文仍保留为结构化诊断字段。类型错误、空值访问、越界、无效数值和预算耗尽等求值错误不会被自定义消息覆盖。

```cft
check {
  level > 0: "等级必须大于 0";
  damage >= 0:
    f"怪物 {id} 的伤害不能为负数，当前为 {damage}";
}
```

格式化字符串使用 `f"...{expression}..."`，可以作为普通字符串表达式或诊断消息。插值支持 `null`、bool、int、float、string 和 enum；集合、object 和 record ref 不能直接插值。`&#123;&#123;` 和 `&#125;&#125;` 分别表示两个花括号转义，结果为字面量 `{` 和 `}`。消息只在条件失败时求值。

## 运算符

| 类别 | 运算符 | 支持的值 |
| --- | --- | --- |
| 逻辑 | `!`、`&&`、`||` | `bool` |
| 相等 | `==`、`!=` | 相同或兼容类型 |
| 顺序比较 | `<`、`<=`、`>`、`>=` | `int`、`float`、同一 enum、`string` |
| 算术 | `+`、`-`、`*`、`/`、`**` | 两侧同为 `int` 或同为 `float` |
| 整数除法和余数 | `//`、`%` | `int` |
| 移位 | `<<`、`>>` | `int` |
| 位运算 | `&`、`|`、`^`、`~` | `int` 或同一个 `@flag` enum |
| 类型判断 | `is TypeName`、`is null` | 对象或 nullable 值 |

`//` 是整数除法，不是注释。CFT 注释使用 `#`。

比较可以连续书写：

```cft
check { 0 <= level <= 100; }
```

enum 不会和整数隐式转换。需要构造没有具名变体的 enum 值时，使用 `EnumName(integer)`：

```cft
check {
  (permissions & Permission.Read) != Permission(0);
}
```

### 优先级

从高到低：

1. 字段访问、索引和调用：`.field`、`?.field`、`[index]`、`?[index]`、`()`
2. 一元运算：`!`、`~`、`-`
3. 幂：`**`，从右向左结合
4. 乘除：`*`、`/`、`//`、`%`
5. 加减和移位：`+`、`-`、`<<`、`>>`
6. 位运算：`&`、`^`、`|`
7. 比较：`==`、`!=`、`<`、`<=`、`>`、`>=`
8. 类型判断：`is`
9. 逻辑与：`&&`
10. 逻辑或：`||`
11. null 合并：`??`，从右向左结合

可以使用括号明确计算顺序。

## 字段与索引

对象和记录引用使用 `.field` 访问字段。数组使用整数索引，字典使用与 key 类型一致的索引：

```cft
check {
  stats.hp > 0;
  rewards[0].count > 0;
  weights[DamageType.Fire] > 0;
}
```

访问 nullable 值之前应先排除 `null`。`&&` 和 `||` 支持短路：

```cft
check {
  next == null || next.level > level;
}
```

也可以使用安全访问和 null 合并：

```cft
check {
  (next?.level ?? 0) >= 0;
  (optional_rewards?[0]?.count ?? 0) > 0;
}
```

`?.` 和 `?[...]` 只允许 nullable receiver；receiver 为 `null` 时返回 `null`，否则执行普通访问。它们不会吞掉数组越界、字典 key 缺失、引用失败或其他求值错误。`lhs ?? rhs` 要求 `lhs` 为 nullable、`rhs` 与非 null 的 lhs 类型一致，并且只在 lhs 为 null 时求值 rhs。

## `when` 块

`when` 只在条件为真时执行其中的规则：

```cft
check {
  when !is_passive {
    cooldown != null;
    cooldown > 0.0;
  }
}
```

`when` 条件必须是 `bool`，块内可以继续嵌套 `when` 或量词。

## 集合量词

量词遍历数组或字典：

```cft
check {
  all cost in costs { cost >= 0; }
  any reward in rewards { reward is CurrencyReward; }
  none tag in tags { tag == ""; }
}
```

| 量词 | 含义 | 空集合结果 |
| --- | --- | --- |
| `all x in values { ... }` | 每个元素都满足块内规则 | 通过 |
| `any x in values { ... }` | 至少一个元素满足块内全部规则 | 失败 |
| `none x in values { ... }` | 没有元素满足块内全部规则 | 通过 |

遍历字典时，局部变量具有 `.key` 和 `.value`：

```cft
all entry in resistances {
  0.0 <= entry.value <= 1.0;
}
```

数组和字典还支持两个 binding。数组顺序固定为 `item, index`，字典顺序固定为 `key, value`：

```cft
all reward, index in rewards {
  reward.count > 0: f"第 {index} 个奖励数量无效";
}

all damage_type, weight in weights {
  weight >= 0: f"{damage_type} 的权重不能为负数";
}
```

单 binding 行为保持兼容：数组 binding 是 item，字典 binding 是带 `.key` / `.value` 的 entry。

## 命名顶层 check 与 `records(Type)`

跨记录规则使用命名顶层 check。顶层作用域没有隐式当前记录，因此不能使用裸字段或虚拟 `id`；可以使用 const、enum、量词 binding 和 `records(Type)`：

```cft
check ItemIntegrity {
  records(Item).len() > 0: "项目中至少需要一个物品";

  all item in records(Item) {
    item.price > 0:
      f"物品 {item.id} 的价格必须大于 0";
  }
}
```

`Type` 必须是静态 object type。`records(Base)` 包含实际类型为 `Base` 及其派生类型的所有顶层记录，不包含内联 object；结果按 `(actual_type, record_key)` 稳定排序。该 special form 只能用于命名顶层 check，不能传入字符串或动态类型表达式。

每个顶层 check 在 baseline round 执行一次。读取 dimension 字段的 statement 只在对应 dimension variant round 执行，不相关的顶层规则不会在所有 variant 中重复运行。增量检查同时跟踪实际读取的 record path 和 record-set membership；字段修改只重跑读取路径重叠的根，新增、删除、rename 或派生类型成员变化会使相关 `records(Type)` 规则失效。

## 类型判断

`is` 可以判断多态对象的实际类型，或判断 nullable 值是否为 `null`：

```cft
check {
  reward is CurrencyReward;
  optional_item is null;
}
```

## 内建方法

| 方法 | 适用类型 | 返回值 | 说明 |
| --- | --- | --- | --- |
| `value.len()` | string / array / dict | `int` | Unicode 字符数或元素数量 |
| `value.contains(x)` | string / array / dict | `bool` | 子串、array 元素或 dict key 是否存在 |
| `array.isUnique()` | int、bool、string、enum 数组及其 nullable 形式 | `bool` | 检查元素是否唯一 |
| `array.min()` | int / float / enum array | 元素类型 | 最小值 |
| `array.max()` | int / float / enum array | 元素类型 | 最大值 |
| `array.sum()` | int / float array | 元素类型 | 求和 |
| `dict.keys()` | dict | array | key 数组 |
| `dict.values()` | dict | array | value 数组 |
| `text.matches("pattern")` | string | `bool` | 正则表达式匹配 |
| `text.startsWith(prefix)` / `endsWith(suffix)` | string | `bool` | 前缀或后缀匹配 |
| `text.isBlank()` | string | `bool` | 空字符串或全为 Unicode whitespace |
| `number.abs()` | int / float | 原数值类型 | 绝对值；`i64::MIN` 报求值错误 |
| `value.isFinite()` | float | `bool` | 是否为有限浮点数 |
| `value.approxEqual(other, epsilon)` | float | `bool` | 绝对误差比较，epsilon 必须有限且非负 |
| `dict.containsKey(key)` / `containsValue(value)` | dict | `bool` | key 或 value 是否存在 |
| `array.isSorted()` / `isStrictlySorted()` | 可排序标量数组 | `bool` | 非递减或严格递增 |
| `array.intersects(other)` / `isDisjoint(other)` | 支持集合比较的标量数组 | `bool` | 是否有交集或无交集 |
| `array.isSubsetOf(other)` / `isSupersetOf(other)` | 支持集合比较的标量数组 | `bool` | 数学集合包含关系，重复项不改变结果 |

`matches` 的 pattern 必须是字符串字面量，使用 Rust `regex` 语法。默认执行子串匹配；完整匹配请使用 `^...$`。

本版本不支持 `matches(f"...")` 动态正则模板。需要匹配由字段组成的精确 ID 时，优先使用普通格式化字符串比较，例如 `id == f"{category}_{level}"`。

`min()` 和 `max()` 应在确认数组非空后调用：

```cft
check {
  when scores.len() > 0 {
    scores.min() <= scores.max();
  }
}
```
