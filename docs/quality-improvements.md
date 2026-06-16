# 质量提升记录

本文档记录已经完成的主要质量提升修改，便于后续审查和持续推进。

## 2026-06-17

### 建立项目质量要求文档

新增 `docs/project-quality-requirements.md`，统一记录项目在代码质量、架构、文档、网页、测试、错误码覆盖、工程门禁和潜在 bug 排查方面的要求。

### 对齐 CI 与本地门禁

CI 增加 `cargo check --workspace`，与本地提交前要求保持一致。

### 修复 VS Code 插件 README 漂移

更新 `editors/vscode-coflow/README.md`：

- 名称从 CFT-only 调整为 Coflow language support。
- 本地打开目录修正为 `editors/vscode-coflow`。
- 语言服务命令修正为 `coflow lsp`。
- 配置项前缀修正为 `coflow.diagnostics.*`。
- 描述补充 `.cfd` 支持。

### 统一测试依赖版本

将根 crate 和 `coflow-pipeline` 的 `rust_xlsxwriter` dev-dependency 从 `0.64` 升级到 `0.95`，与 `coflow-loader-excel` 测试依赖保持一致，减少重复依赖树。

### 清理本地工具配置

删除已跟踪的 `.claude/settings.local.json`，并在 `.gitignore` 中忽略 `.claude/`。

### 明确生成目录 lockfile 例外

`.gitignore` 中显式保留 `examples/*/generated/csharp/coflow.enum.lock.json`，避免普通生成物忽略策略与被跟踪 lockfile 策略冲突。

