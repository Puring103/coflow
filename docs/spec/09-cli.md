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
  - file: data/rpg.xlsx
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

`schema` 可以是一个文件、一个目录，或文件/目录列表。schema 目录会递归查找
`.cft` 文件。

`sources` 配置 Excel 输入。每个 source 包含 workbook `file` 和一个或多个
`sheets`。sheet 可配置：

- `sheet`：Excel worksheet 名称。
- `type`：可选 CFT 类型名；省略时使用 sheet 名称。
- `columns`：可选 Excel 表头文本到 CFT 字段名的映射。

导入的 Excel sheet 必须有 `id` 表头列作为 record key。名为 `#` 的表头是
可选导入控制列：数据行中该列单元格为 `##` 时，整行在读取 `id` 或字段前跳过。

`outputs.data.type` 支持 `json` 或 `messagepack`。

`outputs.code.type` 目前支持 `csharp`。`outputs.code.namespace` 供 C# codegen
使用，并可被命令行参数覆盖。

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
添加 schema 文件和 Excel source。

`init` 会先检查目标 `coflow.yaml` 是否存在。若已存在，命令失败，并且不会
创建目录或修改目标项目。

### `coflow cft check [CONFIG_OR_DIR] [--json] [--stdin-path PATH]`

编译配置中的 CFT schema 文件并输出 schema 诊断。

```powershell
cargo run -- cft check examples/rpg
cargo run -- cft check examples/rpg --json
```

该命令不要求 Excel 文件存在。它校验 schema path、output 配置形状和 source
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

### `coflow cft lsp [CONFIG_OR_DIR]`

启动 CFT language server，供编辑器集成使用。

```powershell
cargo run -- cft lsp examples/rpg
```

language server 使用 schema-only 项目加载，不要求 Excel 文件存在。

支持的编辑器行为：

- 为配置中的项目 schema 文件发布诊断，包含未保存的打开文档内容。
- 对正在编辑的文件，诊断优先使用打开文档 URI。
- 为 CFT 关键字、primitive 和命名类型、enum variant、annotation、默认值、
  字段、check 内建函数和 check 上下文提供补全。
- 为 schema 符号、字段、常量、enum variant 和可解析的跨文件引用提供 hover
  与 go-to-definition。
- 提供 document symbols、document formatting 和 full semantic tokens。
- 遵循标准 JSON-RPC `shutdown` / `exit` 生命周期。

### `coflow check [CONFIG_OR_DIR] [--json]`

运行完整校验管线，但不写产物：

1. 项目配置 preflight。
2. schema 发现与编译。
3. Excel 加载。
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

只有项目、schema、Excel、data model 和 check 诊断都为空时，`build` 才会写
数据产物。如果省略 `outputs.code`，`build` 是 data-only build，只写配置的
数据输出。如果配置了 `outputs.code`，则还会在 C# codegen preflight 成功后
生成代码。

覆盖项：

- `--data-out DIR`：覆盖 `outputs.data.dir`。
- `--code-out DIR`：覆盖 `outputs.code.dir`。
- `--namespace NAME`：覆盖 `outputs.code.namespace`。

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

### `coflow codegen csharp [CONFIG_OR_DIR] [--out DIR] [--namespace NAME]`

生成 C# 运行时加载代码，但不加载 Excel 数据。

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

`codegen csharp` 不要求 Excel source 存在。对于 `@keyAsEnum`，它会生成 schema
声明的 enum 文件并保留 `coflow.enum.lock.json` 中已有的 variant，但无法新增
data-driven variant，因为它不加载数据。`coflow build` 已经加载 data model，
因此可以新增这些 variant。新 variant 追加到 lockfile，已有 variant 的整数值
保持不变。

codegen 会在读取/写入 lockfile、清理旧 `.cs` 文件或写入新文件前执行
preflight。命名错误会产生诊断，并且不会修改既有生成输出。

成功写入时，codegen 会按需创建输出目录，移除该目录顶层旧 `.cs` 文件，然后
写入本次生成的新文件。enum lockfile 作为 `coflow.enum.lock.json` 单独维护。

---

## 命令矩阵

| 命令 | 需要 schema | 需要 Excel 文件 | 构建 data model | 执行 check | 写产物 |
| --- | --- | --- | --- | --- | --- |
| `init` | 否 | 否 | 否 | 否 | 创建项目文件 |
| `cft check` | 是 | 否 | 否 | 否 | 否 |
| `cft lsp` | 是 | 否 | 否 | 否 | 否 |
| `check` | 是 | 是 | 是 | 是 | 否 |
| `build` | 是 | 是 | 是 | 是 | 数据和可选代码 |
| `export json` | 是 | 是 | 是 | 是 | JSON 数据 |
| `export messagepack` | 是 | 是 | 是 | 是 | MessagePack 数据 |
| `codegen csharp` | 是 | 否 | 否 | 否 | C# 代码 |

输出格式、错误码和非阻塞诊断收集规则见 [10-diagnostics.md](10-diagnostics.md)。
