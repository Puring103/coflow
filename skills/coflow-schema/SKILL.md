---
name: coflow-schema
description: "为使用 Coflow 的配置项目设计或修改 CFT 类型、默认值、check、引用、多态与维度，同步受影响的 CFD。用于业务配置建模，不用于开发 Coflow 本身。"
---

# Coflow Schema

依据用户的业务目标维护声明和数据，沿用已授权的项目与范围；缺少业务取值或建模约束时询问。

## 选择起点

- 新项目：用 `coflow init <目录>` 创建配置，依据业务边界定义 CFT，再编写 CFD；配置路径和输出见[项目配置](references/project-config.md)。新类型尚不存在时先读相关源码，不用该类型调用 context。
- 已有项目但类型未知：用 `project inspect`、`project files` 和 `schema symbols` 发现范围；指定 `--project <项目> --json`。
- 已知类型：用 `context --type <类型>` 读取相关声明、规则与少量记录，按需用 `schema inspect`、`rule inspect`、`source read` 补充。按字段、分页与预算取所需信息，保存 revision。
- Schema 或 CFD 已损坏：先看诊断，使用 `project files` 和 `source read` 读取可用源码；模型查询不可用时转为文件修复。完整流程见 [CLI 的项目发现与恢复](references/cli.md#项目发现与错误恢复)。

CLI 参数与请求结构由 `coflow describe <命令> --json` 及 `--schema input` 查询；需要的能力不存在时核对 CLI 版本，不猜测参数或协议。

## 建模与验证

类型划分、默认值、继承及规则放置见[建模取舍](references/modeling.md)，语法见 [CFT](references/cft.md)，写值与引用见 [CFD](references/cfd.md)。

- table 与 singleton 的 id 是固有记录身份；data 可声明自己的 id 字段。`&Table` 为记录引用，data 为内联拥有值。
- `Option<T>` 表示可空，只有显式默认值才允许省略；省略后为空写 `Option<T> = None`。嵌套 Option 保留每层状态。
- 字符串统一为 string，只有显式 `f"..."` 执行插值。不要把普通文本改成模板。
- data、table、singleton 均支持命名或匿名 check，放在字段之后。命名规则在继承链内唯一，父规则先执行；需要复用的对象约束写在 data 自身，跨记录约束用顶层规则。
- 匿名规则的标识从 `rule list` 获取，不自行拼接；源码位置变化后重新查询。需要长期稳定身份时使用命名规则。
- `@localized` 绑定 language，`@dimension("名称")` 绑定其他维度；显式覆盖与回退有效值分别判断，详见[维度](references/localization.md)。

CFT 与 CFD 联合变化用 `apply` 的 `kind: files` 提交完整候选，文件模式的内容与路径见 [CLI 批量修改](references/cli.md#批量修改与请求结构)。声明新字段时，在同一候选中补全受影响记录或给出有业务依据的默认值。

写入携带读取结果的 `--if-revision`，通过 `apply --rules --json` 验证候选并发布；需要用户审阅时加 `--dry-run`。成功后无需重复同范围校验；直接编辑文件后用 `check --rules`。规则报错按[诊断](references/diagnostics.md)修复；冲突时重读并重新形成修改，不替换 revision 后盲目重试。存在数据或规则错误、检查未完成时不报告成功。

## 按需参考

- [建模取舍](references/modeling.md)：类型、默认值、继承与规则放置。
- [CFT](references/cft.md)：声明、类型与业务校验；[公开文档](https://puring103.github.io/coflow/docs/reference/03-language/01-cft)。
- [CFD](references/cfd.md)：字段值、对象、引用与覆盖；[公开文档](https://puring103.github.io/coflow/docs/reference/03-language/02-cfd)。
- [项目配置](references/project-config.md)：输入与生成目录；[公开文档](https://puring103.github.io/coflow/docs/reference/01-project-config)。
- [CLI](references/cli.md)：发现、候选修复、批量请求与冲突；[公开文档](https://puring103.github.io/coflow/docs/reference/08-cli)。
- [维度](references/localization.md)：覆盖与上下文回退；[公开文档](https://puring103.github.io/coflow/docs/reference/10-localization)。
- [诊断](references/diagnostics.md)：错误位置、执行状态与修复；[公开文档](https://puring103.github.io/coflow/docs/reference/09-diagnostics/01-diagnostics)。
