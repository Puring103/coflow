# coflow.yaml 参考

`coflow.yaml` 是 Coflow 项目的入口配置文件。它描述 schema 在哪里、数据从哪里读取，以及构建成功后产物写到哪里。

所有项目相对路径都以配置文件所在目录为根解析。

## 示例

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
| `outputs` | object 或 target list | 否 | 构建、导出和代码生成输出配置。 |
| `dimensions` | object | 否 | 维度/变体配置，目前内建使用 `dimensions.language` 支持本地化。 |

`coflow.yaml` 使用严格字段集。未知的顶层字段会被诊断为配置错误。YAML 映射中不允许重复 key，避免后写字段静默覆盖前面的配置。

严格字段也适用于每个 output target（只允许 `data`、`code`、`loader`）和 dimension 配置
（只允许 `variants`、`out_dir`、`display_name`）。source 以及 data/code/loader 组件可以包含
对应 provider 支持的额外选项；未知选项或类型错误会被诊断为配置错误。

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

- `schema` 不能为空路径，列表形式也不能为空列表；配置路径必须存在。
- 文件必须使用精确小写 `.cft` 扩展名。
- 目录会递归发现精确小写 `.cft` 文件。
- `.CFT` 或混合大小写扩展名不会被当作 schema 文件。
- 发现结果顺序稳定；指向同一实际文件的重复路径只编译一次。
- 目录发现不会跟随符号链接、junction 等路径别名逃出声明的 schema 根目录。

## `sources`

`sources` 配置数据输入。每个 source 必须设置 `path`。

```yaml
sources:
  - path: data
```

### `path`

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

目录 source 会递归发现支持的文件。默认 Coflow CLI 和编辑器提供以下输入类型：

- `.xlsx`
- `.xlsm`
- `.xls`
- `.csv`
- `.cfd`

支持的文件类型不是 `coflow.yaml` 中的固定白名单，而是由当前应用提供的 source provider
声明和识别。使用 Coflow 库的应用可以注册自定义 source provider，增加新的扩展名或识别
规则；注册后，目录发现和未显式设置 `type` 的文件都会使用这些自定义能力。配置文件只能
选择当前应用已经提供的 provider，不能直接定义或安装 provider。

目录里没有 provider 支持的文件会被忽略。

schema-only 命令只校验 source 的配置形状，不要求 `path` 当前存在。需要数据的命令会要求
每个 source path 是已存在的文件或目录。`path` 和显式 `type` 都不能是空字符串。

目录遍历会去除指向同一实际文件的重复项并保持稳定顺序，不会通过符号链接、junction 等路径别名逃出
声明的目录根。配置了 `dimensions` 时，嵌套的维度托管目录也会从普通目录 source 的
发现结果中排除。

### `type`

`type` 是可选 provider id：

```yaml
sources:
  - type: cfd
    path: data/story.cfd
```

如果多个 provider 都能处理同一个 source，应该显式指定 `type`。

对目录 source 显式指定 `type` 时，只发现该 provider 支持的文件；即使目录为空，provider
选项仍会被校验。未指定 `type` 时，每个文件按其格式选择 provider。目录上的额外选项必须
至少适用于一个实际发现的 provider，否则会报告配置错误。

### provider options

除 `type`、`path` 之外的字段会原样传给 provider 作为 options。

例如 Excel 支持 `sheets`：

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

## `sheets`

`sheets` 用于配置表格 provider 的 sheet 映射。Excel 中 `sheet` 是真实 worksheet 名；CSV
没有物理 sheet，省略配置时以去掉扩展名后的文件名作为唯一逻辑 sheet 名，例如
`items.csv` 使用 `items`；显式配置则用 `sheet` 作为类型和表头的映射标签。

省略 `sheets` 时：

- 默认加载所有 sheet。
- sheet 名作为 CFT 类型名。
- 表头文本作为字段名。
- record key 列默认使用 `id`、`Id` 或 `ID`。

显式配置时：

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `sheet` | 是 | Excel worksheet 名称。 |
| `type` | 否 | 目标 CFT 类型名。省略时使用 `sheet`。 |
| `key` | 否 | record key 表头列名。省略时使用 `id`、`Id` 或 `ID`。 |
| `columns` | 否 | 表头文本到 CFT 字段名的重命名映射。 |

`columns` 不是白名单。未列出的表头仍会按原表头文本尝试映射到 CFT 字段。

同一 source 的 `sheet` 名不能重复。`sheet` 必须是非空字符串；可选 `type`、`key` 也必须是
非空字符串。`columns` 的 source header 和目标字段名都不能为空，一个 sheet 内不能把多个
header 映射到同一目标字段。sheet object 当前只读取 `sheet`、`type`、`key`、`columns`；
其他嵌套 key 不参与映射行为。

导入的 sheet 必须有 key 表头列作为 record key。

目录 source 可以同时发现 Excel、CSV 和 CFD 文件。此时 `sheets` 只作用于表格类 source；
CFD 文件仍由文本中的记录类型决定 CFT 类型。

未指定目录 `type` 时，`sheets` 只应用于其中的表格文件，不影响 CFD 文件。如果显式写
`type: cfd`，同一 source 上的 `sheets` 会被诊断为无效选项。

### 跳过数据行

Excel 和 CSV 中 record key 与所有已映射字段都为空的数据行会被跳过。

表格还可以包含名为 `#` 的控制列。数据行中该列的内容去掉首尾空白后等于 `##` 时，整行会
在读取 record key 和字段之前跳过。`#` 控制列不映射到 CFT 字段，也不参与未知字段检查。

### 表格配置示例

如果 sheet 名、表头和 CFT 类型/字段完全一致，只需配置文件路径：

```yaml
schema: schema/

sources:
  - path: data/items.xlsx

outputs:
  data:
    type: json
    dir: generated/data
```

表头与字段名不同时，使用 `columns` 映射：

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

完整的 sheet 映射、表头、`@expand` 和多 sheet 示例见[表格 Source](https://puring103.github.io/coflow/docs/reference/04-sources/02-table)。

## `outputs`

`outputs` 描述一个或多个构建产物 target。每个 target 必须配置 `data`，可以同时配置 `code` 和 `loader`。

```yaml
outputs:
  - data:
      type: json
      dir: generated/json
    code:
      type: csharp
      dir: generated/csharp-json
      namespace: Game.Config
    loader:
      type: csharp-json
  - data:
      type: messagepack
      dir: generated/messagepack
```

单目标可以使用简写对象语法：

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

该写法等价于只配置一个 target。

可以省略整个 `outputs`，也可以配置空列表；schema 检查和数据查询仍可运行，但 `build`、
`export` 或 `codegen` 会按各自需要报告缺少匹配 target。`type` 和 `dir` 都不能是空字符串。

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
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
```

`outputs.*` 除 `type`、`dir` 之外的字段会作为 provider options 传入。例如 `namespace` 是
C# codegen 的 provider option。构建命令直接使用这里配置的输出目录和 provider options。

### `outputs.loader`

loader 负责为一个 target 的 code/data 组合生成加载代码。当前内置 loader 为：

| `type` | code | data |
| --- | --- | --- |
| `csharp-json` | `csharp` | `json` |
| `csharp-messagepack` | `csharp` | `messagepack` |

`loader` 可以省略；Coflow 会按注册顺序选择与 target 的 `code.type` 和 `data.type` 精确匹配的 loader。显式配置时，loader 必须与同一 target 的 code/data 组合兼容。没有 `code` 的 data-only target 不能配置 `loader`。

内置组合之外的 exporter、codegen 和 loader id 只有在当前应用提供对应 provider 时才有效。
`coflow export` 处理全部 data target，`coflow codegen` 处理全部配置了 code 的 target，
`coflow build` 处理全部 data 和 code target。

## `dimensions`

`dimensions` 用于配置维度/变体数据。每个维度使用相同的配置和校验规则；`language` 是 `@localized` 使用的内建维度名。

维度文件的目录结构、内容和维护规则见[维度文件](https://puring103.github.io/coflow/docs/reference/10-localization#维度文件)。

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
| `dimensions.<name>.variants` | 是 | 维度变体列表。每个值必须是合法 CFT 标识符，不能重复，不能是保留变体 `default`。 |
| `dimensions.<name>.out_dir` | 是 | 维度文件的独占托管目录。可为绝对路径或项目相对路径，不能与其他维度目录重叠。 |
| `dimensions.<name>.display_name` | 否 | 编辑器中展示这一组维度文件的人类可读名称。省略时使用维度名；`language` 默认显示为「本地化」。 |

维度名本身也必须是合法 CFT 标识符。`variants` 不能为空列表。

如果不配置 `dimensions`，Coflow 不会构造任何维度数据。schema 字段引用未配置的维度时，会报告 CFT schema 诊断。

`out_dir` 下的文件由 Coflow 按维度规则生成和维护。普通目录 source 递归发现文件时会自动跳过嵌套的维度托管目录；不要把托管目录或其中的文件显式加入 `sources`。维度文件会把源数据中的默认值写入 `default`，再按 `variants` 生成对应变体列或记录。

配置本地化时使用 `dimensions.language`：

```yaml
dimensions:
  language:
    variants: [zh, en]
    out_dir: data/dimensions/language
```

## 输出目录

每个 target 的 `data.dir` 和 `code.dir` 是构建成功后消费者直接使用的输出目录。目录内容由 Coflow 完整接管，不要在其中放置手写文件。

Coflow 会按解析后的真实路径检查输出目录，以下配置会在生成前被拒绝：

- 输出路径已存在但不是目录。
- 输出目录等于项目根，或与 `coflow.yaml`、任一 schema 路径、任一 source 路径重叠。
- 任意两个本次发布的 data/code 输出目录彼此相同、互为父子目录，或通过路径别名实际重叠。

对单文件 source，其父目录也受保护，因此不能把输出目录设为该 source 的同级目录；这是为
防止 Coflow 接管整个输出目录时删除旁边的输入文件。检查会解析已存在的祖先，所以符号链接、
junction、大小写差异和 Windows 尾随空格/点不能绕过这些规则。

## 常见错误

### source 缺少 `path`

每个 source 必须设置文件或目录的 `path`。

### source 使用未知字段

```yaml
sources:
  - file: data/items.xlsx
```

source 只使用通用字段 `type`、`path` 加 provider options。表达文件或目录时使用 `path`。

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
