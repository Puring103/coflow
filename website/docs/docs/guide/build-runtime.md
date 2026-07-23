# 构建与运行时

## 命令阶段

- `coflow check` 编译 schema、解析 source、构建 data model 并执行全部检查，不生成产物。
- `coflow build` 在检查通过后生成配置中声明的数据与代码产物。
- `coflow export` 只生成配置中的数据产物。
- `coflow codegen` 只生成配置中的代码和 loader。

## 产物发布

Coflow 只在全部产物生成并验证成功后替换输出目录。任一步骤失败时，现有产物保持不变。输出目录由 Coflow 管理，不要在其中放置手写文件；`coflow clean` 用于清理构建产生的临时数据。

## C# runtime

C# codegen 根据 schema 生成强类型只读 API，并根据 `outputs.data.type` 生成 JSON 或 MessagePack loader。数据合法性应在 `check` 或 `build` 阶段确认。

详细流程见 [项目流水线](../reference/02-project-pipeline.md) 和 [C# 代码生成](../reference/07-codegen/01-csharp.md)。
