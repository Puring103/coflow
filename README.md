# Coflow

Coflow 是面向游戏配置数据的管线工具，用于把 CFT schema、Excel 配置表、
数据校验、JSON/MessagePack 导出和 C# 运行时加载代码串成一个可重复执行的
工作流。

---

## 能力概览

- 编译 CFT schema 文件。
- 按项目配置加载 Excel sheet 和 CFD 文本数据，构建类型化 data model。
- 执行 schema 中的 `check {}` 规则。
- 导出 JSON 或 MessagePack 数据文件。
- 生成适用于 .NET 和 Unity 风格项目的 C# 运行时加载代码。

---

## 快速开始

运行 RPG 示例：

```powershell
cargo run -- check examples/rpg
cargo run -- build examples/rpg
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
cargo run -- cft check examples/rpg
cargo run -- export json examples/rpg
cargo run -- codegen csharp examples/rpg
```

使用 MessagePack 时，把 `coflow.yaml` 中的 `outputs.data.type` 改为
`messagepack`，再运行：

```powershell
cargo run -- build examples/rpg
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
cargo run -- init my-config
cargo run -- check examples/rpg
cargo run -- build examples/rpg
cargo run -- export json examples/rpg --out generated/data
cargo run -- export messagepack examples/rpg --out generated/data
cargo run -- codegen csharp examples/rpg --out generated/csharp --namespace Game.Config
cargo run -- lsp examples/rpg
```

AI/data automation 命令默认输出 JSON：

```powershell
cargo run -- schema inspect examples/rpg
cargo run -- schema files examples/rpg
cargo run -- data sources examples/rpg
cargo run -- data list examples/rpg --type Item
cargo run -- data get examples/rpg Item.sword
cargo run -- data patch examples/rpg --patch patch.json
```

`data patch` 通过和编辑器相同的 provider writer 层写入数据。它不会用 CFT
`check {}` 阻拦写入；写完后会重建项目并返回诊断，供 agent 继续修正。

完整命令行为见 [CLI 命令规格](docs/spec/09-cli.md)。

---

## CFT 简览

CFT 描述配置数据形状、默认值、record-key 引用和校验规则。

常量：

```cft
const MAX_LEVEL: int = 100;
```

枚举：

```cft
@display("物品稀有度")
enum Rarity {
  Common = 0,
  Rare = 10,
  Epic = 20,
}
```

类型和字段：

```cft
@display("物品")
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
    tags.unique();
    all weight in drop_weights {
      weight > 0;
    }
  }
}
```

常用注解：

- `@idAsEnum(Name)`：把加载到的 record key 填充进手动声明的空 enum，并用于生成强类型 C# key。
- `@display("text")`：在支持的位置输出可读说明。
- `@deprecated`：把生成的 C# symbol 标记为 obsolete。
- `@struct`：让 sealed value-like type 生成 C# struct。
- `@expand`：让 Excel 相邻列展开成嵌套 object 字段。

---

## Excel 编写要点

- 每个导入 sheet 必须有 `id`、`Id` 或 `ID` 列；它是 record key，不是 CFT 字段。
- record key 是 string identifier。
- 名为 `#` 的表头是可选导入控制列；数据行中该列单元格为 `##` 时，整行在
  `id` 或字段解析前跳过。
- object 引用必须显式写为 typed ref，例如 `@Item.sword_01` 或
  `@DropTable.drop_01.rewards[0]`。
- 同类型直接引用可写为 `&sword_01`。路径引用仍必须使用显式 `@Type.key` 根。
- 裸字符串保持字符串语义。导出的 JSON 和 MessagePack 中，引用字段保存为
  `"sword_01"` 这类纯 key 字符串，而不是 `"Item.sword_01"`。

`coflow build` 会在 `coflow.yaml` 同级维护 `coflow.enum.lock.json`，用于稳定
`@idAsEnum` 的整数值。Excel 行顺序变化时，已有生成 enum 的整数值保持不变；
新的数据驱动枚举变体会追加到 lockfile。该文件可提交到版本库；它不属于
生成输出目录。占位 enum 带 `@flag` 时，新变体按 `1, 2, 4, ...` 分配，
不会自动生成 `None = 0`。

check 中常用内建函数包括 `len`、`contains`、`unique`、`min`、`max`、`sum`、
`keys`、`values` 和 `matches`。`unique` 支持可比较标量数组（`int`、`bool`、
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

---

## 内部 crate 边界

- `coflow-api` 只定义 provider trait、诊断、来源位置、产物和写入契约。
- `coflow-project` 负责项目配置、路径解析、配置诊断、schema 文件发现和项目初始化。
- `coflow-engine` 负责共享项目运行时：schema 编译、source resolve/load、data model、check、诊断和 source/record/file 索引。
- `coflow-builtins` 负责默认 provider registry 注册，供 CLI、editor 和 LSP 装配使用。
- 根 crate `coflow` 负责 CLI 命令编排、终端/JSON 输出、导出和 codegen 产物暂存与提交。
- `editors/cfd-editor/src-tauri` 是 editor 的后端宿主，复用 `coflow-engine` 并只保留 editor wire DTO、graph/table 视图和写入命令桥接。
- provider 共享算法分别位于 `coflow-loader-table-core` 和 `coflow-exporter-core`，不放在 `coflow-api`。

---

## 规格文档

详细文档统一位于 `docs/spec/`：

- [CFT 语言规格](docs/spec/01-cft.md)
- [数据模型](docs/spec/02-data-model.md)
- [Schema API](docs/spec/02-schema-api.md)
- [单元格值语法](docs/spec/03-cell-value.md)
- [Excel 加载器规格](docs/spec/04-excel-loader.md)
- [JSON 导出格式](docs/spec/05-json-export.md)
- [C# 代码生成规格](docs/spec/06-csharp-codegen.md)
- [项目管线规格](docs/spec/07-project-pipeline.md)
- [MessagePack 导出格式](docs/spec/08-messagepack-export.md)
- [CLI 命令规格](docs/spec/09-cli.md)
- [诊断规格](docs/spec/10-diagnostics.md)
- [项目介绍页](docs/spec/11-project-architecture.html)
- [CFD 文本配置语法](docs/spec/12-cfd.md)
