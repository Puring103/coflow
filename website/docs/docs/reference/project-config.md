# coflow.yaml 参考

`coflow.yaml` 是 Coflow 项目的入口配置文件。它描述 schema 在哪里、数据从哪里读取，以及构建成功后产物写到哪里。

大多数命令都接受可选的 `CONFIG_OR_DIR` 参数：

- 省略时，在当前目录查找 `coflow.yaml`，然后查找 `coflow.yml`。
- 参数是目录时，在该目录下查找 `coflow.yaml`，然后查找 `coflow.yml`。
- 参数是文件时，直接把该文件作为项目配置读取。

所有项目相对路径都以配置文件所在目录为根解析。

## 最小示例

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

## 顶层字段

| 字段 | 类型 | 必填 | 说明 |
| --- | --- | --- | --- |
| `schema` | path 或 path list | 是 | CFT schema 文件或目录。 |
| `sources` | source list | 否 | 数据输入源。运行 `check`、`build`、`export` 时通常需要。 |
| `outputs` | object | 否 | 构建、导出和代码生成输出配置。 |
| `dimensions` | object | 否 | 维度/变体配置，目前内建使用 `dimensions.language` 支持本地化。 |

`coflow.yaml` 使用严格字段集。未知的顶层字段会被诊断为配置错误。YAML 映射中不允许重复 key，避免后写字段静默覆盖前面的配置。

## `schema`

`schema` 指向 CFT schema：

```yaml
schema: schema/
```

也可以指向单个文件：

```yaml
schema: schema/main.cft
```

或多个文件/目录：

```yaml
schema:
  - schema/common/
  - schema/gameplay/main.cft
```

规则：

- 文件必须使用精确小写 `.cft` 扩展名。
- 目录会递归发现精确小写 `.cft` 文件。
- `.CFT` 或混合大小写扩展名不会被当作 schema 文件。

## `sources`

`sources` 配置数据输入。每个 source 必须且只能设置 `path` 或 `url` 之一。

```yaml
sources:
  - path: data
```

### 本地 source

`path` 可以指向单个文件：

```yaml
sources:
  - path: data/items.xlsx
  - path: data/monsters.cfd
```

也可以指向目录：

```yaml
sources:
  - path: data
```

目录 source 会由 loader resolve 阶段递归发现支持的文件。目前常见输入包括：

- `.xlsx`
- `.xlsm`
- `.xls`
- `.csv`
- `.cfd`

目录里不支持的扩展名会被忽略。

### 远端 source

`url` 用于远端数据源，例如飞书/Lark 电子表格：

```yaml
sources:
  - type: lark-sheet
    url: https://example.feishu.cn/wiki/xxxxx
    app_id: cli_xxx
    app_secret: xxx
```

已知电子表格 token 时，也可以写成：

```yaml
sources:
  - type: lark-sheet
    url: lark:<spreadsheet_token>
    app_id: cli_xxx
    app_secret: xxx
```

暂不支持飞书多维表格/Base。

### `type`

`type` 是可选 provider id：

```yaml
sources:
  - type: cfd
    path: data/story.cfd
```

省略时，Coflow 会通过 provider registry probe 推断 provider。远端飞书 source 通常显式写：

```yaml
type: lark-sheet
```

如果多个 provider 都能处理同一个 source，应该显式指定 `type`。

### provider options

除 `type`、`path`、`url` 之外的字段会原样传给 provider 作为 options。

例如 Excel 和飞书电子表格都支持 `sheets`：

```yaml
sources:
  - path: data/items.xlsx
    sheets:
      - sheet: Item
        type: Item
        key: id
        columns:
          Item ID: id
          Name: name
```

provider options 可以直接写普通字符串：

```yaml
sources:
  - type: lark-sheet
    url: lark:<spreadsheet_token>
    app_id: cli_xxx
    app_secret: xxx
```

## `sheets`

`sheets` 用于配置 Excel workbook 或飞书电子表格中的 sheet 映射。

省略 `sheets` 时：

- 默认加载所有 sheet。
- sheet 名作为 CFT 类型名。
- 表头文本作为字段名。
- record key 列默认使用 `id`、`Id` 或 `ID`。

显式配置时：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `sheet` | 是 | Excel worksheet 或飞书 sheet 名称。 |
| `type` | 否 | 目标 CFT 类型名。省略时使用 `sheet`。 |
| `key` | 否 | record key 表头列名。省略时使用 `id`、`Id` 或 `ID`。 |
| `columns` | 否 | 表头文本到 CFT 字段名的重命名映射。 |

`columns` 不是白名单。未列出的表头仍会按原表头文本尝试映射到 CFT 字段。

导入的 sheet 必须有 key 表头列作为 record key。名为 `#` 的表头是可选导入控制列；数据行中该列单元格为 `##` 时，整行会在读取 key 或字段前跳过。

目录 source 可以同时发现 Excel、CSV 和 CFD 文件。此时 `sheets` 只作用于表格类 source；CFD 文件仍由文本中的记录类型决定 CFT 类型。

### 表格配置示例

如果 Excel sheet 名、表头和 CFT 类型/字段完全一致，可以只写文件路径：

```yaml
schema: schema/

sources:
  - path: data/items.xlsx

outputs:
  data:
    type: json
    dir: generated/data
```

例如 `items.xlsx` 中有一个名为 `Item` 的 sheet：

| id | name | rarity | price |
| --- | --- | --- | --- |
| potion | Potion | Common | 50 |
| sword | Iron Sword | Rare | 120 |

它会按以下规则读取：

- `Item` sheet 对应 CFT 类型 `Item`。
- `id` 列作为 record key。
- `name`、`rarity`、`price` 表头分别映射到同名 CFT 字段。

如果策划表使用展示名表头，可以用 `columns` 映射：

```yaml
sources:
  - path: data/items.xlsx
    sheets:
      - sheet: 物品表
        type: Item
        key: 物品ID
        columns:
          名称: name
          稀有度: rarity
          价格: price
```

对应表格：

| 物品ID | 名称 | 稀有度 | 价格 |
| --- | --- | --- | --- |
| potion | Potion | Common | 50 |
| sword | Iron Sword | Rare | 120 |

这里：

- `sheet: 物品表` 表示读取 Excel 中的 `物品表`。
- `type: Item` 表示这些行加载为 CFT 类型 `Item`。
- `key: 物品ID` 表示 `物品ID` 列是 record key。
- `columns` 把展示名表头映射到 CFT 字段名。

同一个 workbook 可以配置多个 sheet：

```yaml
sources:
  - path: data/gameplay.xlsx
    sheets:
      - sheet: 物品表
        type: Item
        key: 物品ID
        columns:
          名称: name
          稀有度: rarity
      - sheet: 怪物表
        type: Monster
        key: 怪物ID
        columns:
          等级: level
          掉落: drop
```

目录 source 适合把多个 Excel/CSV/CFD 文件放在同一个数据目录：

```yaml
sources:
  - path: data
    sheets:
      - sheet: 物品表
        type: Item
        key: 物品ID
        columns:
          名称: name
```

如果 `data/` 中同时存在 `items.xlsx`、`monsters.csv` 和 `story.cfd`，Coflow 会递归发现支持的文件。上面的 `sheets` 配置只影响表格类 source；`story.cfd` 仍按照 CFD 文本中的记录类型加载。

飞书/Lark 表格使用同样的 `sheets` 结构：

```yaml
sources:
  - type: lark-sheet
    url: https://example.feishu.cn/wiki/xxxxx
    app_id: cli_xxx
    app_secret: xxx
    sheets:
      - sheet: 物品表
        type: Item
        key: 物品ID
        columns:
          名称: name
          稀有度: rarity
          价格: price
```

## `outputs`

`outputs` 描述构建产物。

```yaml
outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
```

### `outputs.data`

数据导出配置：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `type` | 是 | 数据导出 provider，目前支持 `json` 和 `messagepack`。 |
| `dir` | 是 | 数据输出目录。 |

JSON 示例：

```yaml
outputs:
  data:
    type: json
    dir: generated/data
```

MessagePack 示例：

```yaml
outputs:
  data:
    type: messagepack
    dir: generated/data
```

### `outputs.code`

代码生成配置：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `type` | 是 | 代码生成 provider，目前支持 `csharp`。 |
| `dir` | 是 | 代码输出目录。 |
| `namespace` | 否 | C# 代码命名空间。 |

示例：

```yaml
outputs:
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
```

`outputs.*` 除 `type`、`dir` 之外的字段会作为 provider options 传入。例如 `namespace` 是 C# codegen 的 provider option，也可以被 `coflow codegen csharp --namespace` 覆盖。

## `dimensions`

`dimensions` 用于配置维度/变体数据。目前内建使用 `language` 维度支持 `@localized` 字段。

```yaml
dimensions:
  language:
    variants: [zh, en]
    out_dir: data/dimensions/language
    display_name: 本地化
```

字段：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `dimensions.language.variants` | 是 | 语言变体列表。每个值必须是合法 CFT 标识符，不能重复，不能是保留变体 `default`。 |
| `dimensions.language.out_dir` | 是 | 维度合成文件输出目录。可为绝对路径或项目相对路径。 |
| `dimensions.language.display_name` | 否 | 编辑器中展示这一组维度文件的人类可读名称。省略时 `language` 默认显示为「本地化」。 |

如果不配置 `dimensions`，Coflow 不会构造任何维度数据。schema 中存在 `@localized` 字段但缺少 `dimensions.language` 时，会报告维度配置诊断。

`out_dir` 下的文件由 Coflow 按维度规则生成和维护。语言维度会把源数据中的默认值写入 `default`，再按 `variants` 生成对应变体列或记录，让后续 check 可以在每个语言变体下执行。

配置本地化时使用 `dimensions.language`：

```yaml
dimensions:
  language:
    variants: [zh, en]
    out_dir: data/dimensions/language
```

## 输出目录接管

数据导出和 C# codegen 的输出目录由 Coflow 完全接管。

写入时，Coflow 会先生成完整 staging 目录；所有产物成功写入后，再替换目标输出目录。如果构建过程中出现诊断，Coflow 不会写入 build、export 或 codegen 产物。

**不要把手写文件放进 `outputs.*.dir`。目标目录内已有文件、人工文件和其他工具产物不会被保留**。

## 常见错误

### source 同时设置 `path` 和 `url`

```yaml
sources:
  - path: data/items.xlsx
    url: lark:xxxx
```

每个 source 必须且只能设置 `path` 或 `url` 之一。

### source 使用未知字段

```yaml
sources:
  - file: data/items.xlsx
```

source 只使用通用字段 `type`、`path`、`url` 加 provider options。表达本地文件或目录时使用 `path`。

### schema 扩展名大小写不正确

```yaml
schema: schema/Main.CFT
```

schema 文件必须使用精确小写 `.cft`。

### 输出目录指向手写文件目录

```yaml
outputs:
  data:
    type: json
    dir: data
```

输出目录会被 Coflow 接管，不应指向 schema、source 或项目根目录。
