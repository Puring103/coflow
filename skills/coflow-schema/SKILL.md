---
name: coflow-schema
description: "为 Coflow 配置项目从零建模，设计或修改 CFT 类型、默认值、check、引用、多态和维度，并联合更新受影响的 CFD。适用于新项目和配置结构变更；已有结构下的数据维护使用 coflow-workflow。不用于开发 Coflow 本身。"
---

# Coflow Schema

依据业务目标维护声明和数据。沿用用户指定的项目与范围；缺少业务取值、实体边界或约束时询问，默认值应有业务依据。

## 执行主线

1. 新项目读[从零创建](references/new-project.md)，使用配套完整示例理解文件组织。已有项目用 `project inspect`、`project files`、`schema symbols --kind type` 发现范围，再对已有类型执行 `context --type <类型> --json`。
2. 根据[建模取舍](references/modeling.md)确定身份、内联对象、引用和规则边界。修改已有类型时，读取关联声明、规则、记录和覆盖，保存响应顶层 `revision`。
3. 用 `apply` 的 `kind: files` 一次提交 CFT、受影响的 CFD 和输入调整。新增必填字段时在同一候选中补全记录；路径和完整请求见[结构变更](references/schema-migration.md)。
4. `apply --rules --if-revision <revision> --json` 校验候选并写入；需要审阅时加 `--dry-run`。成功已经包含所选校验；直接编辑文件后运行 `check --rules`。有错误或检查未完成时继续处理，冲突后重读再形成候选。

`table` / `singleton` 的 id 是固有身份，`data` 是内联值。`Option<T>` 只表示可空，显式默认值才允许省略。普通字符串保持字面文本，只有 `f"..."` 插值。规则写在类型字段之后。

## 按任务读取

| 任务 | 资料 |
| --- | --- |
| 理解项目、发现能力与判断命令结果 | [核心概念与 CLI](references/concepts.md) |
| 从空目录得到可校验的完整项目 | [从零创建](references/new-project.md) |
| 设置输入、维度目录和生成目标 | [项目配置](references/project-config.md) |
| 划分实体、内联结构及规则边界 | [建模取舍](references/modeling.md) |
| 编写类型、枚举、别名、常量与注解 | [CFT 语法](references/cft-syntax.md) |
| 编写配套记录、字段值、模板 | [CFD 语法](references/cfd-syntax.md) |
| 决定必填、可空与默认行为 | [默认值与 Option](references/defaults-and-options.md) |
| 设计跨记录引用、继承与多态 | [引用与多态](references/references-and-polymorphism.md) |
| 编写或诊断业务规则 | [check 规则](references/check-rules.md) |
| 设计语言、平台等变体及覆盖规则 | [维度建模](references/dimensions.md) |
| 修改已有类型、联合更新数据、修复坏源码 | [结构变更](references/schema-migration.md) |

只加载相关资料。本 skill 可独立完成建模和结构变更；已有结构下的数据维护、C# 应用接入和插件任务由 `coflow-workflow` 承担。命令参数通过 `coflow describe <点分命令> --json` 获取，请求结构按需加 `--schema input`；能力不匹配时核对 CLI 版本。
