---
name: coflow-data
description: "Coflow 数据文件与数据源维护：当用户需要编写或修改 .cfd、Excel/CSV 数据、创建数据文件、同步表头、添加/修改/删除/重命名记录、使用 data patch、处理单元格值语法或修复 schema/data 不一致时使用。"
---

# Coflow Data

使用本 skill 维护外部 Coflow 项目的数据源和记录内容。Schema 设计和 `.cft` 修改使用 `coflow-schema`。

## 编辑流程

1. 先读取 schema 和 source：`coflow schema inspect <project>`、`coflow data sources <project>`。
2. 需要定位记录时运行 `coflow data list <project>` 或 `coflow data get <project> ...`。
3. 新建本地文件用 `coflow data create-file`；字段变化后用 `coflow data sync-header`。
4. 少量记录增删改用 `coflow data patch <project> --patch '<json>'`；需要从文件读取时显式用 `--patch-file patch.json`。
5. 复杂 CFD 整文件整理用 `coflow data write-file <project> --file data/file.cfd --check`。
6. 表格 source 不用整文件写入；通过 provider writer 和 `data patch` 修改。
7. 完成后运行 `coflow check <project>`，并检查 `write_ok`、`check_ok`、`applied`、`failed` 和 `diagnostics`。

## 常用命令

```powershell
coflow data sources <project>
coflow data list <project> --type Item
coflow data get <project> Item.sword_fire
coflow data create-file <project> --file data/items.csv --type Item --provider csv
coflow data create-file <project> --file data/items.cfd --provider cfd
coflow data sync-header <project> --file data/items.csv --type Item
coflow data write-file <project> --file data/items.cfd --check
coflow data patch <project> --patch '{"ops":[]}'
coflow data patch <project> --patch-file patch.json
coflow check <project>
```

## Data Patch 注意事项

- `insert_record` 必须指定 `file`；`set_field`、`rename_record`、`delete_record` 可用 `file` 作 guard。
- `path` 使用结构化段，例如 `{ "kind": "field", "value": "stats" }`、`{ "kind": "index", "value": 0 }`、`{ "kind": "dict_key", "value": "Element.Fire" }`。
- `$ref` 只写 record key；目标类型来自 CFT 字段类型。
- `data patch` 整批规划、预检并原子写入；writer、重建或提交失败会补偿已写来源，
  此时 `applied` 为空且 `affected_files` 不应作为成功结果使用。
- CFT `check {}` 不阻止写入；写入后重建项目并返回诊断。

## Reference

- 数据维护决策、patch 示例和收尾检查：读 `references/data-maintenance.md`。
- CFD 文本数据语法：读 `references/cfd.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/03-language/02-cfd>。
- Excel/CSV 单元格值语法：读 `references/cell-value.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/03-language/03-cell-value>。
- 数据源和 Provider 总览：读 `references/sources-overview.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/04-sources/01-overview>。
- 表格、Excel、CSV 细节：按需读取 `references/table-source.md`、`references/excel.md`、`references/csv.md`。
- CLI data 命令完整行为：读 `references/cli.md`，公开链接 <https://puring103.github.io/coflow/docs/reference/08-cli>。
