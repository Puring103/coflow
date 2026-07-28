# 给策划的配置维护路径

Coflow 不要求所有数据都改成同一种格式。策划可以继续在 Excel 中批量编辑数值，在 CSV 中维护易于版本比较的单表，在 CFD 中表达复杂嵌套结构。

## 需要理解的概念

- CFT type 定义一类数据的字段和规则。
- record key 是稳定身份，引用指向 key，而不是可变的显示名。
- 默认值由 schema 统一定义，source 只需维护例外值。
- `check {}` 表达业务规则，失败会精确定位到 record 和字段。

## 日常流程

1. 从编辑器或 `coflow data list` 确认要修改的记录。
2. 在原始 Excel、CSV 或 CFD 文件中编辑，或使用结构化 patch。
3. 运行 `coflow check <project>`。
4. 根据诊断的文件、sheet、行列、record 和 field path 修复问题。
5. 检查通过后再提交变更。

可视化编辑器用于表格、记录、关系图和诊断浏览；VS Code/LSP 更适合直接编辑 CFT/CFD；AI agent 应使用 CLI 查询和 writer API，不应跳过校验。

建议继续阅读 [数据维护](data-authoring.md)、[编辑器与 LSP](editor.md) 和 [最佳工作流](best-workflow.md)。
