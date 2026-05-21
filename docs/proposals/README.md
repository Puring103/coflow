# coflow 提案目录

本目录保存尚未进入核心语言的语法和语义提案。

核心规则：

1. 核心文档只描述已经决定的最小语言。
2. 未定能力不写进核心规格。
3. 每个提案独立说明动机，语法，语义冲突和实现成本。
4. 提案成熟后再合并进核心文档。

## 编号规则

提案编号按性价比排列：收益越高、实现成本越低、与 coflow 定位越契合，编号越靠前。编号不是语法稳定承诺。

## 当前路线图

| 编号 | 提案 | 状态 | 说明 |
|---|---|---|---|
| P01 | [联合类型与可空类型](./types.md) | 候选 | `T | null` 与 `T?`，补齐空值类型语义 |
| P02 | [is / is not 类型检查](./type-check.md) | 候选 | 运行时类型检查，与联合类型收窄配套 |
| P03 | [match 表达式](./match.md) | 候选 | 状态、枚举、类型分发的表达式语法 |
| P04 | [局部 const 常量](./const.md) | 候选 | 低成本提升运行时代码可读性和静态诊断 |
| P05 | [解构绑定与解构赋值](./destructuring.md) | 候选 | 对象、数组、字典 entry 拆包 |
| P06 | [数组与字典推导式](./array-comprehension.md) | 候选 | 配置生成和集合变换语法糖 |
| P07 | [类 LINQ 标准库](./linq-stdlib.md) | 候选 | 先做 Iterator 标准库算子，不直接加入 query syntax |

## 其他提案

这些提案暂不纳入当前性价比路线图，保留作为设计素材：

1. [控制流表达式](./control-expressions.md)
2. [可选访问与空值赋值](./optional-access.md)，部分能力已进入核心，剩余问题继续跟踪
3. [具名参数和默认参数](./named-and-default-args.md)，已进入核心
4. [class 方法和 validate](./class-methods-and-validate.md)，validate 已重命名为 check 并进入核心
5. [字符串字面量](./string-literals.md)
6. [对象与字典扩展](./object-dict-extensions.md)
7. [数组切片](./slice.md)
8. [`_` 弃元标识符](./discard.md)
