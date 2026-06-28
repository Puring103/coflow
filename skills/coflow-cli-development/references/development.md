# Coflow CLI 开发参考

## 新增命令步骤

1. 增加集成测试，先验证失败或缺失行为。
2. 在根 CLI 入口增加 clap 参数和 subcommand。
3. 在对应 commands 模块增加 CLI 编排和输出。
4. 共享逻辑放在 engine 层，必要时从 engine 公共入口导出。
5. 更新面向用户的 CLI 文档和常用命令说明。
6. 运行目标测试、fmt、clippy、workspace 检查。

先用文件搜索定位现有 CLI 入口、commands 模块、engine 层和集成测试，按当前项目结构继续。

## Schema-only 命令

以下命令不应要求数据源文件存在：

- `cft check`
- `lsp`
- `schema inspect`
- `schema files`
- `schema write-file`
- `data create-file`
- 只依赖 schema 的 header/layout 计算

需要完整数据模型的命令才加载完整 session：

- `data sources`
- `data list`
- `data get`
- `data patch`
- `data write-file --check`
- `check`
- `build`
- `export`

## 数据文件命令

`schema write-file`：

- 只允许写项目 schema 配置展开出的精确小写 `.cft` 文件。
- `--stdin` 提供完整替换内容；`--dry-run` 不落盘；`--check` 用写入内容编译 schema 并输出 diagnostics。
- 非 dry-run 且 `--check` 失败时，文件已经写入，CLI 返回非零并让调用方继续修复。

`data create-file`：

- `.csv` 和 `.xlsx` 按 schema 字段创建表头。
- `.cfd` 只创建空文件。
- 不覆盖已有文件。

`data sync-header`：

- CSV/XLSX 保留同名列，新增列为空，删除旧列。
- CFD 更新匹配类型记录的顶层字段，不创建表头。

`data patch`：

- 走 provider writer。
- 允许部分落盘，报告 `applied` 和 `failed`。
- 写完后重建项目，返回 check 诊断。

`data write-file`：

- 只允许写配置内本地 CFD source 覆盖的精确小写 `.cfd` 文件：未指定 `type` 的目录/`.cfd`，或显式 `type: cfd`。
- `--stdin` 提供完整替换内容；`--dry-run` 不落盘且不运行完整数据检查；`--check` 在非 dry-run 写入后重建项目并输出 diagnostics。
- CSV/XLSX 不走该命令，继续走 provider writer、create-file、sync-header。

## 测试建议

- CLI 参数和 JSON 输出用 CLI 集成测试。
- engine 行为用 engine 层测试。
- provider writer 行为用对应 provider/loader 测试。
- 数据文件命令至少覆盖 CSV 和 CFD 差异。
- 写入命令覆盖 insert、set_field、delete_record。
- 配置编辑相关功能覆盖 `coflow.yaml` 的 schema/source/output/dimensions 边界。
- schema 写入命令覆盖 stdin 写入、dry-run、拒绝非 schema 文件和 check diagnostics。
- CFD 文件写入命令覆盖 stdin 写入、dry-run、拒绝非 cfd、拒绝未配置 source 和 check diagnostics。
- 字段形态或 schema 注解变更要同时覆盖 CFT 编译、data model 校验、provider writer、CLI/JSON patch 和编辑器 wire/前端行为。

## 收尾检查

```powershell
cargo check --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
