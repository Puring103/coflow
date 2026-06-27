---
name: coflow-cli-development
description: "Coflow CLI、engine、provider writer 和自动化命令开发工作流：当用户需要在 Coflow 仓库内新增或修改 CLI 命令、实现 schema/data 自动化能力、调整 engine API、添加集成测试、修复 clippy/test 失败或准备提交时使用。"
---

# Coflow CLI Development

使用本 skill 在 Coflow 仓库内开发工具本身。保持改动小而清晰，优先沿用现有 CLI、engine、provider registry 和测试模式。

## 开发流程

1. 先用本 skill 的 reference 明确预期行为，再按当前工作区已有模块和测试的组织方式定位相邻实现。
2. 新增 CLI 行为先补集成测试；测试文件位置以当前仓库已有 CLI 集成测试为准。
3. 命令参数放在根 CLI 入口，命令编排放在对应 commands 模块，共享逻辑优先放在 engine 层。
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

- API 层定义 provider trait、诊断和写入契约。
- project 层负责项目配置、路径解析和 schema 文件发现。
- engine 层负责 schema 编译、source resolve/load、data model、check、索引和共享读写 API。
- 根 CLI 层负责编排命令参数、JSON/human 输出、导出和 codegen 提交。
- provider 共享算法放在 table-core 或对应 provider 模块，不放进 CLI。

## 何时读取 reference

- 新增 CLI 命令、schema/data 自动化命令或 provider 写入行为时，读取 `references/development.md`。
