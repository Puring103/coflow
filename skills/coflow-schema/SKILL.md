---
name: coflow-schema
description: "设计或修改 Coflow 项目的 CFT 类型、字段、默认值、命名 check、引用、多态与维度，并同步受影响的 CFD。用于配置建模，不用于开发 Coflow 本身。"
---

# Coflow Schema

依据用户的业务目标维护声明和数据，沿用已授权的项目与范围；缺少业务取值或建模约束时询问。

先用 `coflow context --type <类型> --project <项目> --json` 获取相关字段、规则与少量记录，按需用 `schema inspect`、`rule inspect` 或 `source read` 补充。无需预先读取全部项目或所有参考。

CFT 与 CFD 必须同步变化时，使用 `apply` 的 `kind: files` 一次提交候选。`coflow describe apply --schema input --json` 提供准确结构。正式修改用 `apply --rules --if-revision <读取结果的 revision> --json`；需要先审阅时加 `--dry-run`。成功后无需重复校验；直接编辑文件后用 `check --rules` 检查项目。

## 核心语义

- table 与 singleton 的 `id` 是固有记录身份；data 可声明自己的 `id` 字段。`&Table` 是引用，data 类型是内联拥有值。
- `Option<T>` 表示可空，只有显式默认值才允许省略；省略后为空写 `Option<T> = None`。
- 字符串统一为 `string`；只有显式 `f"..."` 执行插值。
- data、table、singleton 支持多个 `check Name {}`；同一继承链规则名唯一，父规则先执行，再按声明顺序执行。顶层规则可查询记录集合。
- `@localized` 绑定 language。直接读取基础值，`.for("ja")` 读取有效变体，`.hasVariant("ja")` 判断显式覆盖，`.variants()` 枚举已发现变体。

## 按需参考

- [建模取舍](references/modeling.md)：类型、引用、多态与边界选择。
- [CFT](references/cft.md)：语法、默认值与修饰符；[公开文档](https://puring103.github.io/coflow/docs/reference/03-language/01-cft)。
- [Check](references/check.md)：规则、量词与消息；[公开文档](https://puring103.github.io/coflow/docs/reference/03-language/04-check)。
- [维度](references/localization.md)：覆盖与上下文回退；[公开文档](https://puring103.github.io/coflow/docs/reference/10-localization)。
- [Schema 查询](references/schema-api.md)：声明与使用位置；[公开文档](https://puring103.github.io/coflow/docs/reference/11-schema-api)。
- [数据模型](references/data-model.md)：加载与引用语义；[公开文档](https://puring103.github.io/coflow/docs/reference/05-data-model)。
