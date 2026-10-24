# Coflow Schema 建模建议

## 先划分数据边界

- table 表达可被其他记录引用、可独立增删改、需要表格行或 CFD record 承载的配置实体。
- data 表达只属于父对象的一段结构，例如 `Stats`、`Reward`、`Condition`。
- 字段使用 `&TableName` 引用记录、data 名称嵌入值；CFD 引用值使用 `&id` 或 `&TableName::id`。
- table 与 singleton 使用 CFD 顶层记录 ID，不重复声明身份字段；data 的业务 id 按普通字段建模。

## 选择字段类型

| 需求 | 建议 |
| --- | --- |
| 固定分类、稀有度、阵营、元素 | `enum` |
| 可扩展配置项，且代码需要强类型 key | `@idAsEnum` + 空 enum |
| 可选值 | `Option<T>`，仅有显式默认值时允许省略；需要省略后为空时声明 `= None` |
| 多个值 | `[T]` |
| 按 key 查找的一组值 | `{string: T}`、`{int: T}` 或 `{Enum: T}` |
| 指向另一条记录 | `&TableName` |
| 嵌入结构 | data 名称 |
| 多种不同结构共用一个字段 | `abstract data` 父类 + 具体子类 |

## 默认值

- 默认值适合稳定、低风险、可推导的值，例如空数组、空字典、通用倍率、默认枚举值。
- 没有业务默认值的关键字段不要强行给默认值；让缺失数据直接暴露为诊断更清晰。
- `Option<T>` 字段仅有显式默认值时允许省略；需要省略后为空时声明 `= None`，嵌套层次通过 Some/None 明确表达。
- 插值模板使用 `string` 与显式 `f"{self.field}"`；普通字符串保持字面文本，业务约束写在 check 里。

## 继承和多态

```cft
abstract data Reward {
  source: string = "drop";
}

sealed data ItemReward : Reward {
  item: &Item;
  count: int = 1;
}

sealed data CurrencyReward : Reward {
  amount: int;
}
```

- data 父类保存共享字段与可复用校验；对象自身约束写在 data 的 check 中，所属记录的关系约束写在 table 或 singleton 中，跨记录约束写在顶层 check。
- `abstract data` 不能直接实例化，适合作为内联多态字段；记录分组使用 table 类型。
- `sealed data` 适合多态叶子；没有基类的 sealed data 可标注 `@struct` 生成 C# 结构体。
- 子类可以赋给父类字段；父类不能赋给子类字段。

## check 设计

- data、table、singleton 均可在字段后声明命名或匿名 check。命名规则适合长期查询与单独执行；匿名规则适合局部断言，通过 rule list 获取其当前标识。
- 常见规则包括 key 命名、数值范围、字符串非空、数组唯一、权重非负、引用集合约束和多态类型约束。
- 读取 `Option<T>` 内部值前，先用 `is Some(value)` 模式绑定；可在 `when` 条件或短路表达式中使用绑定值。
- 编辑器在默认值填充、引用解析后执行 check；引用字段可访问目标记录字段。

## 注解选择

| 注解 | 适用场景 |
| --- | --- |
| `@idAsEnum(EnumName)` | 记录 ID 需要生成稳定代码 enum |
| `@flag` | enum 表达位标志 |
| singleton | 全项目有且仅有一条配置记录 |
| `@localized` | 字段需要按语言维度覆盖 |
| `@dimension("名称")` | 字段需要按平台、地区等自定义维度覆盖 |
| `@int64` | enum 需要 64 位数值 |
| `@label` / `@description` | 类型、成员和字段需要业务名称与说明 |
| `@expand` | 表格中把内联对象展开为相邻列 |
| `@struct` | 无基类 sealed data 在 C# 中生成 struct |

## 常见错误

- 用 `string` 表达固定集合，导致拼写错误只能到运行期发现。改用 `enum`。
- 在 table 或 singleton 中重复声明 `id` 字段。记录身份使用 CFD 顶层 ID；data 中的普通业务 id 字段按其声明使用。
- 把所有嵌套结构都做成顶层记录。只有需要共享、引用或独立维护的对象才提升为顶层记录。
- 把所有东西都内联。需要跨 CFD 文件复用、查找或保持唯一身份时，用顶层记录和 `&TableName` 字段。
- 在 `check` 后继续声明字段。data、table、singleton 的所有字段都放在该类型的 check 之前。
- 让默认值掩盖数据缺失。关键业务值没有合理默认时不要设置默认值。
