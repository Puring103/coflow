# Coflow

Coflow 面向游戏行业配表工具割裂、各项目重复自研、AI 难以深入策划工作流的
现状，提供一套强类型、强校验、可编辑、可渐进本地化、AI 友好的现代化配置工作流。

## 核心亮点

- 统一配置工程链路：覆盖从配置建模到运行时交付的完整流程，减少每个项目重复维护导表脚本和配套工具。
- 强类型建模：用明确的数据结构约束配置内容，减少隐式约定、字段漂移和跨表理解成本。
- 强业务校验：将配置规则前置到构建流程中，提前发现错误，避免问题进入运行时或上线后暴露。
- 多种数据源读写：覆盖本地表格、文本配置和在线协作表格，将不同来源的数据汇总到同一套编辑流程中。
- 专用配置表达：表格适合批量数据，文本配置适合复杂结构，让不同类型的数据用最合适的方式维护。
- AI 友好维护：提供结构化 CLI 命令，让 AI 能理解配置结构、定位数据、修改内容并根据校验结果持续修复，推动 AI 从辅助写文案进入真实配置生产流程。
- 可视化编辑器：提供文件/数据源视图、表格视图、记录详情视图、关系图视图和诊断面板，方便策划和程序从不同角度查看、编辑和排查配置。
- 精准诊断定位：配置错误能清楚定位到具体来源，方便编辑器展示、持续集成拦截和自动化修复。
- 高效本地化：可以按字段逐步开启本地化，不必一次性改造整套配置；且不局限于文本，任意类型的配置值都可以纳入本地化流程。

## AI Agent Skills 安装

如果需要让 AI agent 维护 Coflow 项目，安装仓库内置 skills：

```powershell
npx skills add Puring103/coflow -g --skill "*" --copy -y
```

也可以只安装某一个 skill：

```powershell
npx skills add Puring103/coflow -g --skill coflow-schema-data --copy -y
```

也可以把下面这段话直接复制给 agent，让 agent 在本机完成 skills 安装：

```text
请运行 `npx skills add Puring103/coflow -g --skill "*" --copy -y`
安装 Coflow 内置 skills。
```

---

## 能力概览

- 编译 CFT schema 文件并执行 `check {}` 规则。
- 加载 Excel、CSV、CFD 文本数据和飞书/Lark 表格，构建类型化 data model。
- 通过统一 writer/patch 机制写回支持编辑的数据源。
- 导出 JSON 或 MessagePack 数据文件。
- 生成适用于 .NET 和 Unity 风格项目的 C# 运行时加载代码。
- 提供面向 AI agent 的 schema/data 读取、定位和修改命令。
- 提供可视化编辑器和 LSP/VS Code 集成。

---

## 详细功能介绍

### Schema 与校验

Coflow 使用 CFT 描述配置结构和规则。CFT 支持类型、字段、枚举、默认值、继承、
多态、nullable、数组、字典、记录引用和字段注解。业务规则可以直接写在 schema
中，构建时统一执行，避免配置问题只在游戏运行期暴露。

### 多数据源读取与编辑

Coflow 可以从 Excel、CSV、CFD 文本配置和飞书/Lark 表格读取数据，并汇总成统一
data model。表格适合批量配置，CFD 适合复杂嵌套结构、数组、字典、多态对象和
覆盖模板。支持写回的数据源会通过统一 writer/patch 机制编辑，避免绕过工具手动
改文件导致结构漂移。

### AI 友好工作流

Coflow 提供结构化 schema/data 命令，让 AI agent 可以读取 schema、查看数据源、
列出记录、获取记录详情、创建文件、同步表头、写入 CFD 文件和批量 patch 数据。
写入后返回结构化诊断，agent 可以根据错误位置继续修正，形成可验证的自动维护闭环。

### 可视化编辑器与语言服务

Coflow 编辑器围绕真实项目数据工作，提供文件/数据源视图、表格视图、记录详情视图、
关系图视图和诊断面板。策划可以从表格或记录角度编辑，程序可以从引用关系和诊断
角度排查问题。VS Code/LSP 集成提供 CFT/CFD 的诊断、补全、hover、跳转、符号和
语义高亮能力。

### 渐进式本地化

Coflow 支持按字段逐步开启本地化，不要求一次性改造整套配置。被标记的字段会进入
本地化流程，且不局限于字符串，任意类型的配置值都可以参与本地化。工具链可以生成
和维护翻译表，并在多语言维度下继续执行配置校验。

### 运行时产物

Coflow 可以导出 JSON 或 MessagePack 数据，并生成适用于 .NET 与 Unity 风格项目的
C# 加载代码。构建会先检查项目、数据和输出目录，成功后再写入产物，避免失败构建
污染输出目录。

---

## 快速开始

运行 RPG 示例：

```powershell
coflow check examples/rpg
coflow build examples/rpg
```

生成文件写入 `examples/rpg/coflow.yaml` 声明的路径：

```text
examples/rpg/generated/data
examples/rpg/generated/csharp
```

导出和 codegen 会接管对应输出目录。每次写入都会先在临时 staging 目录生成
完整产物，成功后替换整个输出目录；目录内的旧文件、人工文件和其他工具产物
不会被保留。不要把手写文件放进 `outputs.*.dir`。

单独运行各阶段：

```powershell
coflow cft check examples/rpg
coflow export json examples/rpg
coflow codegen csharp examples/rpg
```

使用 MessagePack 时，把 `coflow.yaml` 中的 `outputs.data.type` 改为
`messagepack`，再运行：

```powershell
coflow build examples/rpg
```

---

## 项目配置

Coflow 项目由 `coflow.yaml` 配置：

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
    namespace: Example.Rpg.Config
```

`schema` 指向一个精确小写 `.cft` 文件、schema 目录，或文件/目录列表。`sources`
使用 `path` 表示本地文件或目录，使用 `url` 表示远端 source；`type` 是可选的
provider id，省略时由 registry probe 推断。除 `type`、`path`、`url` 之外的
source 字段会原样传给 provider 作为 options。`path` 可以指向 `.xlsx` / `.xlsm` /
`.xls` / `.cfd` 文件，也可以指向目录；目录源会由 loader resolve 阶段递归发现
支持的 Excel 和 CFD 文件。

Excel 和飞书电子表格共享同一套 sheet 配置习惯：省略 `sheets` 时默认加载所有
sheet，sheet 名作为 CFT 类型名，表头文本作为字段名；配置 `sheets` 时可显式映射
sheet、类型、record key 列和列头。`key` 可省略，默认使用 `id`、`Id` 或 `ID` 表头列；`columns`
是可选的列头重命名映射，不是白名单。

飞书 source 示例：

```yaml
sources:
  - type: lark-sheet
    url: https://example.feishu.cn/wiki/xxxxx
    app_id: cli_xxx
    app_secret: xxx
    sheets:
      - sheet: Item
        key: id
        columns:
          Name: name
```

`lark-sheet` source 使用顶层 `url` 定位远端表格；`https://.../wiki/...` 会先解析到
真实电子表格 token，已知 token 可写成 `url: lark:<spreadsheet_token>`。暂不支持飞书多维表格/Base。
目录源可同时包含 Excel 和 CFD 文件，此时 `sheets` 只作用于 Excel，CFD 文件仍由
文本中的记录类型决定 CFT 类型。
`outputs.data.type` 支持 `json` 或 `messagepack`；`outputs.code.type` 目前支持
`csharp`。`outputs.*` 除 `type`、`dir` 之外的字段也会作为 provider options
传入，例如 C# codegen 的 `namespace`。

---

## 常用命令

```powershell
coflow init my-config
coflow check examples/rpg
coflow build examples/rpg
coflow export json examples/rpg --out generated/data
coflow export messagepack examples/rpg --out generated/data
coflow codegen csharp examples/rpg --out generated/csharp --namespace Game.Config
coflow lsp examples/rpg
```

AI/data automation 命令默认输出 JSON：

```powershell
coflow schema inspect examples/rpg
coflow schema files examples/rpg
coflow schema write-file examples/rpg --file schema/main.cft --stdin --check
coflow data sources examples/rpg
coflow data list examples/rpg --type Item
coflow data get examples/rpg Item.sword
coflow data create-file examples/rpg --file data/items.csv --type Item --provider csv
coflow data sync-header examples/rpg --file data/items.csv --type Item
coflow data write-file examples/rpg --file data/items.cfd --stdin --check
coflow data patch examples/rpg --patch patch.json
```

`schema write-file` 只允许写入项目配置已包含的精确小写 `.cft` schema 文件；
`--dry-run` 可预览，`--check` 会在写入后或 dry-run 内存内容上编译 schema 并返回诊断。

`data patch` 通过和编辑器相同的 provider writer 层写入数据。它不会用 CFT
`check {}` 阻拦写入；写完后会重建项目并返回诊断，供 agent 继续修正。
`data write-file` 只允许重写配置内本地 CFD source 覆盖的精确小写 `.cfd` 文件
（未指定 `type` 的目录/`.cfd`，或显式 `type: cfd`），适合复杂 CFD 整文件修改；
表格文件仍应使用 `data patch`、`data create-file` 和 `data sync-header`。
`data create-file` / `data sync-header` 是本地文件级命令，支持 `.cfd`、`.csv`
和 `.xlsx`；表格文件同步表头，CFD 文件同步记录顶层字段而不写表头。

完整命令行为见 [CLI 命令参考](website/docs/docs/reference/cli.md)。

---

## AI Agent Skills

仓库内提供面向 AI agent 的 Coflow skills：

- `coflow-schema-data`：维护 Coflow schema、数据文件和记录写入。
- `coflow-cli-development`：开发 Coflow CLI / engine 功能。
- `coflow-cft-cfd-authoring`：编写 CFT schema 和 CFD 文本配置。

从本仓库安装指定 skill：

```powershell
npx skills add Puring103/coflow -g --skill coflow-schema-data --copy -y
```

安装全部 skill：

```powershell
npx skills add Puring103/coflow -g --skill "*" --copy -y
```

本地开发仓库时可先进入仓库目录并预览可安装的 skill：

```powershell
npx skills add . -l
```

---

## CFT 简览

CFT 描述配置数据形状、默认值、record-key 引用和校验规则。

常量：

```cft
const MAX_LEVEL: int = 100;
```

枚举：

```cft
enum Rarity {
  Common = 0,
  Rare = 10,
  Epic = 20,
}
```

类型和字段：

```cft
@idAsEnum(ItemId)
type Item {
  name: string;
  rarity: Rarity = Rarity.Common;
  tags: [string] = [];
  attributes: {string: int} = {};
}

enum ItemId {}
```

继承和多态值：

```cft
abstract type Reward {
  source: string = "drop";

  check { source != ""; }
}

sealed type ItemReward : Reward {
  item: Item;
  count: int = 1;
}
```

校验：

```cft
type Monster {
  level: int;
  tags: [string] = [];
  drop_weights: [int] = [];

  check {
    level >= 1 && level <= MAX_LEVEL;
    tags.isUnique();
    all weight in drop_weights {
      weight > 0;
    }
  }
}
```

常用注解：

- `@idAsEnum(Name)`：把加载到的 record key 填充进手动声明的空 enum，并用于生成强类型 C# key。
- `@struct`：让 sealed value-like type 生成 C# struct。
- `@expand`：让 Excel 相邻列展开成嵌套 object 字段。
- `@localized`：声明字段值按语言维度变化。
- `@singleton`：声明数据集中该 type 有且仅有一条 record。

---

## Excel 编写要点

- 每个导入 sheet 必须有 `id`、`Id` 或 `ID` 列；它是 record key，不是 CFT 字段。
- record key 是 string identifier。
- 名为 `#` 的表头是可选导入控制列；数据行中该列单元格为 `##` 时，整行在
  `id` 或字段解析前跳过。
- CFT 字段类型写成 `&Item`、`[&Item]` 或 `{string: &Item}` 时，表格单元格写
  `&sword_01` 这类 key-only 记录引用。
- 普通 object 字段类型写成 `Stats`、`Reward` 时表示内联对象。表格对象单元格可写
  `Stats{hp: 100, attack: 50}`，多态对象也用 `ConcreteType{...}`。
- 裸字符串保持字符串语义。导出的 JSON 和 MessagePack 中，引用字段保存为
  `"sword_01"` 这类纯 key 字符串，而不是 `"Item.sword_01"`。

`coflow build` 会在 `coflow.yaml` 同级维护 `coflow.enum.lock.json`，用于稳定
`@idAsEnum` 的整数值。Excel 行顺序变化时，已有生成 enum 的整数值保持不变；
新的数据驱动枚举变体会追加到 lockfile。该文件可提交到版本库；它不属于
生成输出目录。占位 enum 带 `@flag` 时，新变体按 `1, 2, 4, ...` 分配，
不会自动生成 `None = 0`。

check 中常用内建函数包括 `len`、`contains`、`isUnique`、`min`、`max`、`sum`、
`keys`、`values` 和 `matches`。`isUnique` 支持可比较标量数组（`int`、`bool`、
`string`、`enum` 及其 nullable 形式），不支持对象数组。

---

## 运行时依赖

生成的 JSON C# 加载器使用 `Newtonsoft.Json`。生成的 MessagePack C# 加载器
使用 MessagePack-CSharp，并走显式 `MessagePackReader` 路径，面向普通 .NET 和
Unity/IL2CPP 风格环境。

默认生成入口是 `CoflowTables`：

```csharp
var tables = CoflowTables.Load(dataDir);
var item = tables.TbItem.Get("potion");
var maybeItem = tables.TbItem.Find("potion");
```

每张表通过 `Tb{TypeName}` 访问器暴露 `Get`、`Find`、`TryGet` 和只读列表 API。
生成 loader 面向受信 Coflow exporter 产物；不会生成自定义 `CftLoadException`。
JSON 导出不会为无记录的表写空 `[]` 文件，C# JSON loader 会把缺失的空表文件
视为空表。
