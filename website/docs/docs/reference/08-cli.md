# CLI 命令

CLI 支持查询、编辑、校验、差异预览和代码生成。项目命令使用 `--project <目录或 coflow.yaml>`；省略时从当前目录向父目录查找项目。项目文件路径相对项目根目录，请求文件路径相对调用目录。

## 查询与上下文

```powershell
coflow describe --json
coflow describe data.set --json
coflow describe apply --schema input --json
coflow project inspect --json
coflow project files --kind schema --json
coflow source read --file schema/main.cft --json
coflow schema inspect --type Item --related --json
coflow schema symbols --kind field --json
coflow schema refs --symbol Item.price --json
coflow context --type Item --examples 3 --json
coflow data list --type Item --fields name,price --limit 20 --json
coflow data get --type Item --id sword --view both --json
coflow data search --text 治疗 --json
coflow data refs --type Item --id sword --direction incoming --json
coflow rule list --type Item --json
coflow rule inspect --rule ItemPrices --json
```

`describe` 默认返回简明命令目录。`describe data.set` 返回该命令的参数、默认值、约束、输入格式和示例；需要 JSON Schema 时，在具体命令后加 `--schema input|output|both`，定义集中在 `result.definition`，只展开一次。

`schema inspect` 返回声明、默认值、继承关系和源码位置；字段查询提供枚举、引用和具体对象类型候选。`context` 聚合声明、规则、记录示例与引用候选，用 `--fields`、`--examples`、`--ref-limit`、`--depth` 和 `--budget` 控制范围；预算单位为序列化结果的字符数，省略内容会明确报告。

`data get --view source|effective|both` 分别查询显式赋值、有效值或两者。`origin` 区分基础赋值（`explicit_base`）、Schema 默认值（`schema_default`）和维度覆盖（`dimension_override`）。显式 `None` 保留赋值来源，省略带 `= None` 的字段属于 Schema 默认值；穿过 None 的路径则返回 `origin: "unreachable"`、`reachable: false`，没有来源位置或表达式。字符串模板表达式和渲染信息分别返回。

列表共享 `--type`、`--include-derived`、`--file`、`--id`、`--where`、`--sort`、`--dimension name=variant`、`--limit` 和 `--cursor`。列表默认每页 50 条，返回 `items`、`total`、`truncated` 与 `next_cursor`。游标绑定查询条件与响应的 `revision`；翻页时可调整 `--limit`，其余条件保持一致。项目变化后旧游标返回 `conflict`，去掉 `--cursor` 重新查询。聚合计算覆盖全部匹配记录，分页限制输出行。

`--where` 支持比较、布尔组合和 `contains`、`is_some`、`is_none`、`is_reachable`。`--sort` 使用逗号分隔路径，前缀 `-` 表示降序。文本字段路径支持 `field.child`、`items[0]` 和 `labels["a.b"]`。

## 修改与预览

```powershell
coflow data draft --type Item --id new_sword --json
coflow data create --type Item --id new_sword --file data/items.cfd --input fields.json
coflow data set --type Item --id sword --path price --value-cfd 100 --expected-cfd 90 --rules
coflow data unset --type Item --id sword --path description
coflow data rename --type Item --id sword --new-id iron_sword
coflow data transfer --type Item --id sword --to-file data/weapons.cfd
coflow data delete --type Item --id obsolete --dry-run --json
coflow source write --file schema/main.cft --input candidates/main.cft --dry-run --json
coflow source create --file data/new.cfd --input candidates/new.cfd
coflow source delete --file data/obsolete.cfd --dry-run
coflow project add-input --kind data --path data/localization
coflow project remove-input --kind data --path data/localization --dry-run
coflow apply --input changes.json --rules --dry-run --json
coflow apply --input changes.json --rules --json
```

草稿区分声明默认值、类型默认值与待输入字段，以 `complete` 和 `required_inputs` 表达完整性。`set` 建立显式赋值，`unset` 移除赋值并应用默认值或缺失检查。相同的已有显式赋值保持源码和时间戳。改名更新已识别引用、维度目标和稳定枚举映射；迁移保留覆盖的来源文件。

写入先构建候选项目，结构与数据校验通过后统一发布；`--rules` 额外执行业务规则。AI 批量修改推荐 `apply --rules --json`，需要先审阅时加 `--dry-run`。成功结果已经包含所选校验，不必紧接着重复执行同范围的 `check`。

项目查询返回 `revision`。所有项目写入与候选预览都可携带 `--if-revision <读取时的 revision>`；`apply` 请求也可在顶层设置 `if_revision`，同时提供时两者必须一致。项目输入变化会返回退出码 3 和当前修订，调用方应重新读取并据此调整修改。`data set --expected-cfd` 和 `set_field.expected` 额外比较按字段类型解析、补齐默认值后的当前基础有效值；模板按 `f` 源表达式比较。文件替换或删除还可使用 `expected_fingerprint` 检查文件内容。

写入结果提供操作计数、匹配与改变数量、文件及自动更新摘要。默认只列失败操作，成功和未执行操作汇总在 `operation_counts`；`--details` 展开所有操作与语义差异。预览的 `files` 保留源码补丁，独立 `diff` 也可查看语义变化。完整源码由 `diff --include-source` 请求，`--details` 不隐含完整源码。失败结果保留 `written`、`written_files`、`restored_files`、`restoration_failed` 和失败操作序号等实际发布信息。

`apply` 支持两种变更集：`kind: files` 的 `changes` 描述源码和输入调整；`kind: operations` 的 `ops` 描述记录、集合及维度操作。操作按顺序使用当前候选状态，引用在全部操作结束后检查。数组插入、删除、移动和字典设置、删除通过操作模式提交。请求参数见 `coflow describe apply --json`，完整请求结构见 `coflow describe apply --schema input --json`。

```json
{
  "protocol_version": 1,
  "kind": "operations",
  "ops": [{
    "op": "set_field",
    "record": {"type": "Item", "id": "sword"},
    "path": [{"kind": "field", "name": "price"}],
    "value": {"kind": "expression", "source": "100"},
    "expected": {"kind": "expression", "source": "90"}
  }]
}
```

值输入使用 CFD 表达式或 `{"kind":"model","value":...}`。类型化模型值保留 int64 十进制字符串、有限浮点、每层 `some`/`none`、枚举身份和数值、引用身份、对象实际类型、集合及显式模板源码。枚举的 `variant` 为单一命名成员名，组合值没有单一成员名。查询模型值可以用于结构化写入，渲染与来源信息保持只读。`--value-file` 读取 CFD 表达式，`-` 表示 stdin；每次调用只能使用一个 stdin 载体。

## 项目发现与错误恢复

新项目用 `coflow init <目录>` 建立配置。已有项目先查看配置与输入，再发现类型：

```sh
coflow project inspect --project . --json
coflow project files --project . --json
coflow schema symbols --project . --kind type --json
coflow context --project . --type Item --json
```

`context --type` 需要类型已经存在。新类型先编写 CFT；类型未知时先用 symbols。配置、Schema 或数据有错误时，按当前可用层级读取，`project inspect/files` 会尽量返回已知信息；模型查询失败后可读取项目已选中的源码，不需要先修好全部数据：

```sh
coflow source read --project . --file schema/items.cft --json
coflow source read --project . --file data/items.cfd --json
```

Schema 错误优先修正类型、声明或配置；CFD 记录无法结构化时按诊断位置修正源码。多文件互相依赖的修复用下面的文件变更集一起提交。`check --input changes.json --rules` 可只校验候选，`apply --input changes.json --rules` 校验后写入。候选仍有错误时文件保持原样，继续按返回诊断修改候选。

## 批量修改与请求结构

`apply` 的两种模式都使用 `protocol_version: 1`；同一请求选择一种模式。`record` 用实际类型与 ID 定位；结构化 path 的片段为 `field`、`index` 或 `dict_key`，数组索引从零开始。值用 `{"kind":"expression","source":"CFD 表达式"}` 或类型化 model 值。

### 记录、集合与维度操作

以下是 `kind: operations` 支持的全部操作。表中列出每种操作的字段，带“可选”的字段可以省略；value、key、expected 均为上述值输入。

| op | 字段 | 行为 |
| --- | --- | --- |
| `create_record` | record、file；可选 fields | 创建记录，fields 按字段名提供值 |
| `set_field` | record、path、value；可选 expected | 设置字段，expected 校验当前基础有效值 |
| `unset_field` | record、path | 移除显式赋值，重新应用默认值或必填检查 |
| `rename_record` | record、new_id | 更新 ID、已识别引用及覆盖目标 |
| `delete_record` | record | 删除记录及其覆盖，剩余引用须有效 |
| `transfer_record` | record、to_file | 迁移基础记录，已有覆盖保留其来源 |
| `set_dimension_value` | record、dimension、variant、field、value；可选 path、file | 设置覆盖；path 相对该顶层字段，覆盖仍按完整字段保存 |
| `clear_dimension_value` | record、dimension、variant、field | 清除显式覆盖 |
| `array_insert` | record、path、index、value | 在数组位置插入元素 |
| `array_remove` | record、path、index | 删除数组元素 |
| `array_move` | record、path、from、to | 移动数组元素 |
| `dict_set` | record、path、key、value | 设置或更新字典项 |
| `dict_remove` | record、path、key | 删除字典项 |
| `set_where` | selector、path、value、expected_count | 对全部匹配记录赋值，匹配数必须符合预期 |

例如给 sword 的 tags 数组添加元素，并给 labels 字典设置一个值（要求 CFT 已声明这两个字段）：

```json
{
  "protocol_version": 1,
  "kind": "operations",
  "ops": [
    {
      "op": "array_insert",
      "record": {"type": "Item", "id": "sword"},
      "path": [{"kind": "field", "name": "tags"}],
      "index": 0,
      "value": {"kind": "expression", "source": "\"weapon\""}
    },
    {
      "op": "dict_set",
      "record": {"type": "Item", "id": "sword"},
      "path": [{"kind": "field", "name": "labels"}],
      "key": {"kind": "expression", "source": "\"category\""},
      "value": {"kind": "expression", "source": "\"equipment\""}
    }
  ]
}
```

### 按条件修改与范围保护

先用 `data list --type Item --where 'price < 100' --fields name,price --json` 核对目标和 total，再提交相同条件的变更。JSON selector 中条件字段名为 `filter`，命令行参数则为 `--where`：

```json
{
  "protocol_version": 1,
  "kind": "operations",
  "ops": [{
    "op": "set_where",
    "selector": {"type": "Item", "filter": "price < 100"},
    "path": [{"kind": "field", "name": "price"}],
    "value": {"kind": "expression", "source": "100"},
    "expected_count": 2
  }]
}
```

expected_count 填核对后的实际数量；不匹配时返回 conflict，候选不发布。selector 的 limit 不作为写入数量上限，操作作用于全部匹配记录。不要根据当前页的 items 数量推测总目标数。操作按顺序读取当前候选，因此前面操作会影响后续筛选结果。

写入时用 `--if-revision <查询结果的 revision>` 保护读取范围；单条字段再用 expected 保护旧值。修改计划已过期时重新读取和审阅，不直接替换旧 revision 重试。

### CFT 与 CFD 联合修改

`kind: files` 支持 `write`、`create`、`delete`、`add_input` 和 `remove_input`。write/create 的 file 是项目相对路径；内容使用 content 字符串或 content_file 文件，两者恰好一个。content_file 相对请求 JSON 所在目录解析，`--input -` 时相对调用目录解析；`--input` 本身相对调用目录。write/delete 可携带 expected_fingerprint。

例如请求文件与 candidates 目录放在同一目录下，将类型和数据一起替换：

```json
{
  "protocol_version": 1,
  "kind": "files",
  "changes": [
    {"op": "write", "file": "schema/items.cft", "content_file": "candidates/items.cft"},
    {"op": "write", "file": "data/items.cfd", "content_file": "candidates/items.cfd"}
  ]
}
```

候选 CFT 与 CFD 必须共同满足新类型约束；新增必填字段时一并补齐数据。add_input/remove_input 使用 `kind: "schema"` 或 `"data"` 与 path，remove_input 只移除配置项。文件模式适合修复不能结构化加载的源码；仅改已知字段或集合时可用操作模式保留其他源码。

```sh
coflow check --project . --input changes.json --rules --json
coflow apply --project . --input changes.json --rules --dry-run --json
coflow apply --project . --input changes.json --rules --json
```

按需要选择只校验、预览或正式写入。`--dry-run` 与正式 apply 的候选校验一致，成功预览不代表已经写入。完整请求结构可用 `coflow describe apply --schema input --json` 查询。

## 维度与统计

```powershell
coflow dimension list --json
coflow dimension inspect --dimension language --json
coflow dimension variants --dimension language --json
coflow dimension get --type Item --id sword --field name --dimension language --variant ja --json
coflow dimension set --type Item --id sword --field name --dimension language --variant ja --value-cfd '"剣"'
coflow dimension clear --type Item --id sword --field name --dimension language --variant ja
coflow dimension missing --dimension language --variant ja --json
coflow dimension diff --dimension language --from-base --to ja --json
coflow stats project --json
coflow stats records --group-by type --json
coflow stats field --type Item --field price --metrics count,min,max,mean --json
coflow stats dimension --dimension language --variant ja --group-by type --json
coflow stats refs --direction incoming --json
```

无显式覆盖时回退基础值。`get` 返回 `explicit` 表示该字段是否覆盖，`variant_exists` 表示该维度是否存在变体；`override` 与 `effective` 分别提供显式覆盖和有效值。显式 `None` 与等于基础值的覆盖都计为覆盖；清除后回退基础值。`missing` 返回缺少显式覆盖的字段、显示说明、基础值及其他显式变体值。维度差异分别报告覆盖状态与有效值变化。`--from-base`、`--to-base` 指定基础视角，名称为 `base` 的变体仍是普通变体。

统计返回参与数量、路径不可达、指定 Option 层不可达及空态数量和统计口径。枚举与布尔字段默认返回值分布；字段与指标在空结果时也会校验。整数总和使用精确十进制字符串，均值保留精确分子与分母，分位数采用 nearest-rank。字符串长度按 Unicode 标量计数。覆盖率分母为可覆盖的记录与字段组合数，零分母时覆盖率为空。引用统计只返回 `--direction` 所选方向，默认双向。

## 校验、差异与生成

```powershell
coflow schema check --json
coflow check --json
coflow check --rules --explain --limit 20 --json
coflow rule list --json
coflow rule inspect --rule Item::ValidPrice --json
coflow check --rule Item::ValidPrice --explain --json
coflow check --input changes.json --rules --json
coflow explain DATA-011 --json
coflow diff --base HEAD --summary --json
coflow diff --input changes.json --json
coflow format --file data/items.cfd --check --json
coflow format --file data/items.cfd --dry-run --json
coflow format --stdin --language cfd
coflow codegen --dry-run --json
coflow codegen --check --json
coflow build --json
```

`schema check` 检查 Schema；`check` 检查结构、字段、引用和维度；`check --rules` 执行全部业务规则。重复 `--rule` 执行选定规则的并集，与 `--rules` 互斥。候选检查使用与 `apply` 相同的请求并保持只读。命名规则的类型内标识形如 `Item::ValidPrice`，顶层规则使用自己的名称。匿名规则也可单独执行，先用 rule list 获取当前标识；源码位置变化后重新查询，需要长期稳定标识时为规则命名。默认返回 `rule_counts` 和请求、已执行、未执行任务计数；`--details` 才展开每个目标的整条规则 `results` 和语句 `tasks`。状态区分通过、条件未触发、业务失败、求值错误、未执行和预算耗尽。`complete` 表示所选校验是否完整完成，和是否通过分别判断。

`diff` 默认比较 Git HEAD 与当前项目，支持提交、分支及候选变更。`--view source|schema|record|field|dimension` 选择源码、Schema、记录、字段或维度视图；省略时返回全部视图。`--summary` 输出计数与对象摘要，`--include-source` 请求完整源码。语义不可用时保留源码差异并报告部分结果。

`format` 默认格式化全部项目输入，支持重复 `--file`。文件格式化的成功、预览和冲突结果统一用 `files` 返回 `path`、`change` 和补丁 `patch`；完整文件可通过 `source read --file <路径>` 获取。`--check` 和 `--dry-run` 互斥且都不写文件；stdin 模式独立于项目，文本输出格式化源码，JSON 将源码放在结果中。

`codegen` 在结构与数据有效后生成代码；`--dry-run` 预览，`--check` 只检查产物一致性。`build` 在全部业务规则通过后生成代码。所有目标准备完成后统一发布，发布失败返回实际写入文件、已恢复文件及恢复失败原因，恢复冲突保留外部修改。

## 输出与退出状态

默认输出终端文本，`--json` 输出单行 JSON；`--pretty` 直接启用缩进 JSON，便于人工查看。`--details` 控制信息量，与排版方式独立。参数、读取和执行失败也返回统一信封：`protocol_version`、`command`、`status`、`result`，以及适用的 `error_category`、`project`、`revision`、`data_scope`、`diagnostics`。

默认最多返回 50 条诊断；有诊断时，`diagnostics_summary` 给出 `total`、`returned`、`truncated`。`--details` 解除默认诊断上限，显式 `--limit` 仍生效。短修复建议放在 `help`，`--explain` 请求更完整的原因与实际值；可直接按 `code` 查询 `coflow explain`。输出截断不改变校验完整性、是否落盘或退出状态。

未提供的可选元数据和空的附加列表会省略；业务值中的 `null`、`None`、空字符串、空数组与空字典保持原样。JSON 请求错误尽量提供 `result.request_path`、从零开始的 `operation_index` 和 `target`，便于修正对应输入。源码位置从零开始，列使用 UTF-16 偏移。

分页截断与项目加载不完整分别报告。数据查询的 `data_scope` 默认返回 `complete`、`available_records`、`selected_file_count`、`incomplete_file_count`、`rejected_record_count` 和 `details_omitted`。`--details` 展开选中文件、不完整文件及原因和被拒绝记录列表；不读取数据的命令不提供该信息。

| 退出码 | 状态 |
| --- | --- |
| 0 | 完整成功 |
| 1 | 校验失败、状态检查不满足或结果不完整 |
| 2 | 参数或请求不合法 |
| 3 | 项目修订、游标、文件或预期旧值冲突 |
| 4 | I/O 或执行失败 |

列表零匹配为成功，精确目标不存在为校验失败。警告随完整成功结果返回。帮助和版本使用各自输出，`lsp` stdout 专用于语言服务。

## 安装与服务

```powershell
coflow init <dir>
coflow lsp --project <project>
coflow skill install -g
coflow skill status -g
coflow skill uninstall -g
coflow self-update
```

## 完整命令与参数

以下列出全部 53 个命令入口。命令前统一加 `coflow`。`必填` 表示必须提供该参数；参数组中的二选一要求另行列出。全局参数对每个命令可用，取值型参数后填写值，开关直接使用参数名。

| 全局参数 | 含义 |
| --- | --- |
| `--project` | 项目目录或 coflow.yaml 路径，省略时向父目录查找 |
| `--json` | 单行 JSON 输出 |
| `--pretty` | 缩进 JSON，隐含 --json |
| `--details` | 展开执行明细和诊断；显式 --limit 仍有效 |
| `--help` / `-h` | 查看当前命令帮助 |
| `--version` / `-V` | 查看 CLI 版本（根命令） |

### `init`

创建项目目录与初始配置。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `dir` | 可选 | 默认 `.` |

### `describe`

列出命令、参数与输入输出结构。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `command` | 可选 | 见下方参数说明 |
| `--schema` | 可选 | `input`、`output`、`both`；同时提供位置参数 `command` |

### `project inspect`

查看项目配置和输入输出范围。

无需专属参数。

### `project files`

列出实际纳入项目的文件。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--kind` | 可选 | `schema`、`data` |

### `project add-input`

向配置加入输入路径。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--dry-run` | 可选 | 开关 |
| `--rules` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--path` | 必填 | 见下方参数说明 |
| `--kind` | 必填 | `schema`、`data` |

### `project remove-input`

从配置移除输入路径，保留文件。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--dry-run` | 可选 | 开关 |
| `--rules` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--path` | 必填 | 见下方参数说明 |
| `--kind` | 必填 | `schema`、`data` |

### `context`

读取指定类型或记录的有界上下文。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--include-derived` | 可选 | 开关 |
| `--file` | 可选 | 见下方参数说明 |
| `--id` | 可选 | 见下方参数说明 |
| `--fields` | 可选 | 见下方参数说明 |
| `--where` | 可选 | 见下方参数说明 |
| `--sort` | 可选 | 见下方参数说明 |
| `--cursor` | 可选 | 见下方参数说明 |
| `--dimension` | 可选 | 见下方参数说明 |
| `--limit` | 可选 | 默认 `50`；最小 1 |
| `--type` | 必填 | 见下方参数说明 |
| `--examples` | 可选 | 默认 `3`；最小 0 |
| `--ref-limit` | 可选 | 默认 `10`；最小 0 |
| `--budget` | 可选 | 默认 `16000`；最小 1 |
| `--depth` | 可选 | 默认 `4`；最小 0 |

### `schema check`

检查当前或候选 CFT 声明。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--input` | 可选 | 见下方参数说明 |
| `--limit` | 可选 | 最小 1 |
| `--explain` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |

### `schema inspect`

查询类型、字段、默认值和相关声明。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--type` | 可选 | 见下方参数说明 |
| `--include-derived` | 可选 | 开关 |
| `--related` | 可选 | 开关 |
| `--depth` | 可选 | 默认 `4`；最小 0 |
| `--limit` | 可选 | 默认 `100`；最小 1 |
| `--field` | 可选 | 同时提供 `--type` |

### `schema symbols`

列出类型、字段、枚举、常量或规则符号。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--cursor` | 可选 | 见下方参数说明 |
| `--kind` | 可选 | `type`、`field`、`enum`、`const`、`rule` |
| `--limit` | 可选 | 默认 `50`；最小 1 |

### `schema refs`

查找声明符号的使用位置。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--cursor` | 可选 | 见下方参数说明 |
| `--symbol` | 必填 | 见下方参数说明 |
| `--limit` | 可选 | 默认 `50`；最小 1 |

### `source read`

读取源码全文或指定行范围。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--start-line` | 可选 | 最小 0 |
| `--end-line` | 可选 | 最小 0 |
| `--file` | 必填 | 见下方参数说明 |

### `source write`

替换已有源码文件。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--expected-fingerprint` | 可选 | 见下方参数说明 |
| `--dry-run` | 可选 | 开关 |
| `--rules` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--file` | 必填 | 见下方参数说明 |
| `--input` | 必填 | 见下方参数说明 |

### `source create`

创建源码文件并纳入项目。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--dry-run` | 可选 | 开关 |
| `--rules` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--file` | 必填 | 见下方参数说明 |
| `--input` | 必填 | 见下方参数说明 |

### `source delete`

删除源码文件。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--expected-fingerprint` | 可选 | 见下方参数说明 |
| `--dry-run` | 可选 | 开关 |
| `--rules` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--file` | 必填 | 见下方参数说明 |

### `data list`

按条件查询记录列表。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--type` | 可选 | 见下方参数说明 |
| `--include-derived` | 可选 | 开关 |
| `--file` | 可选 | 见下方参数说明 |
| `--id` | 可选 | 见下方参数说明 |
| `--fields` | 可选 | 见下方参数说明 |
| `--where` | 可选 | 见下方参数说明 |
| `--sort` | 可选 | 见下方参数说明 |
| `--cursor` | 可选 | 见下方参数说明 |
| `--dimension` | 可选 | 见下方参数说明 |
| `--limit` | 可选 | 默认 `50`；最小 1 |
| `--view` | 可选 | `source`、`effective`、`both`；默认 `effective` |

### `data get`

读取单条记录的显式值或有效值。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--fields` | 可选 | 见下方参数说明 |
| `--type` | 必填 | 见下方参数说明 |
| `--id` | 必填 | 见下方参数说明 |
| `--view` | 可选 | `source`、`effective`、`both`；默认 `effective` |
| `--dimension` | 可选 | 见下方参数说明 |

### `data search`

在记录 ID 和值中搜索文本。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--type` | 可选 | 见下方参数说明 |
| `--include-derived` | 可选 | 开关 |
| `--file` | 可选 | 见下方参数说明 |
| `--id` | 可选 | 见下方参数说明 |
| `--fields` | 可选 | 见下方参数说明 |
| `--where` | 可选 | 见下方参数说明 |
| `--sort` | 可选 | 见下方参数说明 |
| `--cursor` | 可选 | 见下方参数说明 |
| `--dimension` | 可选 | 见下方参数说明 |
| `--limit` | 可选 | 默认 `50`；最小 1 |
| `--text` | 必填 | 见下方参数说明 |
| `--view` | 可选 | `source`、`effective`、`both`；默认 `effective` |

### `data draft`

准备记录草稿与必填项说明。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--id` | 可选 | 见下方参数说明 |
| `--type` | 必填 | 见下方参数说明 |

### `data create`

从字段请求创建记录。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--dry-run` | 可选 | 开关 |
| `--rules` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--type` | 必填 | 见下方参数说明 |
| `--id` | 必填 | 见下方参数说明 |
| `--file` | 必填 | 见下方参数说明 |
| `--input` | 必填 | 见下方参数说明 |

### `data set`

设置显式字段值。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--expected-cfd` | 可选 | 见下方参数说明 |
| `--value-cfd` | 可选 | 见下方参数说明 |
| `--value-file` | 可选 | 见下方参数说明 |
| `--dry-run` | 可选 | 开关 |
| `--rules` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--type` | 必填 | 见下方参数说明 |
| `--id` | 必填 | 见下方参数说明 |
| `--path` | 必填 | 见下方参数说明 |

必须选择 `--value-cfd`、`--value-file` 中的一项。

### `data unset`

移除字段赋值并重新应用默认值检查。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--dry-run` | 可选 | 开关 |
| `--rules` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--type` | 必填 | 见下方参数说明 |
| `--id` | 必填 | 见下方参数说明 |
| `--path` | 必填 | 见下方参数说明 |

### `data rename`

修改记录 ID 并更新引用和覆盖。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--dry-run` | 可选 | 开关 |
| `--rules` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--type` | 必填 | 见下方参数说明 |
| `--id` | 必填 | 见下方参数说明 |
| `--new-id` | 必填 | 见下方参数说明 |

### `data delete`

删除记录及其覆盖。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--dry-run` | 可选 | 开关 |
| `--rules` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--type` | 必填 | 见下方参数说明 |
| `--id` | 必填 | 见下方参数说明 |

### `data transfer`

迁移记录到另一 CFD 文件。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--dry-run` | 可选 | 开关 |
| `--rules` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--type` | 必填 | 见下方参数说明 |
| `--id` | 必填 | 见下方参数说明 |
| `--to-file` | 必填 | 见下方参数说明 |

### `data refs`

查询传入或传出的记录引用。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--cursor` | 可选 | 见下方参数说明 |
| `--type` | 必填 | 见下方参数说明 |
| `--id` | 必填 | 见下方参数说明 |
| `--direction` | 可选 | `incoming`、`outgoing`、`both`；默认 `both` |
| `--limit` | 可选 | 默认 `50`；最小 1 |
| `--depth` | 可选 | 默认 `1`；最小 1；最大 64 |

### `dimension list`

列出维度。

无需专属参数。

### `dimension inspect`

查看维度及绑定字段。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--dimension` | 必填 | 见下方参数说明 |

### `dimension variants`

列出已声明变体。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--cursor` | 可选 | 见下方参数说明 |
| `--dimension` | 必填 | 见下方参数说明 |
| `--limit` | 可选 | 默认 `50`；最小 1 |

### `dimension get`

查看显式覆盖及有效值。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--dimension` | 必填 | 见下方参数说明 |
| `--variant` | 必填 | 见下方参数说明 |
| `--type` | 必填 | 见下方参数说明 |
| `--id` | 必填 | 见下方参数说明 |
| `--field` | 必填 | 见下方参数说明 |

### `dimension set`

设置字段的显式覆盖。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--file` | 可选 | 见下方参数说明 |
| `--path` | 可选 | 见下方参数说明 |
| `--value-cfd` | 可选 | 见下方参数说明 |
| `--value-file` | 可选 | 见下方参数说明 |
| `--dry-run` | 可选 | 开关 |
| `--rules` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--dimension` | 必填 | 见下方参数说明 |
| `--variant` | 必填 | 见下方参数说明 |
| `--type` | 必填 | 见下方参数说明 |
| `--id` | 必填 | 见下方参数说明 |
| `--field` | 必填 | 见下方参数说明 |

必须选择 `--value-cfd`、`--value-file` 中的一项。

### `dimension clear`

清除字段的显式覆盖。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--dry-run` | 可选 | 开关 |
| `--rules` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--dimension` | 必填 | 见下方参数说明 |
| `--variant` | 必填 | 见下方参数说明 |
| `--type` | 必填 | 见下方参数说明 |
| `--id` | 必填 | 见下方参数说明 |
| `--field` | 必填 | 见下方参数说明 |

### `dimension missing`

查找缺少显式覆盖的字段。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--type` | 可选 | 见下方参数说明 |
| `--include-derived` | 可选 | 开关 |
| `--file` | 可选 | 见下方参数说明 |
| `--id` | 可选 | 见下方参数说明 |
| `--fields` | 可选 | 见下方参数说明 |
| `--where` | 可选 | 见下方参数说明 |
| `--sort` | 可选 | 见下方参数说明 |
| `--cursor` | 可选 | 见下方参数说明 |
| `--field` | 可选 | 见下方参数说明 |
| `--dimension` | 必填 | 见下方参数说明 |
| `--variant` | 必填 | 见下方参数说明 |
| `--limit` | 可选 | 默认 `50`；最小 1 |

### `dimension diff`

比较两个变体或基础值与变体。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--type` | 可选 | 见下方参数说明 |
| `--include-derived` | 可选 | 开关 |
| `--file` | 可选 | 见下方参数说明 |
| `--id` | 可选 | 见下方参数说明 |
| `--fields` | 可选 | 见下方参数说明 |
| `--where` | 可选 | 见下方参数说明 |
| `--sort` | 可选 | 见下方参数说明 |
| `--cursor` | 可选 | 见下方参数说明 |
| `--from` | 可选 | 见下方参数说明 |
| `--to` | 可选 | 见下方参数说明 |
| `--from-base` | 可选 | 开关 |
| `--to-base` | 可选 | 开关 |
| `--field` | 可选 | 见下方参数说明 |
| `--dimension` | 必填 | 见下方参数说明 |
| `--limit` | 可选 | 默认 `50`；最小 1 |

必须选择 `--from`、`--from-base` 中的一项。

必须选择 `--to`、`--to-base` 中的一项。

### `stats project`

统计项目规模。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--type` | 可选 | 见下方参数说明 |
| `--include-derived` | 可选 | 开关 |
| `--file` | 可选 | 见下方参数说明 |
| `--id` | 可选 | 见下方参数说明 |
| `--fields` | 可选 | 见下方参数说明 |
| `--where` | 可选 | 见下方参数说明 |
| `--sort` | 可选 | 见下方参数说明 |
| `--cursor` | 可选 | 见下方参数说明 |
| `--dimension` | 可选 | 见下方参数说明 |
| `--limit` | 可选 | 默认 `50`；最小 1 |

### `stats records`

按类型或文件统计记录。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--type` | 可选 | 见下方参数说明 |
| `--include-derived` | 可选 | 开关 |
| `--file` | 可选 | 见下方参数说明 |
| `--id` | 可选 | 见下方参数说明 |
| `--fields` | 可选 | 见下方参数说明 |
| `--where` | 可选 | 见下方参数说明 |
| `--sort` | 可选 | 见下方参数说明 |
| `--cursor` | 可选 | 见下方参数说明 |
| `--dimension` | 可选 | 见下方参数说明 |
| `--limit` | 可选 | 默认 `50`；最小 1 |
| `--group-by` | 可选 | `type`、`file`；默认 `type` |

### `stats field`

统计字段值、集合或 Option 状态。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--type` | 可选 | 见下方参数说明 |
| `--include-derived` | 可选 | 开关 |
| `--file` | 可选 | 见下方参数说明 |
| `--id` | 可选 | 见下方参数说明 |
| `--fields` | 可选 | 见下方参数说明 |
| `--where` | 可选 | 见下方参数说明 |
| `--sort` | 可选 | 见下方参数说明 |
| `--cursor` | 可选 | 见下方参数说明 |
| `--option-layer` | 可选 | 最小 1 |
| `--dimension` | 可选 | 见下方参数说明 |
| `--limit` | 可选 | 默认 `50`；最小 1 |
| `--field` | 必填 | 见下方参数说明 |
| `--group-by` | 可选 | `value` |
| `--quantiles` | 可选 | 默认 `0.5,0.9,0.95,0.99` |
| `--metrics` | 可选 | 见下方参数说明 |

### `stats dimension`

统计显式覆盖率。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--type` | 可选 | 见下方参数说明 |
| `--include-derived` | 可选 | 开关 |
| `--file` | 可选 | 见下方参数说明 |
| `--id` | 可选 | 见下方参数说明 |
| `--fields` | 可选 | 见下方参数说明 |
| `--where` | 可选 | 见下方参数说明 |
| `--sort` | 可选 | 见下方参数说明 |
| `--cursor` | 可选 | 见下方参数说明 |
| `--field` | 可选 | 见下方参数说明 |
| `--dimension` | 必填 | 见下方参数说明 |
| `--variant` | 必填 | 见下方参数说明 |
| `--group-by` | 可选 | `all`、`type`、`file`；默认 `all` |
| `--limit` | 可选 | 默认 `50`；最小 1 |

### `stats refs`

统计记录引用。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--type` | 可选 | 见下方参数说明 |
| `--include-derived` | 可选 | 开关 |
| `--file` | 可选 | 见下方参数说明 |
| `--id` | 可选 | 见下方参数说明 |
| `--fields` | 可选 | 见下方参数说明 |
| `--where` | 可选 | 见下方参数说明 |
| `--sort` | 可选 | 见下方参数说明 |
| `--cursor` | 可选 | 见下方参数说明 |
| `--dimension` | 可选 | 见下方参数说明 |
| `--limit` | 可选 | 默认 `50`；最小 1 |
| `--direction` | 可选 | `incoming`、`outgoing`、`both`；默认 `both` |

### `check`

检查数据，并按选择执行业务规则。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--rules` | 可选 | 开关 |
| `--input` | 可选 | 见下方参数说明 |
| `--limit` | 可选 | 最小 1 |
| `--explain` | 可选 | 开关 |
| `--performance` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--rule` | 可选 | 与 `--rules` 互斥 |

### `rule list`

列出规则及其依赖。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--type` | 可选 | 见下方参数说明 |

### `rule inspect`

查看指定规则。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--rule` | 必填 | 见下方参数说明 |

### `explain`

解释错误码及修复方法。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `code` | 必填 | 见下方参数说明 |

### `apply`

校验并批量写入变更集。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--dry-run` | 可选 | 开关 |
| `--rules` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--input` | 必填 | 见下方参数说明 |

### `diff`

比较 Git 版本或候选变更。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--summary` | 可选 | 开关 |
| `--include-source` | 可选 | 开关 |
| `--type` | 可选 | 见下方参数说明 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--input` | 可选 | 与 `--base` 互斥 |
| `--view` | 可选 | `source`、`schema`、`record`、`field`、`dimension` |
| `--base` | 可选 | 默认 `HEAD` |

### `format`

格式化文件或标准输入。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--dry-run` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--check` | 可选 | 开关；与 `--dry-run` 互斥 |
| `--file` | 可选 | 见下方参数说明 |
| `--stdin` | 可选 | 开关；同时提供 `--language`；与 `--file`, `--project`, `--if-revision` 互斥 |
| `--language` | 可选 | `cft`、`cfd`；同时提供 `--stdin` |

`--stdin` 与文件、项目和修订参数互斥，并且必须提供 `--language`。文件模式可重复传入 `--file`，省略则格式化全部项目输入。

### `codegen`

生成、预览或检查代码。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--dry-run` | 可选 | 开关 |
| `--if-revision` | 可选 | 见下方参数说明 |
| `--check` | 可选 | 开关；与 `--dry-run` 互斥 |

### `build`

执行全部校验并生成代码。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--if-revision` | 可选 | 见下方参数说明 |

### `lsp`

运行标准输入输出语言服务器。

无需专属参数。

### `skill install`

安装配套 AI skills。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--global` | 可选 | 开关 |

`--global`（`-g`）选择全局安装；省略时操作项目内的安装。

### `skill uninstall`

移除已安装的配套 skills。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--global` | 可选 | 开关 |

`--global`（`-g`）选择全局安装；省略时操作项目内的安装。

### `skill status`

查看配套 skills 安装状态。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--global` | 可选 | 开关 |

`--global`（`-g`）选择全局安装；省略时操作项目内的安装。

### `self-update`

检查或安装 CLI 更新。

| 参数 | 要求 | 取值与默认值 |
| --- | --- | --- |
| `--check` | 可选 | 开关 |
| `--yes` | 可选 | 开关 |

`--check` 仅查询更新；`--yes` 安装时跳过交互确认。

## 参数说明

同名参数的用途如下，具体命令支持的参数及必填性以上表为准。

| 参数 | 含义与用法 |
| --- | --- |
| `dir` | init 的位置参数，创建项目的目录，默认当前目录 |
| `command` | describe 的位置参数，命令或组名，使用 `data.set` 这样的点分名称 |
| `code` | explain 的位置参数，完整错误码，例如 `DATA-011` |
| `--schema` | describe 的 JSON Schema 范围：input、output、both；要求指定叶子命令 |
| `--type` | 声明类型或记录类型名称 |
| `--include-derived` | 查询包含派生类型 |
| `--id` | 记录 ID |
| `--field` | 字段名；schema inspect 中需同时指定类型 |
| `--fields` | 逗号分隔的返回字段路径 |
| `--path` | 数据命令中为嵌套值路径；project add-input/remove-input 中为输入路径；dimension set 中为覆盖值内部路径 |
| `--file` | 项目相对源码路径；查询中筛选文件，写入中指定来源或目标 |
| `--kind` | project 命令为 schema/data；schema symbols 为 type/field/enum/const/rule |
| `--symbol` | 声明符号，例如 `Item` 或 `Item.price` |
| `--related` | schema inspect 同时返回被引用声明 |
| `--depth` | 上下文或引用展开深度；data refs 允许 1–64，默认 1 |
| `--examples` | context 中示例记录上限，默认 3 |
| `--ref-limit` | context 中引用候选记录上限，默认 10 |
| `--budget` | context 序列化结果的字符预算，默认 16000，必须大于零 |
| `--start-line` / `--end-line` | 源码读取范围，从零开始，包含起始行、不包含结束行 |
| `--view` | 数据查询为 source/effective/both；diff 为 source/schema/record/field/dimension |
| `--text` | 在记录 ID 与数据值中搜索的文本 |
| `--where` | 类型化布尔筛选条件，支持比较、逻辑及 contains/is_some/is_none/is_reachable |
| `--sort` | 逗号分隔字段路径；路径前加 `-` 表示降序 |
| `--limit` | 返回条目或诊断上限，正整数；各命令默认值见命令表 |
| `--cursor` | 上一页 next_cursor；保持查询条件及项目修订一致 |
| `--dimension` | 维度命令为维度名；记录查询为 `名称=变体`，可重复组合 |
| `--variant` | 所选维度内的变体名 |
| `--from` / `--to` | 维度差异的起始、目标变体 |
| `--from-base` / `--to-base` | 使用基础视角，分别与 --from / --to 二选一 |
| `--direction` | incoming/outgoing/both，查询或统计传入、传出或双向引用 |
| `--group-by` | records 按 type/file；dimension 按 all/type/file；field 按 value 分布 |
| `--metrics` | 逗号分隔统计指标，见下方列表 |
| `--option-layer` | 检查第几层 Option，从 1 开始 |
| `--quantiles` | 逗号分隔的 [0,1] 分位数，默认 0.5,0.9,0.95,0.99 |
| `--input` | JSON 请求文件或 source write/create 的源码内容文件；`-` 读取标准输入 |
| `--value-cfd` | 直接提供按目标字段类型解析的 CFD 表达式 |
| `--value-file` | 从文件读取 CFD 表达式；`-` 读取标准输入，与 --value-cfd 二选一 |
| `--expected-cfd` | 要求当前有效字段值与此 CFD 表达式相等 |
| `--expected-fingerprint` | 要求待替换或删除的文件内容指纹匹配 |
| `--if-revision` | 要求项目修订与上一次查询结果一致 |
| `--new-id` | 改名后的记录 ID |
| `--to-file` | 记录迁移目标 CFD 路径 |
| `--dry-run` | 校验并返回预览，不写入 |
| `--rules` | 执行全部业务规则 |
| `--rule` | 指定规则身份，可重复选择多条，与 --rules 互斥 |
| `--explain` | 返回诊断原因、实际值与规则解释 |
| `--performance` | 返回规则执行耗时与工作量 |
| `--base` | diff 的 Git 基线，默认 HEAD；与候选 --input 互斥 |
| `--summary` | diff 只返回计数和摘要 |
| `--include-source` | diff 同时返回完整源码 |
| `--stdin` | format 从标准输入读取，不使用项目 |
| `--language` | format 标准输入语言，cft 或 cfd |
| `--check` | 只检查状态，不写入；format/codegen 中与 --dry-run 互斥 |
| `--global` / `-g` | skills 操作使用全局目录 |
| `--yes` | self-update 安装更新时跳过交互确认 |

`--metrics` 支持 `count`、`min`、`max`、`sum`、`mean`、`quantiles`、`some`、`none`、`empty_count`、`length_distribution`、`length_min`、`length_max`。指标需与字段类型匹配；Option 层通过 `--option-layer` 选择。分位数按 nearest-rank 计算。

### 请求文件与标准输入

`source write/create --input` 读取完整源码文本；`apply`、候选 `check` / `schema check` / `diff` 的 `--input` 读取 JSON 变更集。`data create --input` 使用字段对象，例如：

```json
{
  "name": { "kind": "expression", "source": "\"铁剑\"" },
  "price": { "kind": "expression", "source": "100" }
}
```

完整请求结构可以用 `coflow describe data.create --schema input --json` 或 `coflow describe apply --schema input --json` 查询。一次调用最多只有一个参数读取 stdin。Shell 会先解析命令行；字符串、括号、空格等表达式应按所用 Shell 的规则引用，也可用 `--value-file` 避免转义。

### 语言服务与 AI 工具

`coflow lsp --project <目录>` 使用标准输入输出传输 LSP 消息，供支持 LSP 的编辑器启动；stdout 不用于普通命令 JSON。配套 skills 可通过 `coflow skill install -g` 安装，用 `coflow skill status -g` 查看状态，用 `coflow skill uninstall -g` 移除。CLI 查询与修改的机器接口使用本页的 JSON 输出和请求结构。
