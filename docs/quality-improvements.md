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

### 修复数据导出旧表文件残留

`coflow-pipeline` 在写出 JSON 或 MessagePack 数据前会根据 `coflow.data.manifest.json`
清理上一轮生成、但本轮不再生成的 `.json` / `.msgpack` 表文件。这样删除或
重命名 schema 表后，旧数据文件不会继续留在输出目录并被消费者误用。

新增回归测试覆盖：

- stale `.json` 文件会被删除。
- stale `.msgpack` 文件会被删除。
- 非 Coflow 数据表扩展的旁路文件会保留。

### 增加产物 manifest，避免误删或覆盖非 Coflow 文件

数据导出和 C# codegen 现在分别维护 `coflow.data.manifest.json` 与
`coflow.csharp.manifest.json`。写入前只清理上一轮 manifest 中存在、但本轮
不再生成的产物；如果输出目录中存在未被 manifest 管理的 `.json`、`.msgpack`
或 `.cs` 文件，命令会拒绝写入，而不是按扩展名直接删除或覆盖。

新增回归测试覆盖：

- 有 manifest 的旧数据表文件会被清理，并刷新 manifest。
- 未被 manifest 管理的 `.json` 文件会阻止导出，原文件保留。
- 未被 manifest 管理的 `.cs` 文件会阻止 C# codegen，原文件保留。
