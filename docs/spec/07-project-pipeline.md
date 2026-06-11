# 项目管线规格

项目加载层负责把 `coflow.yaml`、schema、Excel source、数据导出和代码生成串成完整 CLI 工作流。它不重新实现底层 schema 编译、Excel 解析、数据模型构建或 C# 渲染规则。

实现边界：

- `coflow-project` 负责项目配置、路径解析、schema 文件发现、CFT 编译和 CFT 诊断映射。
- `coflow-pipeline` 负责项目执行流水线：schema 编译后的控制流、Excel 加载、check 诊断聚合、数据导出和 C# codegen。
- CLI 根包只负责命令行参数解析、调用 pipeline API、输出成功消息和诊断。
- `coflow-cft-lsp` 只依赖 `coflow-project`，不依赖 `coflow-pipeline`。

---

## 输入

- `coflow.yaml`
- 项目配置中发现的 CFT schema 文件
- Excel source 定义
- CLI 命令和命令行覆盖项

---

## 职责

- 解析项目配置并解析项目相对路径。
- 发现并编译 schema，得到 `CftContainer`。
- 构建已经解析的 `ExcelSource` 值，并调用 `coflow-loader-excel`。
- 编排 CLI 命令，包括 Excel 加载、数据模型构建和 check 诊断处理。
- 根据 `outputs.data.type` 调用 JSON 或 MessagePack 导出：
  - `json`：调用 `coflow-exporter-json`，输出 `<TypeName>.json`。
  - `messagepack`：调用 `coflow-exporter-messagepack`，输出 `<TypeName>.msgpack`。
- 调用 C# codegen，并把项目配置中的 codegen options 传给 `coflow-codegen-csharp-json` 或 `coflow-codegen-csharp-messagepack`。

---

## 阶段化打开和校验

项目打开分为三个阶段：

- `Project::open_schema_only`：解析 `coflow.yaml`，校验 schema path、output 配置和 source 配置形状；不要求 Excel workbook 存在。
- `Project::validate_for_data`：在 schema-only 之上校验数据阶段 source，要求 workbook 文件存在且每个 source 至少有一个 sheet。
- `Project::validate_for_codegen`：校验 C# codegen 需要的 `outputs.code.type: csharp` 和 `outputs.data.type: json | messagepack`；不要求 Excel workbook 存在。

命令阶段矩阵：

| Command | Schema | Excel source existence | Data model | Codegen target |
| --- | --- | --- | --- | --- |
| `cft check` | yes | no | no | no |
| `cft lsp` | yes | no | no | no |
| `check` | yes | yes | yes | no |
| `build` | yes | yes | yes | optional |
| `export json/messagepack` | yes | yes | yes | no |
| `codegen csharp` | yes | no | no | yes |

---

## 非职责

- 不重新实现 CFT parser、schema compiler 或 schema 反射模型。
- 不重新实现 Excel 单元格解析或跨表引用解析；这些由 `coflow-loader-excel` 负责。
- 不拥有 JSON 或 MessagePack 的 schema-aware 导出遍历规则；这些由 `coflow-exporter-core` 以及具体 exporter 负责。
- 不拥有 C# 类型映射、模板渲染或加载器生成规则；codegen 接收编译后的 `CftContainer` 和 options。
- 不充当生成出的 C# trusted artifact loader。
