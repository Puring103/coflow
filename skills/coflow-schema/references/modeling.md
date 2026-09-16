# Coflow Schema 建模建议

## 先划分数据边界

- `table` 表达可被其他记录引用、可独立增删改、需要 CFD record 承载的配置实体。
- `singleton` 表达全项目唯一的记录；`data` 表达只属于父对象的一段内联结构。
- table/singleton 类型用于记录字段，data 类型用于内联值；`&Type::key` 是 CFD/CFT 中的记录值语法。
- 不要把 record key 复制成字段；CFD 顶层 key 已经是虚拟 `id`。

## 选择字段类型

| 需求 | 建议 |
| --- | --- |
| 固定分类、稀有度、阵营、元素 | `enum` |
| 可扩展配置项，且代码需要强类型 key | `@idAsEnum` + 空 enum |
| 可选值 | `T?`；使用 `None` 或直接书写非空值 |
| 多个值 | `[T]` |
| 按 key 查找的一组值 | `{string: T}`、`{int: T}` 或 `{Enum: T}` |
| 指向另一条记录 | table 或 singleton 类型名 |
| 嵌入结构 | data 类型名 |
| 多种不同结构共用一个字段 | abstract table/data 父类 + 具体子类 |

## 默认值

- 默认值适合稳定、低风险、可推导的值，例如空数组、空字典、通用倍率、默认枚举值。
- 没有业务默认值的关键字段不要强行给默认值；让缺失数据直接暴露为诊断更清晰。
- `T?` 字段没有默认值且在 CFD 中省略时得到 `None`；显式非空值不写 `Some(...)`。
- 动态文本使用 `fstring` 和 `f"..."`；普通字符串中的花括号只是文本。

## 继承和多态

```cft
abstract data Reward {
  source: string = "drop";
}

sealed data ItemReward : Reward {
  item: Item;
  count: int = 1;
}

sealed data CurrencyReward : Reward {
  amount: int;
}
```

- 父类放共享字段和共享 `check`。
- abstract table/data 不能直接实例化，适合作为多态字段类型。
- sealed table/data 适合多态叶子；需要 C# struct 时在 sealed data 上增加 `@struct`。
- 子类可以赋给父类字段；父类不能赋给子类字段。

## `check {}` 设计

- 把上线前必须满足的配置规则写进 `check {}`，而不是放在导表后脚本里。
- 常见规则包括 key 命名、数值范围、字符串非空、数组唯一、权重非负、引用集合约束和多态类型约束。
- 读取 `T?` 内部值前，使用 `is Some(value)` 模式绑定；也可在返回 `T?` 的函数中使用 `value?` 传播 None。
- check 由调用方显式执行；执行时数据已经完成默认值填充和引用解析。

## 注解选择

| 注解 | 适用场景 |
| --- | --- |
| `@idAsEnum(EnumName)` | record key 需要生成稳定代码 enum |
| `@flag` | enum 表达位标志 |
| `@localized` | 字段需要按语言维度覆盖 |
| `@dimension("name")` | 字段需要按指定维度覆盖 |
| `@Host` | singleton 服务由宿主提供 |
| `@struct` | sealed data 在 C# 中生成 struct |

## 常见错误

- 用 `string` 表达固定集合，导致拼写错误只能到运行期发现。改用 `enum`。
- 在 schema 中声明 `id` 字段。改用 CFD 顶层 key，并在 `check` 中读取虚拟 `id`。
- 把所有嵌套结构都做成 table。只有需要共享、引用或独立维护的对象才使用记录，其他结构使用 data。
- 把所有东西都做成 data。需要跨 CFD 文件复用、查找或保持唯一身份时，使用 table 或 singleton。
- 让默认值掩盖数据缺失。关键业务值没有合理默认时不要设置默认值。
