# 项目管线规格

项目加载层负责把 `coflow.yaml`、schema、Excel source、数据导出和代码生成串成完整 CLI 工作流。它不重新实现底层 schema 编译、Excel 解析、数据模型构建或 C# 渲染规则。

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
- 调用 C# codegen，并把项目配置中的 codegen options 传给 `coflow-codegen-csharp`。

---

## 非职责

- 不重新实现 CFT parser、schema compiler 或 schema 反射模型。
- 不重新实现 Excel 单元格解析或跨表引用解析；这些由 `coflow-loader-excel` 负责。
- 不拥有 JSON 或 MessagePack 的 schema-aware 导出遍历规则；这些由 `coflow-exporter-core` 以及具体 exporter 负责。
- 不拥有 C# 类型映射、模板渲染或加载器生成规则；codegen 接收编译后的 `CftContainer` 和 options。
- 不充当生成出的 C# trusted artifact loader。
