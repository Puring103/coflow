---
name: coflow-cli-development
description: "Coflow CLI、engine、provider writer 和自动化命令开发工作流：当用户需要在 Coflow 仓库内新增或修改 CLI 命令、实现 schema/data 自动化能力、调整 engine API、添加集成测试、修复 clippy/test 失败或准备提交时使用。"
---

# Coflow CLI Development

使用本 skill 在 Coflow 仓库内开发工具本身。保持改动小而清晰，优先沿用现有 CLI、engine、provider registry 和测试模式。

## 开发流程

1. 先读相关规格和现有实现：`docs/spec/09-cli.md`、`src/main.rs`、`src/data_commands.rs`、相关 `crates/coflow-engine/src/*.rs`。
2. 新增 CLI 行为先补集成测试，通常放在 `tests/cli_*.rs`。
3. 命令参数在 `src/main.rs` 定义，命令编排放在 `src/*_commands.rs`，共享逻辑优先放在 `coflow-engine`。
4. schema-only 命令不要要求数据源存在；需要完整数据模型的命令再调用完整 session。
5. 数据写入必须走 provider writer 或已有 engine 写入接口，不要在 CLI 层直接改表格/CFD 数据。
6. 输出默认 JSON，`--human` 仅作为可读输出。
7. 用户可恢复错误应输出结构化 diagnostics 并返回非零，不要 panic。
8. 完成后运行仓库要求的检查。

## 仓库检查

在推送任何分支前，从仓库根目录运行：

```powershell
cargo check --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

如果只做局部迭代，可先运行目标测试，例如：

```powershell
cargo test --test cli_ai_data -- --nocapture
```

## 实现边界

- `coflow-api` 定义 provider trait、诊断和写入契约。
- `coflow-project` 负责项目配置、路径解析和 schema 文件发现。
- `coflow-engine` 负责 schema 编译、source resolve/load、data model、check、索引和共享读写 API。
- 根 crate `coflow` 负责编排 CLI、JSON/human 输出、导出和 codegen 提交。
- provider 共享算法放在 `coflow-loader-table-core` 或对应 provider crate，不放进 CLI。

## 何时读取 reference

- 新增 CLI 命令、schema/data 自动化命令或 provider 写入行为时，读取 `references/development.md`。
