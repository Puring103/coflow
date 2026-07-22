# CLI 命令参考

`coflow` CLI 用来初始化项目、检查 schema 和数据、导出配置产物、生成运行时代码，并提供面向编辑器和自动化工具的 schema/data 读写命令。

在源码仓库内开发时可以用：

```powershell
cargo run -- <command>
```

安装后直接使用：

```powershell
coflow <command>
```

下面示例统一使用 `coflow`。

查看 CLI 版本或任一层级的命令帮助：

```powershell
coflow --version
coflow --help
coflow <command> --help
```

`-V` 等价于 `--version`，`-h` 等价于 `--help`。

## 项目参数

大多数命令接受可选的 `CONFIG_OR_DIR`：

```powershell
coflow check
coflow check examples/rpg
coflow check examples/rpg/coflow.yaml
```

解析规则：

- 省略时，从当前目录查找 `coflow.yaml` 或 `coflow.yml`。
- 传入目录时，在该目录内查找 `coflow.yaml` 或 `coflow.yml`。
- 传入文件时，直接把该文件作为项目配置读取。

`coflow.yaml` 中的相对路径都以配置文件所在目录为根解析。

## 退出码与诊断

CLI 退出码用于脚本和 CI 判断命令是否成功：

| 退出码 | 含义 |
| --- | --- |
| `0` | 命令成功完成 |
| `1` | 命令失败，或产生使该命令结果不成功的诊断 |
| `2` | Clap 在执行命令前拒绝了命令行语法或参数值 |

会输出诊断的命令会尽量收集多个错误，再一起返回。只要存在诊断，`check`、`build`、`export`、`codegen` 这类命令就会返回非 `0`，并且不会写入对应产物。

所有命令默认输出适合终端阅读的 human 文本。支持 `--json` 的命令可显式切换为
machine-readable JSON，适合编辑器、CI 和自动化脚本。`cft check`、`check`、`schema`、
`data` 和 `skill` 的相关命令支持 `--json`；`build`、`export`、`codegen`、`clean` 和 `init`
只输出文本状态。

输出模式由具体命令在成功解析项目和输入后应用。配置文件无法定位/解析、stdin/patch 文件
无法读取等提前返回到顶层的错误统一使用 human diagnostics 写到 stderr，即使调用时带了
`--json`。

使用 `--json` 的查询命令会先输出 report 再用退出码表达结果：`schema inspect/files`、
`data sources/list` 在 report 含 diagnostics 时返回 `1`。`data get` 的显式 lookup/limit 错误
返回 `1`；成功生成 report 后，只要至少返回一条 record 就返回 `0`，即使 report 同时带有
项目 diagnostics；空 records 只有在 diagnostics 也为空时返回 `0`。

## 常用流程

新建项目：

```powershell
coflow init my-config
```

只检查 CFT schema：

```powershell
coflow cft check my-config
```

检查完整项目：

```powershell
coflow check my-config
```

导出数据并生成配置中的代码：

```powershell
coflow build my-config
```

只导出配置中的数据产物：

```powershell
coflow export my-config
```

只生成配置中的代码产物：

```powershell
coflow codegen my-config
```

## `init`

`init` 创建一个最小 coflow 项目：

```powershell
coflow init [DIR]
```

省略 `DIR` 时在当前目录创建。

该命令会创建：

- `coflow.yaml`
- `schema/`
- `data/`
- `generated/data/`
- `generated/csharp/`

生成的配置使用 JSON 数据输出、C# 代码输出、显式 `csharp-json` loader、命名空间
`Game.Config`，并带有空的 `sources: []`。空项目可以直接通过 `cft check` 和 `check`，也可以
执行只依赖 schema 的 `codegen`；添加实际类型和记录后，`build`/`export` 才会产生有
业务内容的数据表。

如果目标目录已经存在 `coflow.yaml`，命令会失败，不会修改已有项目。

## `cft check`

`cft check` 只编译项目中的 CFT schema，不加载数据源。

```powershell
coflow cft check [CONFIG_OR_DIR] [--json] [--stdin-path PATH]
```

示例：

```powershell
coflow cft check examples/rpg
coflow cft check examples/rpg --json
```

适用场景：

- 修改 `.cft` 后快速检查类型、枚举、默认值、继承、注解和 `check` 语法。
- 数据文件还没准备好时，先验证 schema。
- 编辑器集成通过 `--stdin-path` 检查未保存的 schema 内容。

`--stdin-path PATH` 会把标准输入当作指定 schema 文件的内容。`PATH` 必须能匹配当前项目 `schema` 配置展开后的 `.cft` 文件。

## `check`

`check` 运行完整项目校验，但不写产物。

```powershell
coflow check [CONFIG_OR_DIR] [--json]
```

示例：

```powershell
coflow check examples/rpg
coflow check examples/rpg --json
```

校验流程包括：

1. 读取并检查项目配置。
2. 发现并编译 CFT schema。
3. 解析数据源。
4. 加载 Excel、CSV、CFD 或其他 Provider 数据。
5. 校验记录类型、默认值和必填字段。
6. 解析 `&Type` 记录引用。
7. 执行 CFT `check {}` 业务校验。

建议在提交 schema 或数据变更前运行该命令。

`check` 不校验 exporter 或 codegen 专用选项，也不生成产物。启用了 C# codegen、MessagePack
exporter 或其他产物输出的项目，提交前应运行 `coflow build`，同时验证数据和产物配置。

## `build`

`build` 运行完整校验，然后导出数据，并按项目配置可选生成代码。

```powershell
coflow build [CONFIG_OR_DIR]
```

示例：

```powershell
coflow build examples/rpg
```

`build` 会生成 `outputs` 中的全部 target。所有项目配置、provider/loader 选择、schema、数据加载、
引用解析和 `check` 全部通过后才会替换输出目录。任一 target 失败时，全部旧输出保持不变；
成功时 data、code 和 `@idAsEnum` 状态会一起更新。

## `export`

`export` 只导出数据，不生成运行时代码：

```powershell
coflow export [CONFIG_OR_DIR]
```

`export` 会导出 `outputs` 中的全部 data target。每个 target 使用自己的 `data.type`、`data.dir`
和 provider options；默认 CLI 提供 `json` 和 `messagepack` exporter，当前应用也可以注册其他
exporter。全部 target 会作为一次操作发布，任一 target 失败时不会替换任何 data 输出。

JSON target 配置示例：

```yaml
outputs:
  data:
    type: json
    dir: generated/data
```

输出文件名为 `<TypeName>.json`。

MessagePack target 配置示例：

```yaml
outputs:
  data:
    type: messagepack
    dir: generated/data
```

输出文件名为 `<TypeName>.msgpack`。

### 输出目录

`outputs.data.dir` 是导出成功后消费者直接读取的目录。命令成功信息输出每个 target 的目录。
输出目录由 Coflow 完整接管，不要在其中放置手写文件。只有全部产物生成并验证成功后才会
替换输出目录；失败时旧输出保持不变。

## `clean`

```powershell
coflow clean [CONFIG_OR_DIR]
```

清除不再使用的历史构建数据和中断遗留的临时文件。当前输出保持不变，清理后无需重新构建。
如果当前构建状态不完整，命令会失败而不会继续删除。没有可清理内容时计数为 0；成功信息
会给出删除的历史版本数和临时条目数。

## `codegen`

`codegen` 只发布项目配置中的运行时代码：

```powershell
coflow codegen [CONFIG_OR_DIR]
```

`codegen` 会生成 `outputs` 中全部配置了 `code` 的 target。每个 target 使用自己的 `code.type`、
`code.dir` 和 provider options；默认 CLI 提供 `csharp` codegen，当前应用也可以注册其他 codegen。
全部 target 会作为一次操作发布，任一 target 失败时不会替换任何 code 输出。该命令是
schema-only，不要求配置的数据源存在；公共代码和 loader 都从完整 schema 生成。

C# target 配置示例：

```yaml
outputs:
  data:
    type: json # 或 messagepack
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
```

同一 target 的 `data.type` 会影响生成的 C# loader；显式 `loader.type` 必须匹配该组合：

| 数据输出类型 | C# loader |
| --- | --- |
| `json` | Newtonsoft.Json loader |
| `messagepack` | MessagePack-CSharp loader |

C# 代码会写入 target 的 `code.dir`，命令成功信息输出该目录。
生成失败时旧目录保持不变。

对于 `@idAsEnum`，单独运行 `codegen` 会复用已有编号；干净 clone 可以从应提交到版本库的
`coflow.enum.lock.json` 恢复编号。该命令不会根据当前数据新增 enum variant；需要补全 variant
时，使用 `coflow build`。

## `lsp`

`lsp` 启动 Coflow language server，供编辑器集成使用。

```powershell
coflow lsp [CONFIG_OR_DIR]
```

language server 使用 schema-only 项目加载，不要求数据源文件存在。

它覆盖 CFT 和 CFD 相关编辑能力，包括：

- schema 诊断。
- CFT 关键字、primitive、类型名、enum variant、annotation、字段、默认值和 check 内建函数补全。
- hover 和 go-to-definition。
- document symbols。
- document formatting。
- semantic tokens。

## `schema`

`schema` 子命令面向自动化工具、编辑器和 AI agent，用来读取或写入 CFT schema。

### `schema inspect`

输出编译后的 schema 视图。

```powershell
coflow schema inspect [CONFIG_OR_DIR] [--type TYPE] [--include-derived] [--json]
```

示例：

```powershell
coflow schema inspect examples/rpg
coflow schema inspect examples/rpg --type Item
coflow schema inspect examples/rpg --type Reward --include-derived
coflow schema inspect examples/rpg --json
```

使用 `--json` 时输出以下结构：

- `types`：`module`、类型名、父类型，abstract/sealed/struct/singleton 标记、可选
  `id_as_enum` 名称，以及包含继承字段的 `fields`。
- `fields`：字段名、结构化 `ty`、`has_default`、结构化 `default`、`is_expand` 和可选
  dimension name/bucket。当前输出不包含注释、annotation 列表或原始类型文本。
- `enums`：`module`、枚举名、`is_flag`，以及带显式整数值的 variants。
- `consts`：`module`、常量名和带 `kind`/`value` 的结构化常量值。
- `dimensions`：维度名、配置 variants，以及使用该维度的 declaring type/field。
- `diagnostics`：schema 编译或项目配置诊断。

`--type TYPE` 只过滤 `types`；`enums` 和 `consts` 仍会完整输出，方便工具解析字段类型、枚举值和常量。`--include-derived` 会同时输出可赋值给该类型的派生类型。

### `schema files`

输出参与本次 schema 编译的 CFT 文件源码。

```powershell
coflow schema files [CONFIG_OR_DIR] [--json]
```

示例：

```powershell
coflow schema files examples/rpg
coflow schema files examples/rpg --json
```

使用 `--json` 时形状为 `{"files":[{"module":"...","source":"..."}],"diagnostics":[]}`。该命令
适合读取 `schema inspect` 不保留的原始注释、注解、类型文本、默认值、字段顺序和 `check` 块。

### `schema write-file`

从标准输入写入项目配置中的 `.cft` 文件。

```powershell
coflow schema write-file [CONFIG_OR_DIR] --file FILE [--dry-run] [--check] [--json]
```

示例：

```powershell
Get-Content schema/main.cft | coflow schema write-file examples/rpg --file schema/main.cft --check
Get-Content schema/main.cft | coflow schema write-file examples/rpg --file schema/main.cft --dry-run --check
```

限制：

- `--file` 必须是当前项目 `schema` 配置包含的 `.cft` 文件。
- 不能写入数据文件、输出目录或未配置的任意路径。
- `--dry-run` 不落盘，只报告差异和检查结果。
- `--check` 总是先把标准输入作为目标 module 的内存 replacement 编译；随后非 dry-run
  模式仍会写入同一份文本，dry-run 不落盘。它不加载数据源、不同步表头，也不执行项目级
  项目数据或 `check {}` 校验。

`--check` 发现 schema 诊断时命令返回 `1`，但诊断不阻止非 dry-run 写入。使用 `--json` 时报告字段为
`file`、`written`、`dry_run`、`changed`、`check_ok` 和 `diagnostics`；未传 `--check` 时
`check_ok` 为 `null`。

## `data`

`data` 子命令面向自动化工具、编辑器和 AI agent，用来读取数据源、读取记录、创建数据文件、同步表头或修改数据。

### `data sources`

输出解析后的数据源、provider id、可用操作和发现的 record 类型。

```powershell
coflow data sources [CONFIG_OR_DIR] [--json]
```

示例：

```powershell
coflow data sources examples/rpg
coflow data sources examples/rpg --json
```

该命令会加载完整项目数据，因此要求数据源文件存在。
输出中的 `diagnostics` 会包含数据加载、记录校验和 CFT `check {}` 诊断。
使用 `--json` 时报告包含 `sources` 和 `diagnostics`；每个 source 包含 `file`、`provider`、
`capabilities` 和该文件实际发现的去重 `types`。`capabilities` 表示该具体 source 当前支持的
操作。

### `data list`

输出轻量 record 索引，不包含完整字段值。

```powershell
coflow data list [CONFIG_OR_DIR] [--type TYPE] [--file FILE] [--limit N] [--offset N] [--json]
```

示例：

```powershell
coflow data list examples/rpg --type Item
coflow data list examples/rpg --file data/items.cfd --limit 20
```

每条记录包含 record type、record key、来源文件和 provider。
输出中的 `diagnostics` 会包含数据加载、记录校验和 CFT `check {}` 诊断。
未传 `--limit` 时 `data list` 返回 offset 后的全部匹配记录。使用 `--json` 时报告包含 `records` 和
`diagnostics`；每项记录形如
`{"record":{"actual_type":"Item","key":"sword"},"file":"...","provider":"..."}`。

### `data get`

输出完整 record 数据。

```powershell
coflow data get [CONFIG_OR_DIR] [TYPE.KEY] [--type TYPE] [--file FILE] [--keys a,b] [--limit N] [--offset N] [--all] [--json]
```

示例：

```powershell
coflow data get examples/rpg Item.sword_fire
coflow data get examples/rpg --type Item --keys sword_fire,staff_ice
coflow data get examples/rpg --type Item --limit 50
coflow data get examples/rpg --type Item --all
```

没有指定单条 `TYPE.KEY` 时，如果匹配记录超过默认安全上限，需要显式传入 `--limit` 或 `--all`。
输出中的 `diagnostics` 会包含数据加载、记录校验和 CFT `check {}` 诊断。

默认安全上限是 100；`--offset` 本身不能解除该限制。`--all` 忽略 `--limit`，返回 offset
之后的全部匹配项。`--keys` 以逗号分隔并在分页前过滤。显式 `TYPE.KEY` 找不到时报告
`DATA-NOT-FOUND`；若该记录存在但被额外的 `--type`/`--file` 过滤掉，则返回空 records，而不报告 not-found。
使用 `--json` 时报告包含 `records` 和 `diagnostics`，每个完整记录包含 `record`、`file`、`provider` 和
按字段名组织的结构化 `fields`。

只有一个位置参数时，已存在路径、包含 `/` 或 `\` 的文本以及 `.yaml`/`.yml` 名称按
`CONFIG_OR_DIR` 解析；其他包含点号的 `TYPE.KEY` 形式按记录 selector 解析。需要同时指定项目
和单条记录时使用两个位置参数。

### `data create-file`

创建数据文件。

```powershell
coflow data create-file [CONFIG_OR_DIR] --file FILE [--type TYPE] [--provider cfd|csv|excel] [--sheet SHEET] [--json]
```

示例：

```powershell
coflow data create-file examples/rpg --file data/items.csv --type Item --provider csv
coflow data create-file examples/rpg --file data/items.cfd --provider cfd
```

规则：

- `--provider` 省略时，根据当前应用中 provider 提供的表格文件类型推断。默认 CLI 的映射
  是 `.cfd`、`.csv`、`.xlsx`。显式值使用 provider id，也接受无歧义的别名（内置 `xlsx`
  是 `excel` 的别名）。
- `.csv` 和 `.xlsx` 会按解析出的具体 CFT type 创建 key 列和继承后的字段表头。`--type`
  省略时可由配置中的 sheet/type 映射推断；无法唯一推断时必须显式提供。abstract type 被拒绝。
- `.cfd` 创建空文件，不写表头。
- CSV 和 CFD 拒绝覆盖已存在文件；Excel 在文件不存在时创建 workbook，在已有 `.xlsx`
  中新增 `--sheet`（省略时使用解析出的 type 名），并拒绝同名 sheet。
- 目标不必预先列为单文件 source；如果它位于已配置目录 source 内，会复用该 source 的
  provider options，否则按未配置目标解析。显式 `--provider` 不能与已配置的
  `source.type` 冲突。

使用 `--json` 时报告包含 `file`、`provider`、可选 `sheet`/`type`、`headers`、`added`、
`removed` 和 `diagnostics`。

使用 Coflow 库的应用可以通过自定义 provider 增加可识别的 source 文件类型。要让新类型也
支持 `data create-file`、`data create-table` 或 `data sync-header`，该 provider 还必须提供
对应的创建或表头同步能力；仅支持读取不会自动获得这些命令的写入能力。配置文件不能直接
定义或安装 provider。

### `data create-table`

使用 `--source` 参数创建数据文件或表，并按当前 schema 写入表头。它与
`data create-file` 使用相同的 provider、type 和 sheet 解析规则，主要用于表达“在 workbook 中建表”。

```powershell
coflow data create-table [CONFIG_OR_DIR] --source SOURCE [--type TYPE] [--provider PROVIDER] [--sheet SHEET] [--json]
```

示例：

```powershell
coflow data create-table examples/rpg --source data/gameplay.xlsx --type Item --sheet Item
```

规则：

- `--source` 是项目相对文件；provider 推断、type/sheet 映射和配置复用规则与
  `data create-file` 相同。
- Excel 会创建缺失 workbook 或在已有 workbook 中追加 sheet，并拒绝同名 sheet；CSV 和
  CFD 会创建新文件并拒绝覆盖。
- `--sheet` 只对 sheet-addressed provider 有实际寻址意义；CSV 将 sheet 当作映射标签，CFD
  是 document-addressed。

### `data sync-header`

按当前 schema 同步数据文件的顶层字段或表头。

```powershell
coflow data sync-header [CONFIG_OR_DIR] --file FILE --type TYPE [--provider cfd|csv|excel] [--sheet SHEET] [--json]
```

示例：

```powershell
coflow data sync-header examples/rpg --file data/items.csv --type Item
coflow data sync-header examples/rpg --file data/items.cfd --type Item
```

行为：

- `.csv` 和 `.xlsx` 会重写表头，保留同名列数据，新增列填空，删除 schema 中不存在的列。
- `.xlsx` 中目标 sheet 缺失时会创建该 sheet；目标 workbook 文件本身必须已存在。
- `.cfd` 会重写匹配 `--type` 的顶层记录字段，保留仍存在字段的源码值，新增字段写入 schema 默认值或类型默认值。
- 支持 `.cfd`、`.csv` 和 `.xlsx` 文件。

命令会先加载并校验全部 source。schema 编译失败，或发现重复 header、重复 key 列、多个
header 映射到同一字段时，不会同步；其他已有的数据或 `check {}` 诊断不会单独阻止本次操作。
该命令不会生成或重写维度托管文件。使用 `--json` 时报告字段与 `data create-file` 相同。

### `data write-file`

从标准输入重写项目配置中的 CFD 文件。

```powershell
coflow data write-file [CONFIG_OR_DIR] --file FILE [--dry-run] [--check] [--json]
```

示例：

```powershell
Get-Content data/items.cfd | coflow data write-file examples/rpg --file data/items.cfd --check
Get-Content data/items.cfd | coflow data write-file examples/rpg --file data/items.cfd --dry-run
```

限制：

- 只接受精确小写 `.cfd` 文件。
- `--file` 必须位于项目配置中的 CFD source 覆盖范围内。
- 不能写入表格文件、输出目录、显式非 CFD provider source 或未配置路径。
- `data write-file` 不创建新 source。新增 CFD 文件应先用 `data create-file --provider cfd`
  创建并纳入可写范围，再用 `data write-file` 重写内容。
- `--dry-run` 不落盘，只比较目标文件内容并输出报告。
- `--check` 在非 dry-run 写入后运行完整项目校验。

`--dry-run --check` 不运行检查，报告中的 `check_ok` 为 `null`。非 dry-run 模式下，如果
`--check` 发现诊断，文件已经写入且命令返回 `1`，调用方需要根据报告继续修正。检查不会
生成或重写维度托管文件。使用 `--json` 时报告字段与 `schema write-file` 相同：
`file`、`written`、`dry_run`、`changed`、`check_ok` 和 `diagnostics`。

### `data patch`

应用批量数据补丁。

```powershell
coflow data patch [CONFIG_OR_DIR] --patch JSON [--json]
coflow data patch [CONFIG_OR_DIR] --patch-file PATCH_FILE [--json]
coflow data patch [CONFIG_OR_DIR] --stdin [--json]
```

示例：

```powershell
coflow data patch examples/rpg --patch '{"ops":[{"op":"set_field","record":{"type":"Item","key":"sword"},"path":[{"kind":"field","value":"price"}],"value":125}]}'
coflow data patch examples/rpg --patch-file patch.json
Get-Content patch.json | coflow data patch examples/rpg --stdin
```

patch JSON 示例：

```json
{
  "stop_on_write_error": true,
  "ops": [
    {
      "op": "insert_record",
      "file": "data/items.cfd",
      "type": "Item",
      "key": "steel_sword",
      "fields": {
        "name": "Steel Sword",
        "price": 250
      }
    },
    {
      "op": "set_field",
      "record": { "type": "Item", "key": "steel_sword" },
      "path": [{ "kind": "field", "value": "rarity" }],
      "value": "Rare"
    },
    {
      "op": "rename_record",
      "record": { "type": "Item", "key": "steel_sword" },
      "new_key": "steel_blade"
    }
  ]
}
```

支持的操作：

| 操作 | 说明 |
| --- | --- |
| `insert_record` | 新增顶层记录，必须指定 `file` |
| `set_field` | 修改字段路径，`file` 可作为来源 guard |
| `set_dimension_value` | 按 owner record、field、dimension 和 variant 写入维度值 |
| `clear_dimension_value` | 清除维度值，使该 variant 回到 missing 状态 |
| `rename_record` | 重命名记录 key，并同步引用 |
| `delete_record` | 删除记录，`file` 可作为来源 guard |

`insert_record.sheet` 可在同一 Excel workbook 中选择目标 sheet。`materialization` 默认为
`minimal`，只持久化调用方字段以及写入所必需的值；可设为 `editable_shape`，让 Coflow
补齐可安全构造的字段。必填 ref、无法选择具体派生类型的 abstract
object 和递归必填 object 仍必须由调用方显式提供。

`set_field`、`rename_record` 和 `delete_record` 的可选 `file` 是来源 guard：record 当前来源与
它不一致时拒绝操作，而不是把 record 移动到该文件。

维度操作保持 record selector 形态，不使用 generated type 或 storage record 地址：

```json
{
  "op": "set_dimension_value",
  "coordinate": {
    "record": { "type": "Item", "key": "potion" },
    "field": "name",
    "dimension": "language",
    "variant": "zh",
    "path": []
  },
  "value": "治疗药水"
}
```

`path` 是相对于整个维度字段值的可选嵌套路径。省略 `expected` 等价于
`{"kind":"any"}`，保持无条件写入；条件写入可使用 `{"kind":"missing"}`，或使用
`{"kind":"value","value":{"kind":"json","value":...}}` 比较一个 JSON mutation
value。expected state 已变化时报告 `MUTATION-DIMENSION-STALE`，且不修改维度文件。
missing 和 explicit `null` 是不同状态：`clear_dimension_value` 产生 missing，写入 JSON
`null` 产生 explicit null。

重命名或删除 owner record 时，对应的维度数据会同步更新；重命名保留已有 variant 值。

`set_field.path` 使用和编辑器相同的路径段：

```json
[
  { "kind": "field", "value": "resistances" },
  { "kind": "dict_key", "value": "Element.Fire" }
]
```

数组下标使用 `{ "kind": "index", "value": 0 }`。`dict_key` 的 `value` 是运行时路径文本：字符串 key 带引号（如 `"north"`），整数 key 写数字文本（如 `1`），枚举 key 写 `Enum.Variant`（如 `Element.Fire`）。

patch value 支持普通 JSON 值，也支持以下特殊对象：

```json
{ "$ref": "sword_01" }
{ "$ref": { "key": "sword_01" } }
{ "$type": "ItemReward", "item": { "$ref": "sword_01" }, "count": 1 }
{ "$dict": [{ "key": "Fire", "value": 10 }] }
```

`$ref` 只写 record key，目标类型来自被写入字段的 CFT 类型（例如 `&Item`、`[&Item]` 或 `{string: &Item}`）。

批量 patch 通过配置的 provider 修改原始数据源。无法完成整批写入时，已经产生的修改会被
撤销，报告中的 `applied` 为空，调用方仍可读取修改前的数据。

`stop_on_write_error` 默认为 `true`：任一操作在写入前验证失败时停止整批，未执行的操作保留在
`remaining_ops`。设为 `false` 时，单项验证错误可记录到 `failed` 后继续处理其他有效操作；
一旦实际写入失败，整批仍会撤销。

整批成功后只重新校验一次项目。最终 `diagnostics` 同时包含写入和项目校验诊断，`check_ok`
根据完整诊断计算。`affected_files` 给出实际写入并去重后的项目 source 路径。

使用 `--json` 时报告字段为：

| 字段 | 含义 |
| --- | --- |
| `write_ok` | 全部请求操作是否都通过验证并成功写入。 |
| `check_ok` | 写入后的项目是否没有 error diagnostics。仅业务 check 失败时，写入仍可能保留。 |
| `applied` | 已应用项的原始 index、操作名、可选 record/file 和 provider outcome。 |
| `failed` | 失败项的原始 index、操作名和该项 diagnostics。 |
| `affected_files` | 实际改写并去重后的项目 source 路径。 |
| `remaining_ops` | 从首个失败 index 开始的原始 request suffix；`stop_on_write_error: false` 时其中可能包含随后已应用的项，调用方应结合 `applied`/`failed` 判断。为空时省略该 JSON 字段。 |
| `diagnostics` | 写入诊断和写入后的项目诊断。 |

## `skill`

`skill` 管理由 Coflow CLI 内置的 AI agent skills，不依赖 Node.js、`npm` 或网络。

安装到当前 Coflow 项目：

```powershell
coflow skill install [CONFIG_OR_DIR] [--json]
```

目标目录为项目根目录下的 `.agents/skills/`。安装包含 `coflow-workflow`、
`coflow-schema` 和 `coflow-data`，重复运行会用当前 CLI 内置版本替换已有副本。

安装到当前用户：

```powershell
coflow skill install -g [--json]
```

全局安装始终写入 `$HOME/.agents/skills/`，并探测 Claude Code、Cursor、Gemini CLI、
GitHub Copilot、OpenCode 和 Windsurf 的专用全局目录。安装记录保存在
`$HOME/.coflow/skill-installs.json`，供状态查询和安全卸载使用。

查询状态：

```powershell
coflow skill status [CONFIG_OR_DIR] [--json]
coflow skill status -g [--json]
```

三个操作都可加 `--json`，输出 `operation`、`scope`、`bundle_version` 和 `targets`；每个
target 包含 `path`、识别出的 `agents` 和 `installed`。卸载命令为：

```powershell
coflow skill uninstall [CONFIG_OR_DIR] [--json]
coflow skill uninstall -g [--json]
```

项目卸载只删除项目中的三个 Coflow skill。全局卸载只访问已知 agent 目标，并删除
三个 Coflow skill，不删除 agent 的共享 skills 根目录或其他来源安装的 skill。

## 命令矩阵

| 命令 | 需要 schema | 需要数据源 | 校验记录 | 执行 CFT check | 写入文件 |
| --- | --- | --- | --- | --- | --- |
| `init` | 否 | 否 | 否 | 否 | 创建项目骨架 |
| `cft check` | 是 | 否 | 否 | 否 | 否 |
| `lsp` | 是 | 否 | 否 | 否 | 否 |
| `schema inspect` | 是 | 否 | 否 | 否 | 否 |
| `schema files` | 是 | 否 | 否 | 否 | 否 |
| `schema write-file` | 是 | 否 | 否 | 否（`--check` 只编译 schema） | 写 `.cft` |
| `data sources` | 是 | 是 | 是 | 是 | 否 |
| `data list` | 是 | 是 | 是 | 是 | 否 |
| `data get` | 是 | 是 | 是 | 是 | 否 |
| `data create-file` | 是 | 否 | 否 | 否 | 创建数据文件 |
| `data create-table` | 是 | 否 | 否 | 否 | 创建表格 sheet/table |
| `data sync-header` | 是 | 是 | 是 | 是 | 更新数据文件 |
| `data write-file` | 是 | 是 | 可选 | 可选 | 写 `.cfd` |
| `data patch` | 是 | 是 | 是 | 是 | 修改数据源 |
| `skill install/uninstall` | 否；项目模式只解析配置 | 否 | 否 | 否 | 写入或删除 Coflow skills |
| `skill status` | 否；项目模式只解析配置 | 否 | 否 | 否 | 否 |
| `check` | 是 | 是 | 是 | 是 | 否 |
| `build` | 是 | 是 | 是 | 是 | 数据和可选代码 |
| `clean` | 否 | 否 | 否 | 否 | 删除历史构建数据和临时文件 |
| `export` | 是 | 是 | 是 | 是 | 全部 data target |
| `codegen` | 是 | 否 | 否 | 否 | 全部 code target |
