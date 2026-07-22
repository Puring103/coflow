# 构建与运行时

## 命令阶段

- `coflow check` 编译 schema、解析 source、构建 data model 并执行全部检查，不生成产物。
- `coflow build` 在检查通过后生成配置中声明的数据与代码产物。
- `coflow export` 只生成配置中的数据产物。
- `coflow codegen` 只生成配置中的代码和 loader。

## 产物发布

Coflow 不会边生成边覆盖正在使用的输出。它先在 staging 和不可变 generation 目录中生成完整产物，验证后再替换稳定输出目录，最后原子发布 `.coflow/artifacts/active.json`。

如果生成、验证或发布失败，上一个 active generation 仍然完整可用。`coflow clean` 会清理历史 generation 和中断的 staging，保留当前活动版本。

## C# runtime

C# codegen 根据编译后的 schema 生成强类型只读 API，并根据 `outputs.data.type` 生成 JSON 或 MessagePack loader。运行时不再解析 CFT，也不重复执行构建期检查。

详细流程见 [项目流水线](../reference/02-project-pipeline.md) 和 [C# 代码生成](../reference/07-codegen/01-csharp.md)。
