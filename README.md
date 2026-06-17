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
  - dir: data
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

`schema` 指向一个 CFT 文件、schema 目录，或文件/目录列表。`sources` 支持
`file` 或 `dir`：文件源可以是 `.xlsx` / `.xlsm` / `.xls` / `.cfd`，目录源会递归
加载支持的 Excel 和 CFD 文件。Excel 源未配置 `sheets` 时默认把 workbook 中每个
sheet 名当作 CFT 类型名、表头当作字段名；配置 `sheets` 时可显式映射 sheet、类型
和列头。`outputs.data.type` 支持 `json` 或 `messagepack`；`outputs.code.type`
目前支持 `csharp`。

---

## 常用命令

```powershell
cargo run -- init my-config
cargo run -- check examples/rpg
cargo run -- build examples/rpg
cargo run -- export json examples/rpg --out generated/data
cargo run -- export messagepack examples/rpg --out generated/data
cargo run -- codegen csharp examples/rpg --out generated/csharp --namespace Game.Config
cargo run -- cft lsp examples/rpg
```

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
@keyAsEnum("ItemId")
type Item {
  name: string;
  rarity: Rarity = Rarity.Common;
  tags: [string] = [];
  attributes: {string: int} = {};
}
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
    unique(tags);
    all weight in drop_weights {
      weight > 0;
    }
  }
}
```

常用注解：

- `@keyAsEnum("Name")`：按加载到的 record key 生成 C# enum。
- `@display("text")`：在支持的位置输出可读说明。
- `@deprecated`：把生成的 C# symbol 标记为 obsolete。
- `@struct`：让 sealed value-like type 生成 C# struct。
- `@expand`：让 Excel 相邻列展开成嵌套 object 字段。

---

## Excel 编写要点

- 每个导入 sheet 必须有 `id` 列；它是 record key，不是 CFT 字段。
- record key 是 string identifier。
- 名为 `#` 的表头是可选导入控制列；数据行中该列单元格为 `##` 时，整行在
  `id` 或字段解析前跳过。
- object 引用必须显式写为 typed ref，例如 `@Item.sword_01` 或
  `@DropTable.drop_01.rewards[0]`。
- 同类型直接引用可写为 `&sword_01`。路径引用仍必须使用显式 `@Type.key` 根。
- 裸字符串保持字符串语义。导出的 JSON 和 MessagePack 中，引用字段保存为
  `"sword_01"` 这类纯 key 字符串，而不是 `"Item.sword_01"`。

`coflow build` 会在 `coflow.yaml` 同级维护 `coflow.enum.lock.json`，用于稳定
`@keyAsEnum` 的整数值。Excel 行顺序变化时，已有生成 enum 的整数值保持不变；
新的数据驱动枚举变体会追加到 lockfile。该文件可提交到版本库；它不属于
生成输出目录。

check 中常用内建函数包括 `len`、`contains`、`unique`、`min`、`max`、`sum`、
`keys`、`values` 和 `matches`。`unique` 支持可比较标量数组（`int`、`bool`、
`string`、`enum` 及其 nullable 形式），不支持对象数组。

---

## 运行时依赖

生成的 JSON C# 加载器使用 `Newtonsoft.Json`。生成的 MessagePack C# 加载器
使用 MessagePack-CSharp，并走显式 `MessagePackReader` 路径，面向普通 .NET 和
Unity/IL2CPP 风格环境。

---

## 内部 crate 边界

- `coflow-project` 负责项目配置、路径解析和 CFT schema 编译。
- `coflow-pipeline` 负责 `check`、`build`、`export` 和 `codegen` 命令的项目执行。
- CLI crate 负责命令行解析和终端输出。

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
