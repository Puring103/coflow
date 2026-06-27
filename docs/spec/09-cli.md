# CLI 命令规格

本文档描述用户可见的 `coflow` 命令行接口。在仓库内开发时可以使用
`cargo run -- <command>`；安装后可直接把 `cargo run --` 替换为 `coflow`。

---

## 通用项目参数

大多数命令接受可选的 `CONFIG_OR_DIR` 参数：

- 省略时，在当前目录查找 `coflow.yaml` 或 `coflow.yml`。
- 参数是目录时，在该目录内查找 `coflow.yaml` 或 `coflow.yml`。
- 参数是文件时，直接把该文件作为项目配置读取。

`coflow.yaml` 中的项目相对路径都以配置文件所在目录为根解析。

---

## 退出码

- `0`：命令成功完成。
- 非 `0`：命令失败，或产生诊断。

会输出结构化诊断的命令，即使已经尽量收集了多个可恢复错误，只要存在诊断就
返回非 `0`。存在诊断时，Coflow 不写入 build/export/codegen 产物。

---

## 项目配置

最小项目配置示例：

```yaml
schema: schema/

sources:
  - path: data
    sheets:
      - sheet: Item
        columns:
          Item ID: id
          Name: name

outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
```

`schema` 可以是一个精确小写 `.cft` 文件、一个目录，或文件/目录列表。schema
目录会递归查找精确小写 `.cft` 文件；`.CFT` 或混合大小写扩展名不会被当作
schema 文件。

`sources` 配置数据输入。每个 source 必须且只能设置 `path` 或 `url` 之一：

- `path` 可指向单个 `.xlsx` / `.xlsm` / `.xls` / `.cfd` 文件，也可指向目录。
- `url` 指向远端 source，例如 Feishu/Lark 电子表格链接；已知飞书电子表格 token
  可写成 `url: lark:<spreadsheet_token>`。
- `type` 是可选 provider id；省略时由 registry probe 推断。Feishu/Lark 可显式写
  `type: lark-sheet`。
- 除 `type`、`path`、`url` 之外的字段会原样作为 provider options 传入 loader。
- 目录源会由 loader resolve 阶段递归发现支持的 Excel 和 CFD 文件，忽略其他扩展名。

Excel 和飞书电子表格 source 都可以配置 provider option `sheets`。如果省略
`sheets`，默认加载 workbook 或远端电子表格中的所有 sheet，sheet 名作为 CFT
类型名，表头文本作为字段名。目录源可以同时发现 Excel 和 CFD 文件；此时 `sheets`
只传给目录内的 Excel workbook，CFD 文件仍按文本记录类型加载。sheet 可配置：

- `sheet`：Excel worksheet 或飞书 sheet 名称。
- `type`：可选 CFT 类型名；省略时使用 sheet 名称。
- `key`：可选 record key 表头列名；省略时使用 `id`、`Id` 或 `ID`。
- `columns`：可选表头文本到 CFT 字段名的重命名映射。

导入的 sheet 必须有 key 表头列作为 record key，默认列名为 `id`、`Id` 或 `ID`。名为 `#` 的表头是
可选导入控制列：数据行中该列单元格为 `##` 时，整行在读取 `id` 或字段前跳过。

单个 `.cfd` 文件 source 不配置 `sheets`，由文本中的记录类型决定 CFT 类型。
Excel 和 CFD 记录会先合并为同一个 data model，再解析引用和执行 check，因此
二者可以相互引用。

`outputs.data.type` 支持 `json` 或 `messagepack`。

`outputs.code.type` 目前支持 `csharp`。output 除 `type`、`dir` 之外的字段会作为
provider options 传入。`outputs.code.namespace` 供 C# codegen 使用，并可被命令行
参数覆盖。

---

## 命令

### `coflow init [DIR]`

在 `DIR` 创建最小 Coflow 项目；省略 `DIR` 时在当前目录创建。

```powershell
cargo run -- init my-config
```

该命令创建：

- `coflow.yaml`
- `schema/`
- `data/`
- `generated/data/`
- `generated/csharp/`

生成的配置使用 JSON 数据输出、C# 代码输出、命名空间 `Game.Config`，并带有
空的 `sources: []`。运行 `check`、`build` 或 `export` 等数据命令前，需要先
添加 schema 文件和数据 source。

`init` 会先检查目标 `coflow.yaml` 是否存在。若已存在，命令失败，并且不会
创建目录或修改目标项目。

### `coflow cft check [CONFIG_OR_DIR] [--json] [--stdin-path PATH]`

编译配置中的 CFT schema 文件并输出 schema 诊断。

```powershell
cargo run -- cft check examples/rpg
cargo run -- cft check examples/rpg --json
```

该命令不要求数据源文件存在。它校验 schema path、output 配置形状和 source
配置形状，然后编译 schema 文件。

`--stdin-path PATH` 把标准输入当作指定 schema 文件的内容。`PATH` 必须指向
已经属于配置 schema 集合的文件，可以是 `schema/main.cft` 这类项目相对
module path，也可以是从项目根解析出的文件路径。无法匹配已配置 schema 文件时，
命令会在 schema 编译前失败。该参数主要用于编辑器集成。

`--json` 输出：

```json
{"diagnostics":[]}
```

或同形状但带有一个或多个诊断项的对象。

### `coflow lsp [CONFIG_OR_DIR]`

启动 Coflow language server，供编辑器集成使用。该命令是编辑器插件默认入口，
覆盖 CFT 和 CFD 相关语言能力。

```powershell
cargo run -- lsp examples/rpg
```

language server 使用 schema-only 项目加载，不要求数据源文件存在。

支持的编辑器行为：

- 为配置中的项目 schema 文件发布诊断，包含未保存的打开文档内容。
- 对正在编辑的文件，诊断优先使用打开文档 URI。
- 为 CFT 关键字、primitive 和命名类型、enum variant、annotation、默认值、
  字段、check 内建函数和 check 上下文提供补全。
- 为 schema 符号、字段、常量、enum variant 和可解析的跨文件引用提供 hover
  与 go-to-definition。
- 提供 document symbols、document formatting 和 full semantic tokens。
- 遵循标准 JSON-RPC `shutdown` / `exit` 生命周期。

### `coflow schema inspect [CONFIG_OR_DIR] [--type TYPE] [--include-derived] [--human]`

输出编译后的 schema 视图，供自动化工具和 AI agent 读取。该命令默认输出 JSON；
传入 `--human` 时输出面向终端浏览的文本。

JSON 报告包含：

- `types`：类型、父类型、类型标记、注解和继承后的字段列表。
- `fields`：字段名、结构化类型引用、原始类型文本、默认值、注解和维度信息。
- `enums`：枚举、枚举值和注解。
- `consts`：schema 常量。
- `diagnostics`：schema 编译或项目配置诊断。

```powershell
cargo run -- schema inspect examples/rpg
cargo run -- schema inspect examples/rpg --type Item
cargo run -- schema inspect examples/rpg --type Item --include-derived
```

`--type TYPE` 只过滤 `types` 数组。`enums` 和 `consts` 仍完整输出，方便 agent
在读取单个类型时解析字段枚举值和常量。`--include-derived` 会同时输出可赋值给
该类型的派生类型，适合需要了解多态可写值的 agent。注解信息会保留在 JSON 中；
如果需要读取注释、check block 或原始 CFT 文本，使用 `schema files`。

### `coflow schema files [CONFIG_OR_DIR] [--human]`

输出参与本次 schema 编译的 CFT module 源文本。该命令默认输出 JSON，形状为
`{"files":[{"module":"...","source":"..."}],"diagnostics":[]}`；传入 `--human`
时按 module 名和源码文本直接打印。

```powershell
cargo run -- schema files examples/rpg
```

该命令用于让 agent 读取当前 schema 定义、注释、注解、默认值和 check 逻辑。

### `coflow data sources [CONFIG_OR_DIR] [--human]`

输出已解析的数据 source、provider id、writer capabilities，以及每个 source 中
发现的 record 类型。该命令会加载完整项目数据，因此要求数据源文件存在。

```powershell
cargo run -- data sources examples/rpg
```

### `coflow data list [CONFIG_OR_DIR] [--type TYPE] [--file FILE] [--limit N] [--offset N] [--human]`

输出轻量 record 索引。每条记录包含 `record.type`、`record.key`、`file` 和
`provider`，不包含完整字段值。

```powershell
cargo run -- data list examples/rpg --type Item
cargo run -- data list examples/rpg --file data/items.cfd --limit 20
```

### `coflow data get [CONFIG_OR_DIR] [TYPE.KEY] [--type TYPE] [--file FILE] [--keys a,b] [--limit N] [--offset N] [--all] [--human]`

输出完整 record 数据。可以直接读取单条记录，也可以按类型、文件、key 集合批量读取。
字段值使用 Coflow 数据模型的结构化 JSON 表示，整数值为了跨语言精度稳定会序列化为
字符串。

```powershell
cargo run -- data get examples/rpg Item.sword
cargo run -- data get examples/rpg --type Item --keys sword,shield
cargo run -- data get examples/rpg --type Item --limit 50
```

未指定单条 `TYPE.KEY` 时，如果匹配记录超过默认安全上限，需要显式传入 `--limit`
或 `--all`，避免 agent 无意中 dump 整个项目。`CONFIG_OR_DIR` 省略时按通用项目
参数规则从当前目录解析；如果只传一个带点的相对配置文件名，例如 `coflow.yaml`，
它仍按项目配置路径处理。

### `coflow data create-file [CONFIG_OR_DIR] --file FILE [--type TYPE] [--provider cfd|csv|excel] [--sheet SHEET] [--human]`

创建本地数据文件。`--provider` 省略时按扩展名推断：`.cfd`、`.csv`、`.xlsx`。
`.csv` 和 `.xlsx` 会按 `--type` 的最新 schema 字段创建表头；如果项目 source 的
sheet 配置声明了 `key` 或 `columns`，表头会使用这些配置中的列名。`.cfd` 只创建
空文件，不写表头。

```powershell
cargo run -- data create-file examples/rpg --file data/items.csv --type Item --provider csv
cargo run -- data create-file examples/rpg --file data/items.cfd --provider cfd
```

命令拒绝覆盖已存在的文件。该命令只需要 schema，不会加载完整数据源，因此可以用于
先创建缺失的数据文件。

### `coflow data sync-header [CONFIG_OR_DIR] --file FILE --type TYPE [--provider cfd|csv|excel] [--sheet SHEET] [--human]`

按最新 schema 同步本地数据文件的顶层列。`.csv` 和 `.xlsx` 会重写第一行表头，
保留同名列的数据，新增列填空，删除 schema 中不存在的列。`.cfd` 没有表头行；
该命令会重写匹配 `--type` 的记录顶层字段，保留仍存在字段的源码值，删除旧字段，
新增字段写入 schema 默认值或类型默认值（nullable 为 `null`）。

```powershell
cargo run -- data sync-header examples/rpg --file data/items.csv --type Item
cargo run -- data sync-header examples/rpg --file data/items.cfd --type Item
```

当前只支持本地 `.cfd`、`.csv` 和 `.xlsx` 文件，不操作远端飞书表格。

### `coflow data patch [CONFIG_OR_DIR] --patch PATCH_FILE [--human]`

通过 provider writer 应用批量数据补丁。该命令默认读取 JSON patch 文件并输出
JSON 报告；传入 `--human` 时输出终端文本。写入会走和编辑器相同的 writer 层，
不会绕过 provider。

```powershell
cargo run -- data patch examples/rpg --patch patch.json
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
      "path": ["rarity"],
      "value": "Rare"
    }
  ]
}
```

支持的操作：

- `insert_record`：新增顶层记录，必须指定 `file`。
- `set_field`：修改字段路径，`file` 是可选 guard；如果字段来自 spread source，
  guard 按实际写入文件判断。
- `delete_record`：删除记录，`file` 是可选 guard。

patch value 支持普通 JSON 值，也支持特殊对象：

```json
{ "$ref": "Item.sword_01" }
{ "$ref": { "type": "Item", "key": "sword_01" } }
{ "$type": "ItemReward", "item": { "$ref": "Item.sword_01" }, "count": 1 }
{ "$dict": [{ "key": "Fire", "value": 10 }] }
```

`$ref` 写入 record 引用，`$type` 写入多态 inline object，`$dict` 用于 enum/int 等
非字符串 key 的 dict。第一版不支持 dict-key 路径写入，例如直接修改
`["resistances", "Fire"]` 会被拒绝。

写入语义：

- 单个写入或 patch value 校验失败时，当前 op 会进入 `failed`。无失败时
  `failed` 是空数组 `[]`。
- `stop_on_write_error: true` 时，首个写入错误会停止后续 op。
- batch patch 按顺序应用，已经成功的 op 不会因后续 op 失败自动回滚；调用方需要
  通过 `applied` 和 `failed` 判断是否发生了部分落盘。
- CFT `check {}` 不阻拦写入；命令会写完后重建项目并返回诊断，因此可能短暂留下
  有错误的数据。
- CLI 在写入失败或最终诊断中存在 error severity 时返回非 `0`。

### `coflow check [CONFIG_OR_DIR] [--json]`

运行完整校验管线，但不写产物：

1. 项目配置 preflight。
2. schema 发现与编译。
3. loader resolve、preflight 和数据加载。
4. data model 构建。
5. 引用解析。
6. CFT `check {}` 执行。

```powershell
cargo run -- check examples/rpg
cargo run -- check examples/rpg --json
```

建议在提交数据变更前运行。该命令会在不依赖无效中间状态的前提下尽量收集
所有诊断。

### `coflow build [CONFIG_OR_DIR] [--data-out DIR] [--code-out DIR] [--namespace NAME]`

运行校验、导出数据，并按配置可选生成代码。

```powershell
cargo run -- build examples/rpg
cargo run -- build examples/rpg --data-out out/data --code-out out/csharp --namespace Game.Config
```

只有项目、schema、数据加载、data model 和 check 诊断都为空时，`build` 才会写
数据产物。如果省略 `outputs.code`，`build` 是 data-only build，只写配置的
数据输出。如果配置了 `outputs.code`，则还会在 C# codegen preflight 成功后
生成代码。

覆盖项：

- `--data-out DIR`：覆盖 `outputs.data.dir`。
- `--code-out DIR`：覆盖 `outputs.code.dir`。
- `--namespace NAME`：覆盖 `outputs.code` provider option 中的 `namespace`。

只要产生任何诊断，`build` 就返回非 `0`，且本次失败运行不写数据或代码产物。
写入前会 preflight data/code 输出路径；如果输出路径已经存在但不是目录，会
报告 artifact 诊断。

### `coflow export json [CONFIG_OR_DIR] [--out DIR]`

导出 JSON 数据。项目配置必须声明：

```yaml
outputs:
  data:
    type: json
```

```powershell
cargo run -- export json examples/rpg
cargo run -- export json examples/rpg --out generated/data
```

输出文件命名为 `<TypeName>.json`。

导出成功后，输出目录由 Coflow 完整接管。本轮数据会先写入同级 staging 目录，
所有文件写入成功后再替换目标输出目录；目标目录中已有的文件和子目录都会被移除。
导出不写 manifest，也不会尝试保留输出目录中的人工文件。若不希望文件被删除，
请把人工文件放到输出目录之外。

### `coflow export messagepack [CONFIG_OR_DIR] [--out DIR]`

导出 MessagePack 数据。项目配置必须声明：

```yaml
outputs:
  data:
    type: messagepack
```

```powershell
cargo run -- export messagepack examples/rpg
cargo run -- export messagepack examples/rpg --out generated/data
```

输出文件命名为 `<TypeName>.msgpack`，内容是裸 MessagePack array，schema
形状与 JSON 导出一致。

MessagePack 导出同样完整替换数据输出目录。JSON 和 MessagePack 格式切换时，
上一轮旧格式文件会随目录替换自然消失。

### `coflow codegen csharp [CONFIG_OR_DIR] [--out DIR] [--namespace NAME]`

生成 C# 运行时加载代码，但不加载数据源。

```powershell
cargo run -- codegen csharp examples/rpg
cargo run -- codegen csharp examples/rpg --out generated/csharp --namespace Game.Config
```

项目配置必须声明：

```yaml
outputs:
  data:
    type: json # 或 messagepack
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
```

`outputs.data.type` 决定生成哪种 loader：

| 数据输出类型 | 生成的 loader |
| --- | --- |
| `json` | Newtonsoft.Json loader |
| `messagepack` | MessagePack-CSharp loader |

`codegen csharp` 不要求数据 source 存在。对于 `@idAsEnum`，它会读取
`coflow.yaml` 同级的 `coflow.enum.lock.json` 并保留已有 variant，但无法新增
data-driven variant，因为它不加载数据。`coflow build` 已经加载 data model，
因此可以新增这些 variant。新 variant 追加到 lockfile，已有 variant 的整数值
保持不变。

codegen 会在读取/写入 lockfile、替换输出目录或写入新文件前执行 preflight。
命名错误会产生诊断，并且不会修改既有生成输出。preflight 通过后，如果
`coflow.enum.lock.json` 格式损坏，命令会返回 artifact diagnostic。

成功写入时，codegen 会先把 C# 文件写入同级 staging 目录，并把更新后的
`coflow.enum.lock.json` 写入 staging 文件；随后提交 lockfile 与输出目录。
C# 输出目录由 Coflow 完整接管，目标目录中已有的 `.cs`、非 `.cs` 文件和子目录
都会被移除。`coflow.enum.lock.json` 位于 `coflow.yaml` 同级，不属于 C# 输出目录。
写入或 staging 失败时，既有输出目录和 lockfile 会保持不变；如果提交阶段中目录
替换失败，Coflow 会尽力回滚已经替换的 lockfile。

---

## 命令矩阵

| 命令 | 需要 schema | 需要数据源文件 | 构建 data model | 执行 check | 写产物 |
| --- | --- | --- | --- | --- | --- |
| `init` | 否 | 否 | 否 | 否 | 创建项目文件 |
| `cft check` | 是 | 否 | 否 | 否 | 否 |
| `lsp` | 是 | 否 | 否 | 否 | 否 |
| `check` | 是 | 是 | 是 | 是 | 否 |
| `build` | 是 | 是 | 是 | 是 | 数据和可选代码 |
| `export json` | 是 | 是 | 是 | 是 | JSON 数据 |
| `export messagepack` | 是 | 是 | 是 | 是 | MessagePack 数据 |
| `codegen csharp` | 是 | 否 | 否 | 否 | C# 代码 |

输出格式、错误码和非阻塞诊断收集规则见 [10-diagnostics.md](10-diagnostics.md)。
