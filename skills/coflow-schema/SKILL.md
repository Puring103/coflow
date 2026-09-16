---
name: coflow-schema
description: "Coflow CFT schema 与配置数据结构建模：当用户需要设计或修改 .cft、table/singleton/data、字段/默认值/enum/const/check、记录引用、继承多态、@idAsEnum、@localized、维度、本地化或游戏配置数据结构建议时使用。"
---

# Coflow Schema

使用本 skill 设计和维护外部 Coflow 项目的 CFT schema。只处理 schema 和建模决策；数据只通过 CFD 文件进入 runtime，代码产物由 `coflow codegen` 生成。

## 建模流程

1. 先读取现有项目：`coflow schema inspect <project>` 获取结构化视图；`coflow schema files <project>` 获取原始注释、字段顺序和 `check`。
2. 明确数据形态：table/singleton 记录、data 内联对象、枚举、引用、数组、字典、`T?` 和多态边界。
3. 先设计字段类型和默认值，再设计 `check {}`；不要用 CFD 文件约定替代 schema 约束。
4. 修改 `.cft` 时优先使用：

```powershell
coflow schema write-file <project> --file schema/main.cft --check
```

5. 如果字段变化影响 CFD 顶层字段，更新对应 `.cfd` 记录并运行 `coflow check`。
6. 完成后运行 `coflow check <project>`。

## 快速规则

- 顶层 record key 由 CFD 记录提供；不要在 CFT 中声明 `id`、`Id` 或 `ID` 字段。
- 固定集合用 `enum`，共享阈值用 `const`，业务规则用 `check {}`。
- table 或 singleton 类型表示记录，data 类型表示内联对象；`&Type::key` 只用于记录值，不是类型语法。
- 可选字段写 `T?`；无默认值的可选字段省略时自动为 `None`。
- abstract table/data 用于多态父类，sealed table/data 用于不可再派生的叶子类型。
- 需要让 record key 进入代码枚举时，用 `@idAsEnum(Name)` 并声明空 enum。
- 使用 `@localized` 前确认 `coflow.yaml` 配置了 `dimensions.language`。

## Reference

- 数据结构设计建议和常见建模取舍：读 `references/modeling.md`。
- CFT schema 语法：读 `references/cft.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/03-language/01-cft>。
- Check 校验语法：读 `references/check.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/03-language/04-check>。
- 本地化与维度：读 `references/localization.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/10-localization>。
- Schema API 输出结构：读 `references/schema-api.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/11-schema-api>。
- DataModel 对默认值、引用和 check 的语义：读 `references/data-model.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/05-data-model>。
