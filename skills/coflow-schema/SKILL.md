---
name: coflow-schema
description: "Coflow CFT schema 与配置数据结构建模：当用户需要设计或修改 .cft、类型/字段/默认值/enum/const/check、记录引用、继承多态、@idAsEnum、singleton、@localized、维度、本地化或游戏配置数据结构建议时使用。"
---

# Coflow Schema

使用本 skill 设计和维护外部 Coflow 项目的 CFT schema。只处理 schema 和建模决策；数据只通过 CFD 文件进入 runtime，代码产物由 `coflow codegen` 生成。

## 建模流程

1. 先读取现有项目：`coflow schema inspect --project <project>` 获取结构化视图；`coflow project files --kind schema --project <project>` 获取原始注释、字段顺序和 `check`。
2. 明确数据形态：顶层记录、内联对象、枚举、引用、数组、字典、`Option<T>` 和多态边界。
3. 先设计字段类型和默认值，再设计 `check Name {}`；不要用 CFD 文件约定替代 schema 约束。
4. 修改 `.cft` 时优先使用：

```powershell
coflow source write --project <project> --file schema/main.cft --input candidates/main.cft --dry-run
```

5. 如果字段变化影响 CFD 顶层字段，更新对应 `.cfd` 记录并运行 `coflow check`。
6. 完成后运行 `coflow check --rules --project <project>`。

## 快速规则

- 顶层记录 ID 由 CFD 记录提供，规则中通过 `id` 读取；table 与 singleton 的 `id` 成员是固有身份，data 可以声明自己的 `id` 字段。
- 固定集合用 `enum`，共享阈值用 `const`，业务规则用 `check Name {}`。
- 字段使用 `&TableName` 表示记录引用，data 名称表示内联值；singleton 通过唯一入口访问。
- 可为空的值写 `Option<T>`；字段只有显式默认值才允许省略。需要省略后为空时写 `Option<T> = None`。
- 抽象父类用于多态接口，sealed type 用于不可再派生的值对象或叶子类型。
- 需要让记录 ID 进入代码枚举时，用 `@idAsEnum(Name)` 并声明空 enum。
- 字符串统一声明为 `string`；普通文本保留花括号，显式 `f"..."` 才执行模板插值。
- data、table、singleton 均可声明多个命名规则，同一继承链规则名唯一，父规则先执行，再按本类型声明顺序执行。
- `@localized` 绑定 language 维度；直接读取基础值，`.for("ja")` 读取有效变体，`.hasVariant("ja")` 判断字段是否显式填写，`.variants()` 遍历该维度全部已发现变体。

## Reference

- 数据结构设计建议和常见建模取舍：读 `references/modeling.md`。
- CFT schema 语法：读 `references/cft.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/03-language/01-cft>。
- Check 校验语法：读 `references/check.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/03-language/04-check>。
- 本地化与维度：读 `references/localization.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/10-localization>。
- Schema API 输出结构：读 `references/schema-api.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/11-schema-api>。
- DataModel 对默认值、引用和 check 的语义：读 `references/data-model.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/05-data-model>。
