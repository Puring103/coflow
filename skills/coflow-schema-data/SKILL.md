---
name: coflow-schema-data
description: "Coflow 项目配置、schema 与数据维护工作流：当用户需要修改 coflow.yaml、配置 sources/outputs/dimensions、修改 CFT schema、创建或同步数据文件、添加/修改/删除配置记录、修复 schema/data 不一致、运行 Coflow 校验或让 AI 维护游戏配置数据时使用。"
---

# Coflow Schema Data

使用本 skill 维护已有 Coflow 项目的 schema 和数据文件。优先使用项目里的 `coflow` CLI；在仓库开发环境中可用 `cargo run -- <command>` 替代安装后的 `coflow <command>`。

## 基本流程

1. 先定位项目配置：读取 `coflow.yaml` 或用户指定的 `CONFIG_OR_DIR`。
2. 读取 schema：优先运行 `coflow schema files <project>` 或查看配置中的 `.cft` 文件。
3. 读取数据概况：需要完整数据时运行 `coflow data sources <project>`、`coflow data list <project>`、`coflow data get <project> ...`。
4. 修改 `coflow.yaml` 时直接编辑 YAML，但必须保留项目相对路径语义；改完运行 `coflow schema inspect <project>` 或 `coflow check <project>`。
5. 修改 schema 时，优先用 `coflow schema write-file <project> --file <schema.cft> --stdin --check` 写入配置内 `.cft`；命令不可用时才直接编辑文件。
6. schema 字段变化影响表格时，运行 `coflow data sync-header <project> --file <file> --type <Type>`。
7. 创建本地数据文件时，运行 `coflow data create-file`；CSV/XLSX 会创建表头，CFD 只创建空文件。
8. 添加、修改、删除单条或少量记录时，使用 `coflow data patch <project> --patch patch.json`，不要手写绕过 provider writer 的表格行。
9. 需要重写复杂 CFD 文本文件时，使用 `coflow data write-file <project> --file <data.cfd> --stdin --check`；只用于 `.cfd`，表格仍走 patch/create/sync 命令。
10. 完成后至少运行 `coflow check <project>`；在仓库开发中还要遵守项目自己的检查要求。

## 写数据规则

- 修改项目配置、复杂 patch、Excel 多 sheet、远端 source、维度/变体时，先读 `references/editing-playbook.md`。
- 添加记录使用 `insert_record`。
- 修改字段使用 `set_field`。
- 删除记录使用 `delete_record`。
- `file` 可作为 guard，避免写错数据源；对用户指定文件的写入应尽量带上。
- `data patch` 是顺序执行，成功的 op 不会因后续失败自动回滚。报告中出现 `failed` 时，要明确告诉用户发生了部分落盘。
- CFT `check {}` 不阻拦写入；命令会写完后重建项目并返回诊断。写入后必须查看 `write_ok`、`check_ok` 和 `diagnostics`。

## 常用命令

```powershell
coflow schema inspect <project>
coflow schema files <project>
coflow schema write-file <project> --file schema/main.cft --stdin --check
coflow schema write-file <project> --file schema/main.cft --stdin --dry-run --check
coflow data sources <project>
coflow data list <project> --type Item
coflow data get <project> Item.sword
coflow data create-file <project> --file data/items.csv --type Item --provider csv
coflow data sync-header <project> --file data/items.csv --type Item
coflow data write-file <project> --file data/items.cfd --stdin --check
coflow data patch <project> --patch patch.json
coflow check <project>
```

## 何时读取 reference

- 需要 patch JSON 示例、schema 变更流程、错误处理细节时，读取 `references/workflows.md`。
- 需要修改 `coflow.yaml`、处理复杂数据写入、Excel/Lark source、多语言维度或完整编辑决策时，读取 `references/editing-playbook.md`。
