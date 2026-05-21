# coflow fixture set

本目录保存从简单到复杂的 `.cf` 集成测试输入。

一两行即可表达清楚的词法或语法边界情况，直接写在对应测试文件中，不放入 `.cf` fixture。fixture 只保留多行或组合场景。

目录约定：

1. `lexer/valid/`：词法合法样例，只要求词法阶段无错误。
2. `lexer/invalid/`：词法错误样例，只要求词法阶段报告错误。
3. `lexer/expect/`：词法 golden 样例，源 `.cf` 和 `.tokens.expect` / `.lex-errors.expect` 放在一起。
4. `parser/valid/`：语法合法样例，只要求语法阶段无错误并返回模块。
5. `parser/invalid/`：词法合法但语法错误样例，只要求语法阶段报告错误。
6. `parser/expect/`：语法 golden 样例，源 `.cf` 和 `.ast.expect` / `.parse-errors.expect` 放在一起。

复杂嵌套样例优先放在对应阶段的 `valid/` 或 `expect/` 目录，用于覆盖对象、数组、字典、函数值、`co fn`、`validate` 和深层字段访问的组合。

可选的 expectation 文件：

1. 合法词法 golden 样例使用同名 `.tokens.expect`，逐行断言完整 token kind 和原始片段。
2. 非法词法 golden 样例使用同名 `.lex-errors.expect`，逐行断言词法错误类型、span 和原始片段。
3. 合法语法 golden 样例使用同名 `.ast.expect`，逐行断言语法树摘要。
4. 非法语法 golden 样例使用同名 `.parse-errors.expect`，逐行断言语法错误类型。语法错误 span 使用专项测试断言，避免 golden 过度绑定恢复策略。
5. expectation 用来锁定关键复杂样例；普通 fixture 仍只要求对应阶段能成功或失败。
