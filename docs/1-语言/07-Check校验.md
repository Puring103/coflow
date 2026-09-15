# Check 校验

> 状态：语言设计
>
> 范围：CFT `check` 的声明、执行语义、`Check.require` 和诊断边界。

`check` 是 CFT 契约中的专用校验结构，使用与 CFD 函数一致的函数体语法。它不能被 CFD 覆盖、返回、
保存或作为回调传递。check 执行适用规则、收集结构化诊断，并在一条规则失败后继续执行相互独立的失败
条件。

## 1. 声明

```text
check-block := "check" [ identifier ] "{" function-body "}"
check-decl  := "check" identifier "{" function-body "}"
```

```cft
type Monster {
  level: int;
  tags: [string] = [];

  check {
    require(1 <= level <= 100, "等级必须在 1 到 100 之间");
    require(tags.isUnique(), "标签不能重复");
  }
}
```

```cft
type Inventory {
  items: [int] = [];

  check itemsValid {
    for item in items {
      require(item >= 0, "物品数量不能为负");
    }
  }
}
```

```cft
check ItemIntegrity {
  require(records(Item).len() > 0, "项目中至少需要一个物品");
}
```

- type 内的 `check` 必须位于该类型的所有字段之后，一个类型只能有一个 `check` 块，可带可选名称。
- `check` 的名称只用于诊断和工具展示，不能作为调用目标。
- 顶层命名 check 用于跨记录规则。
- 父类型的规则也会应用到子类型实例，并按继承链从根类型到实际类型依次执行。
- check 不接受参数、不返回值，也不能相互调用。
- 普通函数不能调用 check；check 不能调用普通函数。
- 应用 Host 外部服务不可从 check 访问。

## 2. `Check.require`

```text
require-call := "require" "(" expression "," expression ")"
```

```cft
use std::Check::require;

type Monster {
  level: int;
  damage: int;

  check {
    require(level > 0, "等级必须大于 0");
    require(damage >= 0, "怪物 {id} 的伤害不能为负数");
  }
}
```

- 返回 `()`。
- 使用前需要 `use std::Check::require;`。
- `condition` 为 `true` 时不产生诊断；为 `false` 时记录诊断并继续执行后续语句。
- `message` 只在条件失败时求值。
- `message` 是字符串表达式，可包含格式化字符串插值。
- `require` 失败不提供后续解包或类型收窄保证。
- 普通内置函数遵循正常参数求值规则；短路运算和类型判断仍是语言结构。

## 3. 可用值

check 表达式可以读取：

- 当前对象及继承字段、虚拟 `id`。
- `const` 常量、enum 值。
- 已解析引用对象的字段。
- 函数体内的局部变量和 `for` 绑定。

```cft
const MAX_LEVEL: int = 100;

type Monster {
  level: int;
  next: &Monster? = None;

  check {
    require(level <= MAX_LEVEL, "等级不能超过 {MAX_LEVEL}");
    if next is Some(monster) {
      require(monster.level >= level, "后继怪物等级不能更低");
    }
  }
}
```

## 4. 控制流与集合遍历

check 体使用函数体的 `if`、`match`、`while`、`for` 和局部变量。

```cft
const MAX_TOTAL: int = 1000;

type Reward {
  rewards: [RewardItem] = [];

  check {
    var total = 0;
    for reward in rewards {
      require(reward.count > 0, "奖励数量必须为正");
      total += reward.count;
    }
    require(total <= MAX_TOTAL, "奖励总量不能超过上限");
  }
}
```

```text
for-stmt := "for" identifier [ "," identifier ] "in" expression block
```

- range `for` 按整数顺序迭代。
- list `for` 按稳定整数顺序读取元素。
- dictionary `for` 使用只读集合的稳定枚举快照。
- 双绑定的顺序固定为 `(索引或 key, 值)`；list 允许单绑定表示元素，dictionary 必须双绑定，range 单绑定表示索引。
- 一条 `require` 失败不会中断后续独立规则。
- 循环和高阶集合操作受执行预算约束。

```cft
type Loot {
  resistances: {string: float} = {};

  check {
    for element, value in resistances {
      require(0.0 <= value && value <= 1.0, "{element} 的抗性必须在 0 到 1 之间");
    }
  }
}
```

## 5. 顶层 check 与 `records(Type)`

```text
records-call := "records" "(" qualified-name ")"
```

```cft
check ItemIntegrity {
  require(records(Item).len() > 0, "项目中至少需要一个物品");

  for item in records(Item) {
    require(item.price > 0, "物品 {item.id} 的价格必须大于 0");
  }
}
```

- 顶层作用域没有隐式当前记录，不能使用裸字段或虚拟 `id`。
- `Type` 必须是静态 object type。
- `records(Base)` 包含实际类型为 `Base` 及其派生类型的所有顶层记录，不包含内联 object。
- 结果按 `(actual_type, record_key)` 稳定排序。
- 该结构只能用于顶层 check。

## 6. 自动规则

- check 自动处理类型规则、继承链规则和跨记录规则。
- 宿主参数、Host 返回值和临时构造值只进行类型及边界验证，不自动触发全量业务 check。
- 校验能力由 check 限定：check 只能使用 `require`、`records`、控制流、运算符和内建集合方法（高阶内建的回调是内联 lambda）；不能调用普通函数或其他 check。
- 应用 Host 外部服务不可从 check 访问。
- 校验失败是诊断，不是返回值。

```cft
type CurrencyReward : Reward {
  amount: int;

  check {
    require(amount > 0, "奖励金额必须为正");
  }
}

check RewardIntegrity {
  for reward in records(Reward) {
    require(reward is CurrencyReward, "奖励必须是已知类型");
  }
}
```

## 7. 诊断

- check 失败产生结构化诊断，保留错误码、严重级别、数据与 schema 位置、related locations 和上下文。
- 诊断可以由 `require` 的自定义消息描述，但不覆盖求值错误。
- 一条规则失败后，check 继续执行其他独立根语句，以便一次性报告多个问题。
- 每个根语句是独立执行单元；函数体内的嵌套语句不会脱离其根语句单独调度。

全部诊断和 fault 条件见 [11-错误与诊断](./11-错误与诊断.md)。
