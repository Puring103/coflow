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
| 非 `0` | 命令失败，或产生阻止继续执行的诊断 |

会输出诊断的命令会尽量收集多个错误，再一起返回。只要存在诊断，`check`、`build`、`export`、`codegen` 这类命令就会返回非 `0`，并且不会写入对应产物。

支持 `--json` 或默认 JSON 输出的命令适合接入编辑器、CI 和自动化脚本；支持 `--human` 的命令适合人工在终端阅读。

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

只导出 JSON 或 MessagePack：

```powershell
coflow export json my-config
coflow export messagepack my-config
```

只生成 C# 代码：

```powershell
coflow codegen csharp my-config
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

生成的配置使用 JSON 数据输出、C# 代码输出、命名空间 `Game.Config`，并带有空的 `sources: []`。运行 `check`、`build` 或 `export` 前，需要先添加 CFT schema 和数据 source。

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
5. 构建 DataModel。
6. 解析 `&Type` 记录引用。
7. 执行 CFT `check {}` 业务校验。

建议在提交 schema 或数据变更前运行该命令。

## `build`

`build` 运行完整校验，然后导出数据，并按项目配置可选生成代码。

```powershell
coflow build [CONFIG_OR_DIR] [--data-out DIR] [--code-out DIR] [--namespace NAME]
```

示例：

```powershell
coflow build examples/rpg
coflow build examples/rpg --data-out out/data --code-out out/csharp --namespace Game.Config
```

覆盖参数：

| 参数 | 作用 |
| --- | --- |
| `--data-out DIR` | 覆盖 `outputs.data.dir` |
| `--code-out DIR` | 覆盖 `outputs.code.dir` |
| `--namespace NAME` | 覆盖 C# codegen 的命名空间 |

只有项目配置、schema、数据加载、引用解析和 `check` 全部通过时，`build` 才会写产物。若产生诊断，本次运行不会写数据或代码产物。

## `export`

`export` 只导出数据，不生成运行时代码。

### `export json`

```powershell
coflow export json [CONFIG_OR_DIR] [--out DIR]
```

项目配置需要声明：

```yaml
outputs:
  data:
    type: json
    dir: generated/data
```

示例：

```powershell
coflow export json examples/rpg
coflow export json examples/rpg --out generated/data
```

输出文件名为 `<TypeName>.json`。

### `export messagepack`

```powershell
coflow export messagepack [CONFIG_OR_DIR] [--out DIR]
```

项目配置需要声明：

```yaml
outputs:
  data:
    type: messagepack
    dir: generated/data
```

示例：

```powershell
coflow export messagepack examples/rpg
coflow export messagepack examples/rpg --out generated/data
```

输出文件名为 `<TypeName>.msgpack`。

### 输出目录接管

数据导出成功后，`outputs.data.dir` 或 `--out` 指定的目录由 coflow 完整接管。

导出会先写入同级 staging 目录，所有文件写入成功后再替换目标输出目录。目标目录中已有文件和子目录都会被移除。不要把手写文件放进数据输出目录。

## `codegen`

`codegen` 只生成运行时代码，不加载数据源。

### `codegen csharp`

```powershell
coflow codegen csharp [CONFIG_OR_DIR] [--out DIR] [--namespace NAME]
```

项目配置需要声明：

```yaml
outputs:
  data:
    type: json # 或 messagepack
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
```

示例：

```powershell
coflow codegen csharp examples/rpg
coflow codegen csharp examples/rpg --out generated/csharp --namespace Game.Config
```

`outputs.data.type` 会影响生成的 C# loader：

| 数据输出类型 | C# loader |
| --- | --- |
| `json` | Newtonsoft.Json loader |
| `messagepack` | MessagePack-CSharp loader |

C# 输出目录由 coflow 完整接管，目录中已有文件和子目录都会被移除。`coflow.enum.lock.json` 位于 `coflow.yaml` 同级，不属于 C# 输出目录。

对于 `@idAsEnum`，单独运行 `codegen csharp` 会读取已有 `coflow.enum.lock.json`，但不会加载数据源，因此不会新增 data-driven enum variant。需要根据当前数据补全 variant 时，使用 `coflow build`。

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
coflow schema inspect [CONFIG_OR_DIR] [--type TYPE] [--include-derived] [--human]
```

示例：

```powershell
coflow schema inspect examples/rpg
coflow schema inspect examples/rpg --type Item
coflow schema inspect examples/rpg --type Reward --include-derived
coflow schema inspect examples/rpg --human
```

默认输出 JSON，包含：

- `types`：类型、父类型、类型标记、注解和继承后的字段列表。
- `fields`：字段名、结构化类型引用、原始类型文本、默认值、注解和维度信息。
- `enums`：枚举、枚举值和注解。
- `consts`：schema 常量。
- `diagnostics`：schema 编译或项目配置诊断。

`--type TYPE` 只过滤 `types`；`enums` 和 `consts` 仍会完整输出，方便工具解析字段类型、枚举值和常量。`--include-derived` 会同时输出可赋值给该类型的派生类型。

### `schema files`

输出参与本次 schema 编译的 CFT 文件源码。

```powershell
coflow schema files [CONFIG_OR_DIR] [--human]
```

示例：

```powershell
coflow schema files examples/rpg
coflow schema files examples/rpg --human
```

该命令适合读取原始注释、注解、默认值、字段顺序和 `check` 块。

### `schema write-file`

从标准输入写入项目配置中的本地 `.cft` 文件。

```powershell
coflow schema write-file [CONFIG_OR_DIR] --file FILE --stdin [--dry-run] [--check] [--human]
```

示例：

```powershell
Get-Content schema/main.cft | coflow schema write-file examples/rpg --file schema/main.cft --stdin --check
Get-Content schema/main.cft | coflow schema write-file examples/rpg --file schema/main.cft --stdin --dry-run --check
```

限制：

- `--file` 必须是当前项目 `schema` 配置包含的 `.cft` 文件。
- 不能写入数据文件、输出目录或未配置的任意路径。
- `--dry-run` 不落盘，只报告差异和检查结果。
- `--check` 会在写入后编译 schema；dry-run 模式下会用标准输入内容在内存中检查。

非 dry-run 模式下，如果 `--check` 发现诊断，文件已经写入，调用方需要根据报告继续修正。

## `data`

`data` 子命令面向自动化工具、编辑器和 AI agent，用来读取数据索引、读取记录、创建本地数据文件、同步表头或通过 writer 修改数据。

### `data sources`

输出解析后的数据源、provider id、writer 能力和发现的 record 类型。

```powershell
coflow data sources [CONFIG_OR_DIR] [--human]
```

示例：

```powershell
coflow data sources examples/rpg
coflow data sources examples/rpg --human
```

该命令会加载完整项目数据，因此要求数据源文件存在。

### `data list`

输出轻量 record 索引，不包含完整字段值。

```powershell
coflow data list [CONFIG_OR_DIR] [--type TYPE] [--file FILE] [--limit N] [--offset N] [--human]
```

示例：

```powershell
coflow data list examples/rpg --type Item
coflow data list examples/rpg --file data/items.cfd --limit 20
```

每条记录包含 record type、record key、来源文件和 provider。

### `data get`

输出完整 record 数据。

```powershell
coflow data get [CONFIG_OR_DIR] [TYPE.KEY] [--type TYPE] [--file FILE] [--keys a,b] [--limit N] [--offset N] [--all] [--human]
```

示例：

```powershell
coflow data get examples/rpg Item.sword_fire
coflow data get examples/rpg --type Item --keys sword_fire,staff_ice
coflow data get examples/rpg --type Item --limit 50
coflow data get examples/rpg --type Item --all
```

没有指定单条 `TYPE.KEY` 时，如果匹配记录超过默认安全上限，需要显式传入 `--limit` 或 `--all`。

### `data create-file`

创建本地数据文件。

```powershell
coflow data create-file [CONFIG_OR_DIR] --file FILE [--type TYPE] [--provider cfd|csv|excel] [--sheet SHEET] [--human]
```

示例：

```powershell
coflow data create-file examples/rpg --file data/items.csv --type Item --provider csv
coflow data create-file examples/rpg --file data/items.cfd --provider cfd
```

规则：

- `--provider` 省略时按扩展名推断：`.cfd`、`.csv`、`.xlsx`。
- `.csv` 和 `.xlsx` 会按 `--type` 的当前 schema 字段创建表头。
- `.cfd` 创建空文件，不写表头。
- 命令拒绝覆盖已存在文件。

### `data sync-header`

按当前 schema 同步本地数据文件的顶层字段或表头。

```powershell
coflow data sync-header [CONFIG_OR_DIR] --file FILE --type TYPE [--provider cfd|csv|excel] [--sheet SHEET] [--human]
```

示例：

```powershell
coflow data sync-header examples/rpg --file data/items.csv --type Item
coflow data sync-header examples/rpg --file data/items.cfd --type Item
```

行为：

- `.csv` 和 `.xlsx` 会重写表头，保留同名列数据，新增列填空，删除 schema 中不存在的列。
- `.cfd` 会重写匹配 `--type` 的顶层记录字段，保留仍存在字段的源码值，新增字段写入 schema 默认值或类型默认值。
- 当前只操作本地 `.cfd`、`.csv` 和 `.xlsx` 文件。

### `data write-file`

从标准输入重写项目配置中的本地 CFD 文件。

```powershell
coflow data write-file [CONFIG_OR_DIR] --file FILE --stdin [--dry-run] [--check] [--human]
```

示例：

```powershell
Get-Content data/items.cfd | coflow data write-file examples/rpg --file data/items.cfd --stdin --check
Get-Content data/items.cfd | coflow data write-file examples/rpg --file data/items.cfd --stdin --dry-run
```

限制：

- 只接受精确小写 `.cfd` 文件。
- `--file` 必须位于项目配置中的本地 CFD source 覆盖范围内。
- 不能写入表格文件、远端 source、输出目录、显式非 CFD provider source 或未配置路径。
- `--dry-run` 不落盘，只比较目标文件内容并输出报告。
- `--check` 在非 dry-run 写入后运行完整项目校验。

非 dry-run 模式下，如果 `--check` 发现诊断，文件已经写入，调用方需要根据报告继续修正。

### `data patch`

通过 provider writer 应用批量数据补丁。

```powershell
coflow data patch [CONFIG_OR_DIR] --patch PATCH_FILE [--human]
```

示例：

```powershell
coflow data patch examples/rpg --patch patch.json
```

patch 文件示例：

```json
{
  "check_after_write": true,
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
| `rename_record` | 重命名记录 key，并同步引用 |
| `delete_record` | 删除记录，`file` 可作为来源 guard |

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
{ "$ref": "Item.sword_01" }
{ "$ref": { "type": "Item", "key": "sword_01" } }
{ "$type": "ItemReward", "item": { "$ref": "Item.sword_01" }, "count": 1 }
{ "$dict": [{ "key": "Fire", "value": 10 }] }
```

写入会走 provider writer 层，不绕过数据源。批量 patch 按顺序应用；已经成功的操作不会因为后续操作失败自动回滚，调用方需要读取输出中的 `applied` 和 `failed`。

## 命令矩阵

| 命令 | 需要 schema | 需要数据源 | 构建 DataModel | 执行 CFT check | 写入文件 |
| --- | --- | --- | --- | --- | --- |
| `init` | 否 | 否 | 否 | 否 | 创建项目骨架 |
| `cft check` | 是 | 否 | 否 | 否 | 否 |
| `lsp` | 是 | 否 | 否 | 否 | 否 |
| `schema inspect` | 是 | 否 | 否 | 否 | 否 |
| `schema files` | 是 | 否 | 否 | 否 | 否 |
| `schema write-file` | 是 | 否 | 否 | 可选 schema 编译 | 写 `.cft` |
| `data sources` | 是 | 是 | 是 | 否 | 否 |
| `data list` | 是 | 是 | 是 | 否 | 否 |
| `data get` | 是 | 是 | 是 | 否 | 否 |
| `data create-file` | 是 | 否 | 否 | 否 | 创建数据文件 |
| `data sync-header` | 是 | 否 | 否 | 否 | 更新本地数据文件 |
| `data write-file` | 是 | 是 | 可选 | 可选 | 写 `.cfd` |
| `data patch` | 是 | 是 | 是 | 可选 | 通过 writer 修改数据 |
| `check` | 是 | 是 | 是 | 是 | 否 |
| `build` | 是 | 是 | 是 | 是 | 数据和可选代码 |
| `export json` | 是 | 是 | 是 | 是 | JSON 数据 |
| `export messagepack` | 是 | 是 | 是 | 是 | MessagePack 数据 |
| `codegen csharp` | 是 | 否 | 否 | 否 | C# 代码 |

