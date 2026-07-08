# Coflow Schema 建模建议

## 先划分数据边界

- 顶层 `type` 表达可被其他记录引用、可独立增删改、需要表格行或 CFD record 承载的配置实体。
- 内联 `type` 表达只属于父对象的一段结构，例如 `Stats`、`Reward`、`Condition`。
- `&Type` 用于跨记录共享和引用；普通 `Type` 用于嵌入值。
- 不要把 record key 复制成字段；数据源的 key 列或 CFD 顶层 key 已经是虚拟 `id`。

## 选择字段类型

| 需求 | 建议 |
| --- | --- |
| 固定分类、稀有度、阵营、元素 | `enum` |
| 可扩展配置项，且代码需要强类型 key | `@idAsEnum` + 空 enum |
| 可选值 | `T?`，可省略时加 `= null` |
| 多个值 | `[T]` |
| 按 key 查找的一组值 | `{string: T}`、`{int: T}` 或 `{Enum: T}` |
| 指向另一条记录 | `&Type` |
| 嵌入结构 | `Type` |
| 多种不同结构共用一个字段 | `abstract type` 父类 + 具体子类 |

## 默认值

- 默认值适合稳定、低风险、可推导的值，例如空数组、空字典、通用倍率、默认枚举值。
- 没有业务默认值的关键字段不要强行给默认值；让缺失数据直接暴露为诊断更清晰。
- nullable 字段没有默认值时仍必须填写；如果希望数据源可省略，写 `= null`。
- 默认值不能引用其他字段，也不能表达运行期逻辑；这种规则应写在 `check {}` 里。

## 继承和多态

```cft
abstract type Reward {
  source: string = "drop";
}

sealed type ItemReward : Reward {
  item: &Item;
  count: int = 1;
}

sealed type CurrencyReward : Reward {
  amount: int;
}
```

- 父类放共享字段和共享 `check`。
- `abstract type` 不能直接实例化，适合作为字段类型或分组入口。
- `sealed type` 适合多态叶子和 value-like 对象；需要 C# struct 时再加 `@struct`。
- 子类可以赋给父类字段；父类不能赋给子类字段。

## `check {}` 设计

- 把上线前必须满足的配置规则写进 `check {}`，而不是放在导表后脚本里。
- 常见规则包括 key 命名、数值范围、字符串非空、数组唯一、权重非负、引用集合约束和多态类型约束。
- 访问 nullable 字段前先用 `when x != null { ... }` 或短路表达式保护。
- check 会在默认值填充、引用解析后执行；引用字段可访问目标记录字段。

## 注解选择

| 注解 | 适用场景 |
| --- | --- |
| `@idAsEnum(EnumName)` | record key 需要生成稳定代码 enum |
| `@flag` | enum 表达位标志 |
| `@singleton` | 全项目有且仅有一条配置记录 |
| `@localized` | 字段需要按语言维度覆盖 |
| `@expand` | 表格中把内联对象展开为相邻列 |
| `@struct` | sealed value type 在 C# 中生成 struct |

## 常见错误

- 用 `string` 表达固定集合，导致拼写错误只能到运行期发现。改用 `enum`。
- 在 schema 中声明 `id` 字段。改用数据源 key，并在 `check` 中读取虚拟 `id`。
- 把所有嵌套结构都做成顶层记录。只有需要共享、引用或独立维护的对象才提升为顶层记录。
- 把所有东西都内联。需要跨 source 复用、查找或保持唯一身份时，用顶层记录和 `&Type`。
- 在 `check` 后继续声明字段。`check` 必须放在 type 内最后。
- 让默认值掩盖数据缺失。关键业务值没有合理默认时不要设置默认值。
