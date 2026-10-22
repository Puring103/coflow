# CLI 命令

CLI 支持查询、编辑、校验、差异预览和代码生成。项目命令使用 `--project <目录或 coflow.yaml>`；省略时从当前目录向父目录查找项目。项目文件路径相对项目根目录，请求文件路径相对调用目录。

## 查询与上下文

```powershell
coflow describe --json
coflow describe data.set --json
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

`describe` 返回命令参数、默认值、条件默认值、依赖与互斥条件、值格式与数值范围、输入输出 JSON Schema、退出状态和示例。`schema inspect` 返回声明、默认值、继承关系和源码位置；字段查询提供枚举、引用和具体对象类型候选。`context` 聚合声明、规则、记录示例与引用候选，支持输出预算并报告省略项。

`data get --view source|effective|both` 分别查询显式赋值、有效值或两者。`origin` 区分基础赋值（`explicit_base`）、Schema 默认值（`schema_default`）和维度覆盖（`dimension_override`）。显式 `None` 保留赋值来源，省略带 `= None` 的字段属于 Schema 默认值；穿过 None 的路径则返回 `origin: "unreachable"`、`reachable: false`，没有来源位置或表达式。字符串模板表达式和渲染信息分别返回。

列表共享 `--type`、`--include-derived`、`--file`、`--id`、`--where`、`--sort`、`--dimension name=variant`、`--limit` 和 `--cursor`。列表默认每页 50 条，返回 `items`、`total`、`truncated` 与 `next_cursor`。项目变化后重新从第一页查询。聚合计算覆盖全部匹配记录，分页限制输出行。

`--where` 支持比较、布尔组合和 `contains`、`is_some`、`is_none`、`is_reachable`。`--sort` 使用逗号分隔路径，前缀 `-` 表示降序。文本字段路径支持 `field.child`、`items[0]` 和 `labels["a.b"]`。

## 修改与预览

```powershell
coflow data draft --type Item --id new_sword --json
coflow data create --type Item --id new_sword --file data/items.cfd --input fields.json
coflow data set --type Item --id sword --path price --value-cfd 100
coflow data unset --type Item --id sword --path description
coflow data rename --type Item --id sword --new-id iron_sword
coflow data transfer --type Item --id sword --to-file data/weapons.cfd
coflow data delete --type Item --id obsolete --dry-run --json
coflow source write --file schema/main.cft --input candidates/main.cft --dry-run --json
coflow source create --file data/new.cfd --input candidates/new.cfd
coflow source delete --file data/obsolete.cfd --dry-run
coflow project add-input --kind data --path data/localization
coflow project remove-input --kind data --path data/localization --dry-run
coflow apply --input changes.json --dry-run --json
coflow apply --input changes.json --rules --json
```

草稿区分声明默认值、类型默认值与待输入字段，以 `complete` 和 `required_inputs` 表达完整性。`set` 建立显式赋值，`unset` 移除赋值并应用默认值或缺失检查。相同的已有显式赋值保持源码和时间戳。改名更新已识别引用、维度目标和稳定枚举映射；迁移保留覆盖的来源文件。

写入先构建候选项目，结构与数据校验通过后统一发布；`--rules` 额外执行业务规则。`--dry-run` 返回校验与差异，保持正式文件原状。正式写入前检查外部修改，冲突时返回退出码 3。结果报告操作状态、涉及文件、实际写入、差异与恢复结果。

写入结果包含匹配记录数、改变记录数与改变字段数，以及默认值物化和引用、覆盖、输入配置、稳定映射的自动更新摘要。差异区分显式赋值、有效值和插值渲染变化。

`apply` 支持两种变更集：`kind: files` 的 `changes` 描述源码和输入调整；`kind: operations` 的 `ops` 描述记录、集合及维度操作。操作按顺序使用当前候选状态，引用在全部操作结束后检查。数组插入、删除、移动和字典设置、删除通过操作模式提交。请求的准确结构见 `coflow describe apply --json`。

```json
{
  "protocol_version": 1,
  "kind": "operations",
  "ops": [{
    "op": "set_field",
    "record": {"type": "Item", "id": "sword"},
    "path": [{"kind": "field", "name": "price"}],
    "value": {"kind": "expression", "source": "100"}
  }]
}
```

值输入使用 CFD 表达式或 `{"kind":"model","value":...}`。类型化模型值保留 int64 十进制字符串、有限浮点、每层 `some`/`none`、枚举身份和数值、引用身份、对象实际类型、集合及显式模板源码。枚举的 `variant` 为单一命名成员名，组合值为空。查询模型值可以用于结构化写入，渲染与来源信息保持只读。`--value-file` 读取 CFD 表达式，`-` 表示 stdin；每次调用只能使用一个 stdin 载体。

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

`schema check` 检查 Schema；`check` 检查结构、字段、引用和维度；`check --rules` 执行全部业务规则。重复 `--rule` 执行选定规则的并集，与 `--rules` 互斥。候选检查使用与 `apply` 相同的请求并保持只读。规则使用稳定名称，类型内规则形如 `Item::ValidPrice`，项目规则使用自己的名称。`results` 汇总每个目标的整条规则，`tasks` 提供语句执行明细；状态区分通过、条件未触发、业务失败、求值错误、未执行和预算耗尽。`complete` 表示是否完成执行，诊断输出限制保留完整计数。

`diff` 默认比较 Git HEAD 与当前项目，支持提交、分支及候选变更。`--view source|schema|record|field|dimension` 选择源码、Schema、记录、字段或维度视图；省略时返回全部视图。`--summary` 输出计数与对象摘要，`--include-source` 请求完整源码。语义不可用时保留源码差异并报告部分结果。

`format` 默认格式化全部项目输入，支持重复 `--file`。`--check` 和 `--dry-run` 互斥且都不写文件；stdin 模式独立于项目，文本输出格式化源码。

`codegen` 在结构与数据有效后生成代码；`--dry-run` 预览，`--check` 只检查产物一致性。`build` 在全部业务规则通过后生成代码。所有目标准备完成后统一发布，发布失败返回实际写入文件、已恢复文件及恢复失败原因，恢复冲突保留外部修改。

## 输出与退出状态

默认输出终端文本，`--json` 输出单个结果对象，包含 `protocol_version`、`command`、`status`、`error_category`、`project`、`result`、`data_scope` 和 `diagnostics`。参数、读取和执行失败也使用该信封。源码范围从零开始，列使用 UTF-16 偏移。分页截断与项目加载不完整分别报告。数据查询的 `data_scope` 返回加载完整性、可用记录数量、选中数据文件、加载不完整文件及原因和被拒绝的记录；不读取数据的命令该字段为空。

| 退出码 | 状态 |
| --- | --- |
| 0 | 完整成功 |
| 1 | 校验失败、状态检查不满足或结果不完整 |
| 2 | 参数或请求不合法 |
| 3 | 文件或预期旧值冲突 |
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
