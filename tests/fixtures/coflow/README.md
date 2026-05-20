# coflow fixture set

本目录保存从简单到复杂的 `.cf` 集成测试输入。

目录约定：

1. `valid/`：应能通过对应阶段的合法样例。早期阶段可以只消费其中一部分。
2. `invalid/lex/`：词法错误样例。

复杂嵌套样例优先放在 `valid/100-complex-nested-module.cf`，用于覆盖对象、数组、字典、函数值、`co fn`、`validate` 和深层字段访问的组合。
