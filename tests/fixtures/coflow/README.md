# coflow fixture set

本目录保存从简单到复杂的 `.cf` 集成测试输入。

目录约定：

1. `valid/`：应能通过对应阶段的合法样例。早期阶段可以只消费其中一部分。
2. `invalid/lex/`：词法错误样例。
3. `invalid/parse/`：词法合法但语法错误的样例。

复杂嵌套样例优先放在 `valid/100-complex-nested-module.cf`，用于覆盖对象、数组、字典、函数值、`co fn`、`validate` 和深层字段访问的组合。

可选的 expectation 文件：

1. 合法样例可以增加同名 `.tokens.expect`，例如 `100-complex-nested-module.tokens.expect`，逐行断言完整 token kind 和原始片段。
2. 非法词法样例可以增加同名 `.errors.expect`，逐行断言错误类型、span 和原始片段。
3. expectation 用来锁定关键复杂样例；普通 fixture 仍只要求对应阶段能成功或失败。
