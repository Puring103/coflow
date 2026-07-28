# CFT Schema 建模

CFT 用于定义配置的类型、默认值、引用和业务规则。建模时应先表达稳定的业务概念，再考虑 Excel 列或 CFD 文本的具体布局。

```cft
enum Rarity {
  Common,
  Rare,
}

type Item {
  name: string;
  rarity: Rarity = Rarity.Common;
  tags: [string] = [];

  check {
    name != "";
    tags.isUnique();
  }
}
```

## 建模顺序

1. 定义 enum、const 和基础 type。
2. 用继承和 abstract/sealed type 表达多态。
3. 用 `&Type` 表达 record 引用，用对象字段表达内联值。
4. 把稳定补全规则写成默认值。
5. 把必须拦截的业务不变量写入 `check {}`。
6. 运行 `coflow cft check <project>` 后再开始录入数据。

语法、表达式、注解和维度见 [CFT 语言参考](../reference/03-language/01-cft.md)。
