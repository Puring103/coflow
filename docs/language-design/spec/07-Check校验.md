# Check 校验

## 声明、身份与顺序

check 是对已编译 schema 和不可变 CFD 模型执行的业务约束。`data`、`table`、`singleton` 均可在字段之后声明多个命名或匿名规则；项目级规则在顶层声明。每条规则至少包含一条语句。

```text
check-decl := "check" [ identifier ] [ ":" message ] "{" check-statement { check-statement } "}"
check-body := "{" { check-statement } "}"
check-statement := expression [ ":" message ] ";"
                 | "when" expression [ ":" message ] check-body
                 | ("all" | "any" | "none") binding [ "," binding ] "in" expression [ ":" message ] check-body
message := string-literal | formatted-string-literal
```

规则的公开身份为 `DeclaringType::RuleName`；顶层规则直接使用规则名。类型及其继承链内禁止重名，无继承关系的类型可以使用相同短名。继承规则先执行父类型，再执行子类型；每个类型内按声明顺序执行。规则不覆盖父类型规则。

```cft
data Reward {
  item: &Item;
  count: int;
  check PositiveCount: "奖励数量无效" {
    count > 0: f"数量 {count} 必须大于 0";
  }
  check AvailableItem {
    item.enabled: "奖励物品必须启用";
  }
}
```

记录规则绑定当前记录，data 规则绑定当前内联对象。内联实例沿拥有关系枚举，包含对象字段、数组、字典、`Some` 及维度有效值；`None` 没有实例。实例枚举不追踪引用，规则表达式可以读取引用目标。

## 断言、条件和量词

每条断言必须返回 bool。普通失败记录诊断并继续收集同一规则的其他失败。`when condition` 仅在条件成立时执行主体，根条件不成立的任务状态为 `not_triggered`。

| 构造 | 成立条件 | 空集合 |
| --- | --- | --- |
| `all` | 每个元素都满足主体全部断言 | 成立 |
| `any` | 至少一个元素满足主体全部断言 | 不成立 |
| `none` | 没有元素满足主体全部断言 | 成立 |

数组双绑定顺序为 `item, index`，索引从零开始；字典双绑定顺序为 `key, value`，单绑定为具有 `.key` 和 `.value` 的条目。绑定只在量词主体内可见，不允许重复或遮蔽外层量词绑定。

`when false` 的主体不执行，该语句正常完成。因此 any/none 中筛选满足特定条件的元素时应使用布尔合取：

```cft
check AvailableItems {
  any item in records(Item): "项目至少需要一个可用物品" {
    item.enabled && item.price > 0;
  }
}
```

规则、when 和量词可以在块前提供冒号消息。块消息使用外围作用域，量词绑定仅可用于量词主体中的消息。失败时块消息补充断言消息，保留原有实际值、路径和来源证据；成功的块不求值消息。

## 维度语义

普通字段表达式读取基础值；`.for(variant)` 读取指定变体有效值；`.hasVariant(variant)` 只检测该字段是否有显式覆盖；`.variants()` 返回该字段所属维度全部已发现变体的有效值字典。

```cft
table Item {
  @localized name: string;
  check TranslationComplete {
    !name.isBlank(): "基础名称不能为空";
    all variant, text in name.variants(): "翻译不完整" {
      name.hasVariant(variant): f"缺少 {variant} 的显式名称";
      !text.isBlank(): f"{variant} 名称不能为空";
    }
  }
}
```

显式 `None`、空字符串及与基础值相同的覆盖均满足 hasVariant。缺少覆盖时选择基础表达式并在当前维度上下文求值；模板依赖读取同维度同变体的有效值。没有在项目中出现的变体不会由 variants 自动产生，固定交付要求应显式检查 `hasVariant("ja")`。

## 规划与执行边界

`coflow-language` 编译规则块、根语句、静态字段/记录集合依赖及维度依赖。`CheckStatementInfo` 保存声明类型、块索引、规则短名、公开 rule_id 与根语句索引，`check_statements_for_rule` 返回完整规则的根语句序列。

`coflow-runtime` 枚举记录和内联 data 实例，按照依赖选择受影响规则。规则块是增量执行的最小单位：任意根语句依赖受影响时，完整重跑同一实例的该规则。具有 data 规则的项目使用完整计划，以覆盖拥有树变化及跨记录读取；重新发布诊断前清除旧任务。维度变体目录变化会重新检查全部规则。

`coflow-checker` 在不可变模型上执行根语句任务，按实例和规则顺序排序。业务失败继续执行；表达式或消息求值错误停止该实例的当前规则，剩余根语句标记为未执行，其他规则继续。单任务及请求级预算约束求值、集合遍历和文本生成。

| 任务状态 | 含义 |
| --- | --- |
| `passed` | 已执行且成立 |
| `not_triggered` | 根 when 条件不成立，主体未执行 |
| `failed` | 业务断言不成立 |
| `evaluation_error` | 求值不能完成，如除零、越界 |
| `budget_exhausted` | 当前任务达到求值预算（引擎枚举 `BudgetExceeded`） |
| `not_executed` | 同规则前置错误或请求预算使任务未执行 |

统计中的 task 指根语句与实例的组合；对外规则列表按 rule_id 归并。

## 诊断与入口

诊断携带规则身份、CFT 精确表达式/语句位置、CFD 数据路径与来源，以及 when、量词元素和维度上下文。all 保留失败元素的位置；any 报告零个匹配及集合大小；none 定位满足条件的元素。用户消息不会抹去来源信息。

执行器另外保存实际值的模型游标，区分存储来源、主报错位置及来源引用的维度上下文。CLI 解释按该游标读取实际值，变体失败读取该变体的有效值；通过维度引用访问目标基础字段时读取目标基础值。游标只用于内部求值与代次重映射，不进入编辑器和 CLI 的位置协议。

| 入口 | 执行范围 |
| --- | --- |
| `coflow schema check` | CFT 编译检查 |
| `coflow check` | 声明与 CFD 数据校验 |
| `coflow check --rules` | 声明、数据与全部业务规则 |
| `coflow check --rule Reward::PositiveCount --explain` | 选定规则及执行解释 |
| `coflow rule list` | 可选择的命名或匿名规则 |
| `coflow build` | 全部校验通过后生成代码 |

CLI 的 data、stats、ref、dimension 查询与检查共享 schema 和模型的只读语义；规则不执行 CLI 子进程。C# 加载器负责数据加载，业务规则由 Rust 检查引擎执行。入口协议见 [CLI 设计](../../runtime-design/cli.md)，表达式类型与内建操作见 [校验表达式](06-校验表达式.md)。

## 匿名规则

类型内部和文件顶层均支持 `check { ... }` 与 `check: "消息" { ... }`。匿名规则不参与名称冲突检查，按源码顺序执行。AST 保留缺省名称；schema 使用 `@模块:字节偏移` 标识匿名块，类型规则的完整标识加上 `Type::` 前缀。位置变化后标识可以变化，稳定标识由显式命名提供。CLI 查询、选择执行和诊断使用同一规则身份。
