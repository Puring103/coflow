# HumanPark Luban → Coflow 迁移（测试沙盒）

把 `HumanPark` Unity 项目里基于 Luban 的配置流程
（`HumanPark/Assets/Plugins/Luban/`）端到端迁移到 Coflow 的试验目录。

原 `HumanPark` 项目**未做任何修改**：本目录是一份自包含的副本，包含数据、
schema 和代码生成目标，全部通过 Coflow CLI 跑通。

## 目录结构

- `schema/enums.cft` / `beans.cft` / `tables.cft` — 由 Luban 的
  `__beans__.xml`、`__tables__.xml`、`enums/*.xml`、`enums_gen/*.xml`
  迁移过来的 CFT schema，按"枚举 / 共享结构 / 数据表"拆分到三个文件。
- `data/Configs.xlsx` — 单行表头的 coflow 兼容配置表；可选的 `#` 控制列中
  写 `##` 的数据行不会被导入。
- `coflow.yaml` — 项目配置；指向 schema 目录、配置表，以及
  `generated/` 下的 JSON 和 C# 输出。

## 运行方式

```powershell
# 跑 coflow 流水线
cargo run --quiet -- check examples/humanpark
cargo run --quiet -- build examples/humanpark
```

`build` 会写出：

- `generated/data/<Type>.json`
- 命名空间为 `Core.Data.Config` 的 `generated/csharp/<Type>.cs`
- 对带 `@keyAsEnum("...")` 注解的类型，按表内实际记录 key 生成真正的
  C# enum，并让生成类型的 `Id` 属性使用该 enum。

## 用到的 coflow 特性

- **Excel `id` 特殊列**：每个 sheet 的 `id`、`Id` 或 `ID` 列是记录 key，不需要也不能在
  CFT 中声明 `id`、`Id` 或 `ID` 字段。check 中的 `id` 是虚拟只读 key。
- **类型级 `@keyAsEnum` codegen**：用于“key 是策划起的字符串、希望 C# 强类型化”的表
  （`GeneConfig`、`SkinConfig`、`PhaseConfig`、`AbilityConfig`、`SubstanceConfig` 等）。
  build 时按表内实际 key 生成对应的 C# enum（例如 `GeneId.cs`），并让引用字段如
  `BioRemainsConfig.Gene` 使用解析后的强类型对象。
- **`@expand` 行内展开**：`TerrainConfig` 的 `EnvironmentConfig`、
  `InitialConfig` 字段加 `@expand` 后，loader 把父字段及其后续连续若干列
  按内层 type 字段顺序读取并组装成嵌套对象——不需要在 xlsx 单元格里写
  `{a, b, c}`。
- **显式 typed ref 跨表引用**：`GeneConfig.parentGene`、`BioRemainsConfig.gene`
  指向 `GeneConfig`；Excel 中引用单元格可以写成 `@GeneConfig.Gene_Spore`，同类型直接引用也可以写成 `&Gene_Spore`，可空引用留空即可。
- **宽松 bool 解析**：`is_base`、`isInit`、`isInitial` 在 xlsx 里仍是 `0`/`1`，
  cell parser 会接受。
- **schema 多文件**：`schema:` 目录指向 `schema/`，coflow 自动加载里面的所
  有 `.cft`，所有顶层定义共享同一全局命名空间。

## Excel 表格式约定

- 每个 sheet 的第一行是字段名；第二行开始是数据行。
- `id`、`Id` 或 `ID` 列是特殊记录 key 列，不映射到 CFT 字段。
- 表头为 `#` 的列是可选导入控制列，不映射到 schema 字段；该列值为 `##`
  的数据行会被 loader 跳过。
- 空行会被跳过；只填了 id、其它列全空的占位行仍需要用 `# = ##` 显式跳过。
- 数组使用 coflow 单元格语法里的 `|` 分隔。
- 对象引用使用 `@Type.key`，例如 `@GeneConfig.Gene_Spore`；当前字段类型就是引用根类型时也可以写 `&key`。路径引用必须写完整 `@Type.key.path`。数组引用写成
  `@GeneConfig.Gene_A|@GeneConfig.Gene_B` 或 `&Gene_A|&Gene_B`。

枚举名 → int、bool 转换、嵌套对象打包等之前的 workaround 都已不再需要：
coflow 端直接支持。

> 注：导出 JSON 中所有表都会写出保留字段 `"id"`，其值来自 Excel 的记录 key。
> 对带 `@keyAsEnum` 的类型，C# 端会把 `Id` 提升为生成 enum，但 JSON 仍保持
> 原始 key 字符串。

## 未覆盖的部分

- Luban 项目还有一份 UI 侧的实例：
  `HumanPark/ui/Assets/Plugins/Luban`。本沙盒只迁移了主端的数据集。
- Luban 端的校验脚本（`Tools/LubanFeishuValidator/validator.mjs`）未做迁移。
  对应规则可以用相关 type 的 `check { ... }` 块表达，留作后续工作。
- Luban xlsx 中以中文/英文混合的可选字段（`name` 这种 localizedString）目
  前在 cell 里只填了 key；如果以后要支持双语 fallback，可在表格里把
  cell 写成 `key, "fallback text"` 形式。
