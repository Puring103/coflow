# 编辑器与 LSP

## 可视化编辑器

CFD Editor 使用与 CLI 相同的 `coflow-runtime` 加载项目，不另外实现一套解析和校验语义。它提供：

- 文件与类型导航。
- 记录表格和字段编辑。
- 引用关系图。
- 项目诊断和源位置跳转。
- 基于 writer transaction 的可撤销变更。

编辑器的项目 generation 是不可变快照。当文件重载或写入完成时，后端构建新 generation 并通过 revision 防止旧请求覆盖新状态。

## VS Code 与 LSP

Coflow LSP 为 CFT 和 CFD 提供诊断、补全、hover、定义跳转和语义高亮。LSP 适合文本编辑和快速导航，但完整项目交付仍应运行 `coflow check` 或 `coflow build`。

## 与 CLI 的分工

编辑器和 LSP 负责交互和快速反馈；CLI 负责可重复的检查、批处理、CI 和 artifact 发布。三者共用 schema、runtime、provider registry 和诊断合同。
