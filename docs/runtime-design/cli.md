# Coflow CLI 设计

本文描述当前 CLI 面向 AI 编辑与自动化工作流的命令体系、输入输出协议、执行语义和验收要求。模块边界见 [系统架构](../architecture/overview.md)，语言规则见 [语言规格](../language-design/spec/01-总览.md)，公开用法见 [CLI 参考](../../website/docs/docs/reference/08-cli.md)。

## 1. 目标与职责

CLI 支持完整的配置编辑流程：发现项目、读取任务上下文、查询记录与维度、生成草稿、修改数据、预览候选变更、执行校验、查看统计与差异、生成代码。

基本流程是读取项目、提交修改、校验并写入文件。单条编辑命令适合简单修改，`apply` 用于一次提交多个修改；两者使用相同的校验和写入规则。`--dry-run` 只返回预览，正式执行时重新读取项目并校验。

设计目标：

- 使用语义查询减少全文读取，使用字段投影、分页和上下文裁剪控制输出量。
- 为记录、引用、维度和默认值提供明确的读写语义。
- 单步修改和批量修改使用相同的操作模型、校验流程和写入规则。
- 在无桌面编辑器的环境中执行声明、数据和业务规则校验。
- 支持 CFT、CFD 和项目输入配置的联合变更。
- 提供稳定、完整、可解析的成功与失败输出。
- 使用确定性的语言与模型能力辅助 AI；业务值、翻译内容和修复策略由调用方提供。

## 2. 命令体系

| 分组 | 命令 | 职责 |
| --- | --- | --- |
| 初始化 | `init` | 创建项目 |
| 自描述 | `describe` | 查询命令、参数和结构化输入输出协议 |
| 项目 | `project inspect/files/add-input/remove-input` | 查询有效项目配置、文件来源和输入范围，管理输入 |
| 任务上下文 | `context` | 聚合与目标类型或记录相关的声明、规则、示例和引用候选 |
| Schema | `schema check/inspect/symbols/refs` | 编译检查、声明查询、符号定位和使用位置查询 |
| 源码 | `source read/write/create/delete` | 读取源码、提交候选文件内容和管理项目源文件 |
| 数据 | `data list/get/search/draft/create/set/unset/rename/delete/transfer/refs` | 记录查询、草稿、编辑和引用查询 |
| 维度 | `dimension list/inspect/variants/get/set/clear/missing/diff` | 维度元数据、覆盖读写和完整性查询 |
| 统计 | `stats project/records/field/dimension/refs` | 项目规模、字段分布、覆盖率和引用分布 |
| 校验 | `check` | 检查数据，按选项执行业务规则 |
| 规则 | `rule list/inspect` | 查询业务规则、来源和静态依赖 |
| 诊断帮助 | `explain` | 按诊断码查询含义、触发条件和修复信息 |
| 批量变更 | `apply` | 对候选文件变更或结构化操作统一预览、校验与发布 |
| 差异 | `diff` | 比较项目源码、记录和维度覆盖的变化 |
| 格式化 | `format` | 格式化项目、指定文件或标准输入 |
| 代码生成 | `codegen` | 生成、预览和检查生成代码 |
| 完整构建 | `build` | 执行完整校验后生成代码 |
| 语言服务 | `lsp` | 提供语言服务器协议 |
| 技能管理 | `skill install/uninstall/status` | 管理随 CLI 分发的技能 |
| CLI 更新 | `self-update` | 查询和安装 CLI 更新 |

命令帮助、网站参考和技能示例使用本规范定义的名称、参数与执行语义。

## 3. 通用调用协议

### 3.1 项目与路径

```text
coflow <command> [subcommand] [arguments] --project <dir-or-config> --json
```

- `--project` 是项目命令的全局参数，接受项目目录或 `coflow.yaml`。
- 省略时从当前目录向父目录查找项目，并在结果中返回实际项目根目录。
- `init <dir>`、全局技能管理、自更新和命令自描述可以独立于项目运行。
- `--file`、目标文件、项目输入路径均相对项目根目录解释。
- `--input`、`--value-file` 等请求载体路径相对调用目录解释。
- 变更集内引用的候选内容文件相对变更集所在目录解释；stdin 变更集相对调用目录解释。
- 结果中的项目文件路径优先使用项目相对路径和 `/` 分隔符，项目外输入返回规范化绝对路径。
- 项目定位、路径解析与输入发现使用 Runtime 的统一实现。

### 3.2 输出格式

普通命令默认输出适合终端阅读的文本，`--json` 输出单行 JSON 对象，`--pretty` 隐含 JSON 并使用缩进。stdout 仅包含结果，进度与日志写入 stderr。`--details` 独立控制详细程度，默认采用计数与失败诊断，排版选项不改变响应范围。

统一信封包含：

| 字段 | 含义 |
| --- | --- |
| `protocol_version` | CLI 结构化协议版本 |
| `command` | 规范命令名称 |
| `status` | `success`、`partial`、`invalid`、`conflict` 或 `error` |
| `error_category` | `request` 或 `execution`；仅 `error` 状态提供 |
| `project` | 成功定位的项目根目录 |
| `revision` | 读取或发布后的项目输入修订，用于后续前置条件 |
| `result` | 命令专属结果 |
| `data_scope` | 默认提供加载完整性、可用记录数和来源计数；`--details` 展开来源列表 |
| `diagnostics` | 有界结构化诊断 |
| `diagnostics_summary` | 有诊断时提供 `total`、`returned` 和 `truncated` |

诊断包含错误码、阶段、严重性、消息、简短修复建议 `help`，以及可获得的源码位置、记录坐标、字段路径、相关位置和上下文。可选元数据为 None 时省略，附加列表为空时省略；业务值的 `null`、`None`、空字符串和空集合保留。坐标约定统一为从零开始的行列和 UTF-16 列偏移。

`data_scope` 默认保留 `complete`、`available_records`、`selected_file_count`、`incomplete_file_count`、`rejected_record_count` 和 `details_omitted`；`--details` 返回 `selected_files`、`incomplete_files`（含原因）和 `rejected_records`，空列表省略。精简范围信息不裁剪查询业务值。

CLI 默认最多输出 50 条诊断，`diagnostics_summary` 保存裁剪前总数；`--details` 解除默认上限，显式 `--limit` 仍限制诊断条数。输出裁剪发生在执行结果形成之后，不能影响执行完整性、事务发布信息、退出状态或业务值。成功任务默认汇总，`--details` 才输出逐目标规则和语句结果。

`--json` 覆盖参数错误、项目打开失败、输入读取失败、解析失败、执行失败和发布冲突。帮助与版本查询使用各自明确的输出方式；`lsp` 的 stdout 专用于语言服务器协议。

### 3.3 退出状态

| 退出码 | 含义 |
| --- | --- |
| `0` | 请求完成，满足命令要求 |
| `1` | 校验未通过、要求的结果不完整或状态检查未满足 |
| `2` | 命令参数或请求协议不合法 |
| `3` | 项目修订、分页游标、文件内容或预期旧值不一致 |
| `4` | I/O、权限、外部工具或内部执行失败 |

`success` 对应退出码 `0`；`invalid` 表示 Schema、数据、业务规则或产物状态检查未通过，对应 `1`；`partial` 表示要求的结果或执行不完整，对应 `1`；`error` 配合 `error_category: request` 对应 `2`；`conflict` 对应 `3`；`error` 配合 `error_category: execution` 对应 `4`。精确目标不存在属于 `invalid`。存在警告的完整成功结果仍使用 `success`。

请求解析错误尽量返回 `result.request_path`、从零开始的 `operation_index` 和 `target`。执行失败时返回失败原因；写入命令保留 `written`、实际写入文件、恢复成功和失败项及失败操作序号。

分页截断与项目数据不完整分别报告：分页通过 `truncated` 和 `next_cursor` 表达，项目加载或计算不完整通过 `status: partial` 表达。

列表查询匹配零条是成功；精确查询不存在的类型、记录、字段或文件返回目标不存在诊断。包含维度回退语义的查询按维度规则处理。

### 3.4 自描述

```bash
coflow describe --json
coflow describe data.set --json
coflow describe apply --schema input --json
```

根命令与分组查询默认只列 `commands` 目录、全局参数、退出码及下一步查询提示。具体命令返回单个 `result.definition`，包含参数、默认值、条件默认值、依赖与互斥条件、值约束、输入格式、只读或写入属性及简短示例。`--schema input|output|both` 仅适用于具体命令，按需在同一 definition 内提供 Schema，响应中不重复展开定义。自描述协议与实际参数和 DTO 定义同步生成。

每个命令列出必填参数、允许值、可重复参数和专属结果。参数格式检查在项目加载前完成。

### 3.5 值与字段路径

- 简单赋值支持 `--value-cfd`，使用字段预期类型解析 CFD 表达式。
- 大型对象、数组和文本支持 `--value-file` 或 stdin。
- 结构化请求使用带 `kind` 的值编码；CFD 表达式和模型值编码有明确区分。
- 查询结果保留有符号 64 位整数、有限浮点、嵌套 Option、枚举、引用、对象实际类型和字符串模板的语义。
- 普通字符串、引用、枚举与 `None` 具有不同表示；JSON `null` 不承担多层 Option 编码。
- 人类可读路径支持字段、数组下标和字典 key；JSON 请求使用类型化路径段，避免带点号、引号的字典 key 产生歧义。

模型值使用唯一的类型化编码，查询返回的值可以直接用于结构化写入：

| `kind` | 内容 |
| --- | --- |
| `bool` | `value` 为布尔值 |
| `int` | 整数 wire 表示；`value` 为十进制字符串，按目标 int / long 声明校验 32 / 64 位范围 |
| `float` | 浮点 wire 表示；`value` 为有限 JSON 数字，目标 float 按 binary32 舍入，double 保留 binary64 精度 |
| `string` | `value` 为字符串 |
| `none` | Option 当前层为空态 |
| `some` | `value` 为内层类型化值，可以继续嵌套 Option |
| `enum` | `type` 为枚举身份，`value` 为十进制数值字符串；`variant` 为单一命名成员名，组合值为空 |
| `ref` | `type` 为目标类型，`id` 为目标记录身份 |
| `object` | `type` 为实际类型，`fields` 为字段名到类型化值的映射 |
| `array` | `items` 为类型化值数组 |
| `dict` | `entries` 为包含类型化 `key` 和 `value` 的条目数组 |
| `fstring` | `source` 为完整 CFD `f"..."` 表达式；此协议标签表示字符串模板源码，字段类型仍为 `string` |

写入值使用 `{"kind":"model","value":<类型化值>}` 或 `{"kind":"expression","source":<CFD 表达式>}`。两种输入均转换为同一个值模型并按预期类型校验。`--value-cfd` 与 `--value-file` 互斥，后者读取 CFD 表达式，路径 `-` 表示 stdin。每次调用只允许一个 stdin 载体。

字符串模板的渲染结果放在独立的只读 `rendered` 信息中。字段可达性与显式赋值状态作为值外部的元数据返回；不可达路径与 Option 空态分别表达。

JSON 路径使用 `field`、`index`、`dict_key` 段，字典段的 `key` 使用类型化值。文本路径使用 `field.child`、`items[0]` 和 `labels["a.b"]`，字典键按字段声明类型解析。数组下标从零开始；路径在读取和写入中使用相同的解析与类型检查。

### 3.6 筛选、分页与排序

记录、维度明细和统计共享同一套选择器，支持类型、派生类型、文件、记录 ID、字段条件和维度取值视角。

- `--type` 默认精确匹配实际类型，`--include-derived` 扩展到具体派生类型。
- `--fields` 选择输出字段；`--where` 提供有限、无副作用的条件表达式。
- 条件支持类型检查后的比较、布尔组合、集合包含和状态判断，复用语言已有的值语义。
- 基础值是默认取值视角；`--dimension name=variant` 可重复指定不同维度，同一维度仅有一个选择。
- `--sort` 显式指定排序；默认按实际类型与记录 ID 稳定排序。
- 明细列表默认 `--limit 50`，通过 `--cursor` 翻页；游标只用于同一查询的后续读取。
- 游标绑定查询条件与项目 `revision`；页大小可调整，其余条件保持一致。项目变化后旧游标返回 `conflict`，从第一页重新查询。
- 聚合计算覆盖全部匹配记录，分页只限制聚合结果行或明细展示。

`--where` 使用字段路径、CFD 值字面量、比较运算 `== != < <= > >=`、布尔运算 `! && ||`、括号、集合包含和状态判断。布尔优先级依次为 `!`、比较、`&&`、`||`。函数限定为 `contains(path, value)`、`is_some(path)`、`is_none(path)` 和 `is_reachable(path)`；字符串与数组的包含按语言值语义处理，字典包含检查键。不可达值的比较结果为假，状态由对应判断函数查询。表达式在遍历前完成类型检查。

`--sort` 接受逗号分隔的字段路径，前缀 `-` 表示降序。升序中不可达值先于 `None`，再按可比较语义值排序；降序反转该顺序。对象、集合及互不兼容的类型作为排序目标时返回请求错误。所有排序最后追加实际类型与记录 ID 的升序作为并列顺序。异构查询的路径必须在所选类型上合法；嵌套对象按查询范围内记录的实际类型绑定路径，零样本类型按声明验证。运行时 Option 空态造成的不可达单独返回。

明细列表返回 `items`、`total`、`truncated` 和 `next_cursor`，分页大小可以调整。数据加载不完整时返回对应计数，`--details` 展开未加载文件及原因；计数仅表示已成功加载的范围。

列表通过分页控制输出量。上下文查询可以省略示例和关联信息，并返回继续查询的命令；精确查询返回完整的目标值。

## 4. 项目发现与源码操作

### 4.1 项目信息

```bash
coflow project inspect --json
coflow project files --kind data --json
coflow project files --kind schema --json
```

`project inspect` 返回有效配置、输入根、代码生成目标、维度显示配置和项目诊断。

`project files` 返回实际选中的文件、输入配置来源、文件角色、所属类型与维度摘要。文件角色支持基础记录、维度覆盖和混合文件，能够区分配置项与配置项展开后的实际文件。

这两个命令尽可能在 schema 或数据存在错误时运行，并明确未能获得的信息。

### 4.2 输入管理

```bash
coflow project add-input --kind data --path data/localization
coflow project remove-input --kind data --path data/localization --dry-run --json
```

输入管理校验重复、重叠和路径归属。`remove-input` 只移除配置中的输入项；物理文件删除使用 `source delete`。输入范围变化参与候选项目校验。

### 4.3 源码操作

```bash
coflow source read --file schema/items.cft --json
coflow source write --file schema/items.cft --input candidates/items.cft --dry-run --json
coflow source create --file data/new-items.cfd --input candidates/new-items.cfd
coflow source delete --file data/obsolete.cfd --dry-run --json
```

- `read` 返回源码、内容指纹和文件元数据，支持行范围；读取已选中源码不要求模型构建成功。
- `write` 替换已存在文件，`create` 创建新文件并按需要更新项目输入。
- `delete` 删除源文件并协调显式输入配置；引用和覆盖目标是否仍有效由候选项目校验决定。
- CFT 与 CFD 使用同一套文件写入接口。
- 写入默认执行结构与数据校验，校验失败时不发布。
- `--dry-run` 执行候选校验并返回差异，省略最终发布。
- 联合修改多个文件使用 `apply`。

## 5. Schema、规则与任务上下文

### 5.1 Schema 查询

```bash
coflow schema check --json
coflow schema inspect --type Item --related --json
coflow schema inspect --type Item --field price --json
coflow schema symbols --kind enum --json
coflow schema refs --symbol Item --json
```

`schema inspect` 返回类型类别、继承关系、字段声明类型、默认值、声明来源、显示说明、维度绑定和源码位置。字段级查询额外提供枚举候选、引用目标类型和具体对象类型候选。

`--related` 收集目标依赖的对象、枚举和常量，并按依赖去重。递归对象、引用环使用符号链接表达，遍历受深度和数量预算约束。未知筛选目标产生诊断。

`schema symbols` 提供类型、字段、枚举、常量和规则的轻量索引。

`schema refs` 返回符号的语义使用位置，并区分声明、类型引用、默认值、规则表达式和 CFD 中的使用位置。

### 5.2 规则查询

```bash
coflow rule list --type Item --json
coflow rule inspect --rule ItemPrices --json
```

规则输出包含身份、所属类型或项目、源码位置、表达式、消息和静态依赖。类型规则使用稳定身份 `Type::Rule`，项目规则使用名称；依赖、消息和位置按整个规则汇总。`--rule` 选择完整命名规则，内部根语句下标不构成公开身份。

静态依赖标明直接字段、记录集合和维度投影。依赖分析结果区分精确依赖与保守范围，不把引用图等同于完整业务影响范围。

### 5.3 任务上下文

```bash
coflow context --type Item --examples 3 --json
coflow context --type Item --id sword --json
```

上下文聚合：

1. 目标声明、说明、默认值和源码位置。
2. 关联类型、枚举、常量和可选择的具体类型。
3. 适用规则和可能约束目标的项目级规则。
4. 少量稳定选取的已有记录示例。
5. 引用目标摘要与进一步查询入口。
6. 维度、变体和相关诊断。
7. 目标记录的有效值与来源。

支持字段范围、示例数量、引用候选数量、依赖深度和输出字符预算。预算不足时保留核心类型信息，报告省略项及对应查询方式；裁剪后仍返回有效 JSON。

## 6. 记录查询与编辑

### 6.1 查询

```bash
coflow data list --type Item --fields id,name,price --limit 20 --json
coflow data get --type Item --id sword --json
coflow data search --text "治疗" --type Item --json
coflow data refs --type Item --id sword --direction incoming --json
```

`data get` 支持 `--view source|effective|both`，默认 `effective`：

- `source` 返回实际赋值、源码位置与可定位的源码片段。
- `effective` 返回按语言规则加载后的基础值或指定维度有效值。
- `both` 同时返回两种视角及取值来源。

来源区分显式基础赋值（`explicit_base`）、Schema 默认值（`schema_default`）和维度覆盖（`dimension_override`）。显式 `None` 沿用其赋值来源；省略带 `= None` 的字段属于 `schema_default`。穿过 None 等不可达路径返回 `origin: "unreachable"`、`reachable: false`，不提供来源位置或表达式。字符串模板的源码表达式与渲染值分别返回。

`data refs` 支持传入、传出和双向查询。每条边返回目标坐标、宿主坐标、字段路径、维度投影和来源位置。默认一跳，递归查询显式指定深度并报告截断。

### 6.2 草稿与创建

```bash
coflow data draft --type Item --id new_sword --json
coflow data create --type Item --id new_sword --file data/items.cfd --input item-fields.json
```

草稿区分 Schema 默认值、用于展示形状的类型初值和需要调用方输入的字段。引用字段返回目标类型与候选查询入口；抽象对象返回可选具体类型。

草稿包含 `complete` 和待输入项，不把形状初值当成已确定的业务值。创建时执行完整的字段与引用校验。singleton 的记录身份遵循语言规则。

### 6.3 直接修改

```bash
coflow data set --type Item --id sword --path price --value-cfd '100'
coflow data set --type Item --id sword --path quality --value-cfd 'Quality::Rare'
coflow data unset --type Item --id sword --path description
coflow data rename --type Item --id sword --new-id iron_sword
coflow data delete --type Item --id obsolete_item --dry-run --json
coflow data transfer --type Item --id sword --to-file data/weapons.cfd
```

- `set` 建立或替换明确的字段或嵌套值的显式赋值，默认作用于基础记录；与默认有效值相同的赋值仍建立显式数据。
- `unset` 移除字段赋值，随后应用默认值或缺失检查；设置 `None` 是独立赋值操作。
- 对默认对象的嵌套修改需要物化时，结果报告被物化的完整字段与实际来源。
- `rename` 更新记录身份、全部已识别引用、维度覆盖目标及相关 `@idAsEnum` 稳定映射文件。
- `delete` 协调该记录的覆盖删除，存在未解决的引用时由候选校验阻止发布。
- `transfer` 修改基础记录的源文件归属，已有覆盖保持其显式来源位置，报告涉及的所有文件。
- 已存在且相同的显式赋值保持原始源码。`changed: false` 表示最终源码与所有关联文件均未变化，保持文件内容和时间戳。

### 6.4 集合与批量编辑

公共操作模型提供数组插入、删除、移动及字典设置、删除操作。集合操作通过 `apply` 使用，以避免为每种容器扩展大量顶层命令。

- 数组下标按操作执行时的候选状态解释。
- 字典 key 按 Schema 键类型解释。
- 重命名、创建、删除和集合修改的先后顺序明确记录在操作数组中。
- 批量条件修改在该操作开始时按当前候选状态一次展开为具体记录坐标与操作，并返回目标清单与匹配数量。
- 支持 `expected_count` 和预期旧值，防止修改范围意外扩大或覆盖已有修改。
- 增量、比例调整等计算由调用方生成明确的新值；公共操作协议优先保持确定性的赋值语义。

## 7. 维度查询与编辑

### 7.1 元数据与取值

```bash
coflow dimension list --json
coflow dimension inspect --dimension language --json
coflow dimension variants --dimension language --json
coflow dimension get --dimension language --variant ja --type Item --id sword --field name --json
```

维度来自 CFT 字段绑定，变体来自项目选中的 CFD 覆盖块。变体查询包含显式空覆盖块声明的变体，以及每个变体的声明位置和覆盖数量。

`dimension get` 返回基础值、显式覆盖状态、覆盖值、有效值、取值来源与文件位置。

| 状态 | 显式覆盖 | 有效值 |
| --- | --- | --- |
| 无覆盖 | 否 | 基础值 |
| 普通覆盖 | 是 | 覆盖值 |
| 显式 `None` | 是 | 对应 Option 空态 |
| 覆盖与基础值相同 | 是 | 覆盖值 |

未知维度是错误；合法但尚未出现的变体按语言规则读取基础值，结果同时报告 `variant_exists: false`。

### 7.2 覆盖写入

```bash
coflow dimension set --dimension language --variant ja --type Item --id sword --field name --value-cfd '"剣"'
coflow dimension clear --dimension language --variant ja --type Item --id sword --field name
```

- 首次设置覆盖可以引入新变体。
- `set` 建立或替换显式覆盖，与基础值相同的覆盖仍作为显式数据保留；已有相同覆盖保持原始源码。
- 已有覆盖写回其来源文件。
- 新覆盖可以显式指定 `--file`；省略时复用项目统一的维度文件组织策略，并在发布前返回目标文件。
- 创建维度文件与更新显式数据输入配置属于同一事务。
- `clear` 移除整个字段的显式覆盖，使其回退基础值。
- 嵌套修改要求已存在可编辑的完整变体值；缺少时返回需要先设置完整字段的诊断。
- 覆盖字段必须绑定目标维度，值必须满足字段类型。
- `clear` 后保留明确的变体声明；该命令只清除字段覆盖。

空变体声明及全局变体改名、删除通过源码变更集表达。全局变体操作覆盖选定项目中的所有相关声明与覆盖，并按最终候选状态校验。

### 7.3 缺失与比较

```bash
coflow dimension missing --dimension language --variant ja --type Item --field name --json
coflow dimension diff --dimension language --from-base --to ja --type Item --json
coflow dimension diff --dimension language --from ja --to en --type Item --json
```

`missing` 查询不存在显式覆盖的记录字段，不将合法回退判为配置错误。结果包含基础值、字段说明和已有相关变体值，供调用方生成补齐内容。

`diff` 分别报告覆盖状态变化和有效值变化，支持筛选与分页。翻译正确性由业务规则或调用方判断。

基础视角使用 `--from-base` 或 `--to-base`，分别与 `--from`、`--to` 互斥；变体名称按原样解释。多维度取值先按语言规则合并全部选定覆盖，再计算字符串模板渲染值。差异报告显式表达式变化和引用、默认值、覆盖变化引起的间接渲染变化。

## 8. 统计

```bash
coflow stats project --json
coflow stats records --group-by type --json
coflow stats field --type Item --field price --metrics count,min,max,mean --json
coflow stats field --type Item --field quality --group-by value --json
coflow stats dimension --dimension language --variant ja --group-by type --json
coflow stats refs --direction incoming --json
```

| 类别 | 指标 |
| --- | --- |
| 项目 | Schema 类型数、记录数、文件数、维度数、变体数、引用边数、已获得的诊断计数 |
| 记录 | 按实际类型、继承范围、文件分组的记录数量 |
| 数值字段 | 数量、最小值、最大值、总和、均值、分位数 |
| 枚举与布尔 | 各取值数量和比例 |
| Option | 指定层的 `Some`、`None` 数量 |
| 字符串 | 空字符串数量、长度分布 |
| 数组与字典 | 空集合数量、长度分布 |
| 维度 | 可覆盖字段数、显式覆盖数、回退数、显式 `None` 数、覆盖率 |
| 引用 | 传入与传出引用数量分布、零传入记录、引用最多的记录 |

`stats refs --direction incoming|outgoing|both` 只返回所选方向的计数，默认双向。

统计遵循统一查询范围与取值视角，并返回统计口径：

- 记录数按实际记录计数，继承汇总不重复计数。
- 维度覆盖率分母为选定范围中可绑定该维度的“记录 × 字段”数量。
- 显式 `None` 计为覆盖；与基础值相同的显式覆盖也计为覆盖。
- 分母为零时覆盖率为空，并返回零分母状态。
- 覆盖率表示显式数据覆盖程度，翻译质量需要独立判断。
- 选取嵌套字段时，分别报告参与统计数量、路径不可达数量、指定 Option 层不可达数量和空态数量。
- 数值聚合定义精度与溢出行为，不静默丢失 64 位整数精度。
- 字符串长度以 Unicode 标量值计数，并在指标说明中明确。
- 零传入引用只表示当前配置模型中没有传入边。
- 无法加载的记录与文件单独计数；部分数据统计标记 `partial`，不表示完整项目结果。

规则执行耗时与工作计数通过 `--performance` 按需返回，与配置内容统计分离。内部事务缓存计数由测试验证。

## 9. 校验、诊断与生成

### 9.1 校验层级

| 命令 | 执行范围 |
| --- | --- |
| `schema check` | 项目 Schema 输入、CFT 解析与编译 |
| `check` | Schema、CFD、默认值、字段类型、记录身份、引用与维度合法性 |
| `check --rules` | 结构与数据检查，加全部 CFT 业务规则 |
| `check --rule <id>` | 结构与数据检查，加指定规则；报告实际执行范围 |
| `build` | `check --rules` 全部通过后生成并发布代码 |
| `codegen` | 结构与数据有效后生成代码，业务规则独立于生成执行 |

```bash
coflow check --rules --json
coflow check --rules --explain --limit 20 --json
coflow check --input changes.json --rules --json
```

`check --input` 使用与 `apply` 相同的变更集协议，对候选项目只读执行校验。Schema 候选检查同样支持请求文件或 stdin。

校验结果区分已通过、失败、未执行、因依赖错误跳过和预算耗尽。业务规则未完成时不能返回全部通过。

只有错误严重性的诊断阻止校验通过与发布，警告和信息诊断随结果返回。要求执行的规则必须全部完成且通过；未要求执行的业务规则使用 `not_executed`。`--rules` 与 `--rule` 互斥，重复 `--rule` 执行所选规则的并集并报告范围。

默认校验结果保留 `rule_counts`、`requested_tasks`、`executed_tasks` 和 `rejected_tasks` 等计数。`--details` 展开按目标汇总的 `results` 与根语句 `tasks`。`complete` 表示所选检查全部执行完成，普通业务失败可以是完整结果。

默认诊断上限为 50；`--details` 解除默认上限，`--limit` 显式限制返回条数，均保留完整计数。执行预算独立于输出限制。筛选某条记录的诊断不等于仅执行该记录的所有潜在依赖。

### 9.2 诊断解释

```bash
coflow explain DATA-011 --json
```

默认实例诊断的 `help` 提供简短修复方向。错误码解释包含含义、触发条件、合法形式和相关查询入口。`check --explain` 按需加入预期类型、实际类型、规则源码、相关值和有限候选；`--rule` 可将解释限定到指定规则。

自动修复入口仅承载确定性的源码修复。需要选择业务值、改变规则或决定翻译内容的情况返回待决信息。

### 9.3 生成与构建

```bash
coflow codegen --dry-run --json
coflow codegen --check --json
coflow build --json
```

- `codegen --dry-run` 返回将新增、修改、删除的产物和稳定映射变化。
- `codegen --check` 判断产物是否与当前输入一致，不写文件；存在差异返回退出码 `1`。
- 所有生成目标准备完成后统一发布，准备或校验失败时保留既有产物；发布与恢复遵循公共事务协议。
- `build` 明确返回每个阶段的状态、诊断和产物摘要。
- 成功修改配置后是否生成代码由调用方决定，数据写入命令只处理其声明的事务范围。

## 10. 候选变更与事务

### 10.1 统一入口

```bash
coflow apply --input changes.json --rules --dry-run --json
coflow apply --input changes.json --rules --json
```

变更集使用 `kind: files` 或 `kind: operations`，同一请求选择一种输入模式。两种模式共享候选构建、诊断、Diff、前置条件和发布流程。

文件模式描述文件创建、替换、删除及项目输入调整，适合 Schema 与 CFD 联合修改和错误源码修复。操作模式描述记录、集合和维度的语义修改。

文件模式示意：

```json
{
  "protocol_version": 1,
  "kind": "files",
  "changes": [
    {
      "op": "write",
      "file": "schema/items.cft",
      "content_file": "candidates/items.cft"
    },
    {
      "op": "write",
      "file": "data/items.cfd",
      "content_file": "candidates/items.cfd"
    }
  ]
}
```

操作模式示意：

```json
{
  "protocol_version": 1,
  "kind": "operations",
  "ops": [
    {
      "op": "set_field",
      "record": { "type": "Item", "id": "sword" },
      "path": [{ "kind": "field", "name": "price" }],
      "value": { "kind": "expression", "source": "100" }
    },
    {
      "op": "set_dimension_value",
      "record": { "type": "Item", "id": "sword" },
      "dimension": "language",
      "variant": "ja",
      "field": "name",
      "value": { "kind": "expression", "source": "\"剣\"" }
    }
  ]
}
```

单条编辑命令与 `apply` 使用相同的操作。请求格式由 CLI 定义，具体执行由 Runtime 完成。

### 10.2 执行顺序

1. 读取项目和请求，检查参数、目标与写入范围。
2. 按顺序执行修改，准备修改后的文件和项目配置。
3. 校验修改后的 Schema、数据、引用与维度；指定 `--rules` 时执行业务规则。
4. 计算差异；`--dry-run` 返回预览。
5. 正式写入前检查相关文件是否被外部修改，写入全部文件并返回结果。

操作按数组顺序执行，每一步使用前面操作修改后的数据。记录先创建，再修改字段或添加覆盖；改名后的操作使用新身份。引用在全部操作完成后检查，因此可以引用同批稍后创建的记录。某一步失败时停止执行，剩余操作标记 `not_executed`。

候选事务共享编译后的 Schema、文件语法树与记录索引；只重新解析实际变化的文件并更新索引。常规批次复用一次基线模型，最终结构校验仅构建一次候选模型，after 快照复用该会话。`set_where` 每次读取当前候选模型，以维持顺序筛选语义。

读取响应返回 `revision`，由配置、Schema、数据、稳定映射和输入文件身份与内容通过稳定 SHA-256 编码产生。所有项目写入与候选预览支持 `--if-revision`；两种 ChangeSet 的顶层都支持 `if_revision`，与参数同时提供时必须相同。前置条件在候选修改前校验，发布前重新发现输入范围并检查本次读取的原始内容；冲突报告当前修订。游标绑定相同修订，翻页时仅 `limit` 可改变。

`data set --expected-cfd` 与 `set_field.expected` 按字段类型比较当前基础有效值；默认字段按 Schema 物化，模板保留 `f` 源表达式进行比较。文件替换与删除可附加 `expected_fingerprint`。这些局部条件与覆盖所有操作的项目修订条件可以组合。冲突结果提供操作序号、目标、预期值和实际值。批量操作先确定目标清单并检查 `expected_count`，再修改这些目标。

### 10.3 校验与发布约束

- 所有操作成功且选定校验范围完整通过后才发布；请求、候选执行或校验失败保持正式文件原状。
- 默认执行结构与数据校验，业务规则通过 `--rules` 显式启用；结果始终返回业务规则执行状态。
- 项目已有错误时，文件模式可提交修复后的完整候选；严格校验以最终状态为准。
- 正式执行时重新读取并校验，预览结果供调用方查看。
- 写入前检查影响本次修改的 Schema、数据、配置、稳定映射及输入范围；发现外部修改时返回冲突。
- 路径规范化并解析符号链接后检查写入范围。源码写入允许项目内 CFT、CFD 与项目配置；项目外写入限定为显式输入文件或显式输入目录内的源文件。新源文件按候选配置纳入输入。生成目录专用于产物发布，与源码输入范围保持分离。配置路径按候选项目根目录解释。
- CLI 与编辑器通过 Runtime 协调写入。写入失败时尝试恢复原始文件，并报告实际写入、恢复成功和失败的文件；恢复时发现外部修改则保留该修改并报告冲突。

查询命令只读取文件。格式化只检查所选源码的格式，代码生成检查生成所需的数据；各命令的校验范围见对应章节。

### 10.4 写入结果

所有语义写入与源码写入统一返回：

- `changed`、`written`、`dry_run`。
- 结构与数据校验结果、业务规则状态及执行完整性。
- 默认 `operation_counts` 汇总所有操作状态，仅返回失败操作的序号、目标与原因；`--details` 返回全部操作。
- 匹配记录数、改变记录数、改变字段数。
- 涉及文件与实际写入文件。
- 自动更新的引用、覆盖、输入配置和稳定映射。
- 预览 `files` 中的源码补丁；`--details` 展开语义 Diff，独立 `diff` 命令提供按视图查询。失败报告与 Schema 预览使用同一源码报告形式。
- `diff --include-source` 才提供完整 `before` / `after`，`--summary` 仅保留路径与变更种类；`--details` 不隐含完整源码。
- `revision_before`、`revision_after` 表示读取与成功发布后的输入状态。

`written` 表示全部修改已成功写入。写入失败时返回实际写入文件、已恢复文件和恢复失败项。调用方可以通过源码指纹或重新读取数据确认实际内容。

默认精简输出保留上述发布与恢复信息，包含 `failed_operation`、`written_files`、`restored_files`、`restoration_failed` 等实际存在的上下文。单次 CLI 调用的返回状态完整反映是否落盘。校验失败与写入失败分别表达。

AI 修改流程采用定向读取、按已授权目标构建候选、`apply --rules` 提交；需要审阅时预览。成功响应已包含相同范围的校验结果，无需重复运行 `check`。

## 11. 差异与格式化

### 11.1 差异

```bash
coflow diff --base HEAD --summary --json
coflow diff --base main --type Item --json
coflow diff --input changes.json --json
```

- 普通 `diff` 默认比较 Git `HEAD` 与当前项目，`--base` 支持指定提交或分支。
- 历史端使用指定提交中的项目配置、Schema、CFD 和稳定映射，按历史输入范围构建；当前端使用当前配置及实际选中的输入，包含选中的未跟踪文件。提交或分支在调用开始时解析为固定提交身份。
- 历史端无法从指定仓库还原的输入列出路径和原因，结果标记不完整并保留可获得的源码差异。历史端与当前端分别返回配置来源和加载完整性。
- `diff --input` 比较当前项目与候选变更，不依赖 Git；与 `--base` 互斥。
- `--view source|schema|record|field|dimension` 选择源码、Schema、记录、字段或维度覆盖视图；省略时返回全部视图。
- `--summary` 只返回计数和对象摘要；`--include-source` 显式返回完整文件内容。
- JSON 返回差异与相关值，完整文件内容由 `--include-source` 请求。
- 任一端语义模型不可用时仍返回可获得的源码差异，标记 `semantic_available: false` 和 `status: partial`，退出码为 `1`。
- 存在正常差异本身不表示命令失败。
- 有效值变化与显式赋值变化分别报告，默认值与覆盖回退导致的变化具有来源说明。

### 11.2 格式化

```bash
coflow format --file data/items.cfd --check --json
coflow format --file data/items.cfd --dry-run --json
coflow format --stdin --language cfd
```

- 项目模式省略文件范围时格式化全部配置输入，支持重复 `--file`。
- `--check` 只检查格式，存在差异返回退出码 `1`。
- `--dry-run` 返回格式差异，不写文件。
- 文件模式的成功、预览、检查和冲突结果统一使用 `files[]` 的 `path`、`change`、`patch`；完整源码通过 `source.read` 读取。
- stdin 模式独立于项目；文本模式输出格式化源码，JSON 模式将源码放入结果对象。
- `--check` 与 `--dry-run` 互斥；stdin 的检查模式输出检查结果，普通与预览模式输出格式化源码。stdin 与项目文件范围互斥，语言由 `--language cft|cfd` 指定。
- 多文件先完成读取和格式化，再按公共事务发布。
- 语义写入尽量保留无关源码布局和注释；全文件规范格式化由 `format` 承担。

## 12. 实现边界

- CLI 负责参数解析、协议解码、输出与退出码映射。
- Runtime 负责项目打开、统一查询、候选构建、业务规则编排、语义写入、关联文件调整和发布。
- Model 负责数据值、引用、维度取值和模型诊断。
- Checker 负责业务规则执行和预算；完成状态向 Runtime 与 CLI 传播。
- Language 负责词法、语法、Schema 编译和语言级定位；Format 负责源码格式化。
- Staging 负责文件变更的统一发布与失败恢复。
- 编辑器和 CLI 共享维度目标文件策略、记录改名关联更新及项目输入配置调整。
- Schema 查询、上下文聚合、记录列表和统计复用统一选择器与取值视角。
- 查询默认使用不执行业务规则的数据会话，规则查询与执行显式选择，避免重复计算。
- 全局 Schema 不可用时保留项目和源码查询；部分 CFD 加载失败时返回可用记录并明确完整性。
- CLI 业务规则执行使用无界面的 Runtime 接口。

## 13. 验收要求

| 范围 | 验收要求 |
| --- | --- |
| 项目与协议 | 无项目、参数错误、输入读取失败时返回约定的 JSON 和退出码；命令自描述与实际协议一致 |
| 查询完整性 | 错误 Schema、局部错误 CFD 和未知目标具有明确状态；数据不完整与分页截断分别表达 |
| 值语义 | 64 位整数、嵌套 Option、枚举、引用、对象实际类型和字符串模板可无歧义读取与提交 |
| 上下文 | 关联声明与规则按目标收集，输出受预算约束，省略项可继续查询 |
| 规则执行 | 结构校验与业务规则执行范围明确；规则失败、跳过和预算耗尽均有独立状态 |
| 记录事务 | 支持前向引用、循环引用、记录迁移与字段修改的组合操作 |
| 维度事务 | 支持新记录与覆盖同批创建；覆盖文件、项目输入和记录身份关联更新一致 |
| 文件事务 | 支持多文件 Schema 与 CFD 联合变更；校验失败后项目文件保持原状 |
| 并发与恢复 | 外部修改触发冲突；发布失败执行恢复，并准确报告恢复结果 |
| 统计 | 分母、空集合、部分模型、数值精度和分位数计算方法明确且可验证 |
| 维度语义 | 回退基础值、显式空态和等于基础值的覆盖分别表达，覆盖率口径一致 |
| 分页 | 排序稳定，按相同查询条件分批读取；分页与加载不完整分别表达 |
| 差异 | 源码、记录和维度变化可分别查看；语义不可用时返回可用差异及完整性状态 |
| 格式化 | 项目、文件和 stdin 模式遵循一致格式规则，预览与检查均不写文件 |
| 生成与构建 | 产物状态检查保持只读，完整构建在全部规定校验通过后发布生成目标 |

### 13.1 关键行为用例

| 输入或场景 | 预期结果 |
| --- | --- |
| 查询并提交 int64 最大值及嵌套 `Some(None)` | 值往返一致，整数以十进制字符串承载 |
| 省略默认值为 `100` 的字段，执行 `set 100` | 建立显式赋值，`changed: true`；再次执行保持源码与时间戳 |
| 覆盖设置为与基础值相同的值，再执行 `clear` | 首次建立显式覆盖；清除后回退基础值并保留变体声明 |
| 同批创建两个互相引用的记录并创建覆盖 | 引用按最终集合连接；覆盖目标在操作时已经创建；统一校验发布 |
| 改名后以新身份修改，批量选择器读取前序修改 | 后续操作使用当前候选状态，批量目标一次展开并固定 |
| 第二个操作旧值条件失败 | 返回冲突与操作序号，后续操作未执行，正式文件保持原状 |
| 校验只有警告、规则未要求执行 | 完整结果为 `success`，规则状态为 `not_executed`，允许发布 |
| 要求执行的规则预算耗尽 | 返回 `partial` 与执行计数，正式文件保持原状 |
| 发布前输入新增文件或目标内容变化 | 返回 `conflict`，正式文件保持原状 |
| 写入中失败 | 尝试恢复原始文件，返回实际写入和恢复结果，保留恢复时发现的外部修改 |
| 部分 CFD 加载失败且查询无匹配 | 返回 `partial`、加载范围与不完整计数 |
| 历史提交修改了 Schema 和输入范围 | 历史端独立构建；无法还原的输入明确计入不完整范围 |
| 变体名称为 `base` | `--from base` 查询该变体，`--from-base` 查询基础值 |
| 上下文输出需要裁剪 | 保留目标声明，省略示例或关联信息，并返回继续查询的命令 |
