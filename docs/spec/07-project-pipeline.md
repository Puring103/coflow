# 项目管线规格

项目加载层负责把 `coflow.yaml`、schema、数据 source、数据导出和代码生成串成完整 CLI 工作流。它不重新实现底层 schema 编译、Excel/CFD 解析、数据模型构建或 C# 渲染规则。

实现边界：

- `coflow-project` 负责项目配置 shape、路径解析、schema 文件发现、CFT 编译和 CFT 诊断映射；provider 私有字段只作为 options 保留。
- `coflow-pipeline` 负责项目执行流水线：schema 编译后的控制流、provider registry dispatch、loader resolve/preflight/load、数据模型构建、check 诊断聚合、artifact 安全写入。
- CLI 根包是内置 provider 的组合根：注册 Excel/Lark/CFD loader、JSON/MessagePack exporter 和 C# codegen，然后调用 pipeline API、输出成功消息和诊断。
- `coflow-lsp` 依赖 `coflow-project` 和 `coflow-api` 做 schema-only 加载与诊断渲染，不依赖 `coflow-pipeline`。

---

## 输入

- `coflow.yaml`
- 项目配置中发现的 CFT schema 文件
- 数据 source 定义
- CLI 命令和命令行覆盖项

---

## 配置发现和路径规则

命令的 `CONFIG_OR_DIR` 参数由 project 层统一解析：

- 未提供时，在当前目录查找 `coflow.yaml`，然后查找 `coflow.yml`。
- 参数是目录时，在该目录下查找 `coflow.yaml`，然后查找 `coflow.yml`。
- 参数是文件时，直接作为项目配置读取。

项目相对路径均以配置文件所在目录为根。`schema` 可以是单个精确小写 `.cft`
文件、单个目录或文件/目录列表；目录会递归发现精确小写 `.cft` 文件，
忽略其他扩展名。schema
文件按 module id 排序后注册到 `CftContainer`，因此同一项目在不同文件系统
遍历顺序下仍保持稳定。绝对 schema 路径允许指向项目根之外的文件。

`coflow.yaml` 的顶层和 outputs 容器使用严格字段集。source 只能使用通用字段
`type`、`path`、`url` 加 provider options；output 只能使用通用字段 `type`、`dir`
加 provider options。source 必须且只能设置 `path` 或 `url` 之一。旧字段 `file`、
`dir` 和 `lark_sheet` 会在 YAML 反序列化阶段被拒绝。`columns` 映射拒绝重复 Excel
header key，避免 YAML map 后写覆盖导致隐式丢配置。

---

## 职责

- 解析项目配置 shape 并解析项目相对路径。
- 发现并编译 schema，得到 `CftContainer`。
- 通过 `coflow-api::ProviderRegistry` 选择 `DataLoader`，并把 source resolve 下沉给 loader provider。
- 编排 CLI 命令，包括数据加载、数据模型构建和 check 诊断处理。
- 根据 `outputs.data.type` 从 registry 查找 `DataExporter`，并写入 provider 返回的 `ArtifactSet`。
- 根据 `outputs.code.type` 从 registry 查找 `CodeGenerator`，并把项目配置中的 codegen options 传入 provider。

---

## 阶段化打开和校验

项目打开分为三个阶段：

- `Project::open_schema_only`：只解析并反序列化 `coflow.yaml`；不要求数据源文件存在。
- `Project::schema_diagnostic_set`：schema-only 命令在打开项目后调用，聚合 schema path、output 配置和 source 通用 shape 诊断；仍不要求数据源文件存在。
- `Project::data_diagnostic_set`：在 schema-only 之上校验数据阶段 source，要求本地 `path` 指向存在的文件或目录；`url` 不做本地路径存在性校验，provider 私有字段由 loader 后续校验。
- `Project::codegen_diagnostic_set`：校验 codegen 命令需要的 output 通用 shape；C# namespace 这类 provider option 不由 project 层解释。

命令阶段矩阵：

| 命令 | Schema | 数据源存在性 | Data model | Codegen 目标 |
| --- | --- | --- | --- | --- |
| `cft check` | 是 | 否 | 否 | 否 |
| `lsp` | 是 | 否 | 否 | 否 |
| `check` | 是 | 是 | 是 | 否 |
| `build` | 是 | 是 | 是 | 可选 |
| `export json/messagepack` | 是 | 是 | 是 | 否 |
| `codegen csharp` | 是 | 否 | 否 | 是 |

本地目录 source 没有显式 `type` 时，pipeline 会把目录交给所有注册 loader 的
`resolve` 阶段，各 loader 自己发现可处理的文件并返回 `ResolvedSource`。单文件或
远端 URL source 先通过 registry probe 选择 loader；若多个 loader 同等匹配，则要求
显式设置 `type`。

---

## Schema 覆盖与 LSP 边界

`compile_schema_project` 支持 `--stdin-path` 覆盖单个 schema 文件内容，用于
编辑器把未保存内容传入编译。覆盖目标必须匹配已经配置的 schema 文件：

- 可按 project-relative module id 匹配，例如 `schema/main.cft`。
- 也可按从项目根解析出的实际文件路径匹配。
- 未匹配到任何已配置 schema 文件时返回 CLI 错误：
  `` `--stdin-path ...` is not part of the configured schema ``。

LSP 只使用 schema-only 加载和 schema 编译，不进入数据加载、data model、导出
或 codegen 阶段。它维护打开文档的内存覆盖层，发布项目 schema 诊断，并提供
completion、hover、definition、document symbol、formatting 和 semantic tokens。

---

## 配置解析错误边界

`coflow.yaml` 的顶层和嵌套通用结构使用严格字段集。未知顶层/output 字段、旧
source 字段、YAML 语法错误、重复 `columns` key、无法读取配置文件等问题发生在
结构化项目诊断之前，CLI 以不可聚合错误返回；这些问题不会进入 `PROJECT-001`
诊断列表。

`PROJECT-001` 只覆盖配置文件已经成功读取和反序列化之后的可聚合
项目预检问题，例如路径为空、schema/source/output 配置缺失或类型不支持。

---

## 产物写入安全

所有会写产物的命令都在写入前执行可聚合诊断和 artifact preflight：

- `build`：先完成项目、schema、数据加载、data model、引用和 check；再检查
  data output path；如果配置了 `outputs.code`，还会检查 C# codegen preflight
  和 code output path。任一诊断存在时不写数据，也不写代码。
- `export json/messagepack`：先完成数据校验，再检查目标输出目录；有诊断时
  不写任何导出文件。
- `codegen csharp`：先完成 schema-only 校验、codegen 配置校验、schema 编译、
  codegen preflight 和 code output path 检查；有诊断时不读写 enum lockfile，
  不替换 C# 输出目录，也不生成新 `.cs` 文件。

artifact preflight 会检查输出目标是否能被 Coflow 安全接管，例如目标路径已经存在
但不是目录、输出目录指向项目根或包含 schema/source、多个输出目录互相重叠。
目录不存在时由写入阶段创建。staging、commit、lockfile 读写和 artifact path
安全检查失败会返回 `DiagnosticSet`，使用 `SourceLocation::Artifact` 定位。

数据导出和 C# codegen 的输出目录由 Coflow 完全接管。写入阶段先创建同级
staging 目录并写入完整产物；所有文件成功写入后，再用 staging 目录替换目标
输出目录。目标目录内旧文件、子目录、人工文件和其他工具产物均不会保留。
因此 `outputs.data.dir` 和 `outputs.code.dir` 必须只用于 Coflow 生成物。

C# codegen 的 `coflow.enum.lock.json` 写在 `coflow.yaml` 同级，而不是 C# 输出
目录内。codegen 会先读取并合并 lockfile，生成完整 C# staging 目录和 lockfile
staging 文件；全部 staging 成功后再提交写入。若 `.cs` staging 或 lockfile
staging 任一步失败，既有输出目录和既有 lockfile 保持不变。若提交阶段发生
文件系统错误，pipeline 会尽力回滚 lockfile 和旧输出目录，并返回 artifact
diagnostic。

`build` 的 codegen 是可选阶段。项目没有配置 `outputs.code` 时，`build` 仍会
完成数据校验和数据导出，但不会生成代码，也不会要求 code output 配置存在。

---

## Check 诊断处理

`coflow-pipeline::check_project` 在 schema、数据加载、data model 或 CFT `check`
产生可定位错误时，返回 `PipelineOutcome::Diagnostics`，不返回 `Err`。`Err` 只表示
配置文件发现/读取/YAML 解析这类还无法进入结构化诊断模型的命令级错误。

`type: lark-sheet` 或可 probe 的 Feishu/Lark URL source 由 Lark loader 处理。
它用 `app_id`/`app_secret` 获取 tenant access token；`url` 支持 `/sheets/{token}`
或 `/wiki/{token}`，wiki 链接会通过 wiki node API 解析到真实电子表格 token；
已知电子表格 token 可用 `url: lark:<spreadsheet_token>` 表达。读取到的飞书
单元格会转换为共享 table loader 的 `TableSource`，因此 `sheets`、`key`、
`columns` 的语义与 Excel 相同。暂不支持飞书多维表格/Base。

CLI `coflow check` 对 `PipelineOutcome::Diagnostics` 的处理规则：

- 退出码为非 0。
- 默认 human 输出写入 stderr，包含诊断 code、stage、项目相对文件路径、sheet/cell（如果来自 Excel）和 message。
- `--json` 输出写入 stdout，格式为 `{"diagnostics":[...]}`，退出码仍为非 0。
- check 诊断使用 `CFD-CHECK-*` code，stage 为 `CHECK`。

---

## 非职责

- 不重新实现 CFT parser、schema compiler 或 schema 反射模型。
- 不重新实现 Excel 单元格解析、飞书 API 读取或 CFD 文本解析；这些由注册进 registry 的 loader provider 负责。
- 不拥有 JSON 或 MessagePack 的 schema-aware 导出遍历规则；这些由 `coflow-api::export` 以及具体 exporter provider 负责。
- 不拥有 C# 类型映射、模板渲染或加载器生成规则；codegen provider 接收编译后的 `CftContainer` 和 options。
- 不充当生成出的 C# trusted artifact loader。
