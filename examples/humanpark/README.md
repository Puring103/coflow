# HumanPark Luban → Coflow 迁移（测试沙盒）

把 `HumanPark` Unity 项目里基于 Luban 的配置流程
（`HumanPark/Assets/Plugins/Luban/`）端到端迁移到 Coflow 的试验目录。

原 `HumanPark` 项目**未做任何修改**：本目录是一份自包含的副本，包含数据、
schema 和代码生成目标，全部通过 Coflow CLI 跑通。

## 目录结构

- `schema/enums.cft` / `beans.cft` / `tables.cft` — 由 Luban 的
  `__beans__.xml`、`__tables__.xml`、`enums/*.xml`、`enums_gen/*.xml`
  迁移过来的 CFT schema，按"枚举 / 共享结构 / 数据表"拆分到三个文件。
- `data/Configs.coflow.xlsx` — 由 `tools/convert_luban_xlsx.py` 从原
  `Configs.xlsx` 转换得到的 coflow 兼容版本。
- `tools/convert_luban_xlsx.py` — 一次性转换脚本（幂等，直接读取原 Luban
  xlsx）。
- `coflow.yaml` — 项目配置；指向 schema 目录、转换后的 xlsx，以及
  `generated/` 下的 JSON 和 C# 输出。

## 运行方式

```powershell
# Luban 端有改动时重新生成转换后的 xlsx
python tools/convert_luban_xlsx.py

# 跑 coflow 流水线
cargo run --quiet -- check examples/humanpark
cargo run --quiet -- build examples/humanpark
```

`build` 会写出：

- `generated/data/<Type>.json`
- 命名空间为 `Core.Data.Config` 的 `generated/csharp/<Type>.cs`
- 对带 `@KeyAsEnumValue` 注解的 type，再生成 `<Type>Id.cs`，把所有 id
  作为 `public const string` 暴露给业务代码。

## 用到的 coflow 特性

- **枚举类型的 `@id`**：保留给字段类型同时也被引用的枚举做主键，
  例如 `TerrainConfig.id: TerrainType`、`TileTagConfig.id: TileTagFlags`、
  `GameFeatureConfig.id: GameFeature`、`AttributeConfig.id: CreatureAttribute`。
  cell 里写枚举名（`Water`、`LifeCycle_LifeSpan`），导出 JSON 时落为整数。
- **`@expand` 行内展开**：`TerrainConfig` 的 `EnvironmentConfig`、
  `InitialConfig` 字段加 `@expand` 后，loader 把父字段及其后续连续若干列
  按内层 type 字段顺序读取并组装成嵌套对象——不需要在 xlsx 单元格里写
  `{a, b, c}`。
- **`@ref` 跨表引用**：`GeneConfig.parentGeneId`、`BioRemainsConfig.geneId`
  指向 `GeneConfig`；`@ref` 字段允许用 `string?` 表达"可选引用"。
- **`@KeyAsEnumValue` codegen**：用于"id 是策划起的字符串、没有合理整数枚举可绑"的表
  （`GeneConfig`、`SkinConfig`、`PhaseConfig`），以及"原本 luban 端是枚举但仅在 @id
  上使用、没有别的地方当字段类型引用"的表（`AbilityConfig`、`SubstanceConfig`）。
  这两种情况下 schema 直接用 `id: string`，build 时按表内实际 id 生成对应的常量类
  （`<Type>Id.cs`）。这样可以彻底去掉那两个 enum 定义。
- **宽松 bool 解析**：`is_base`、`isInit`、`isInitial` 在 xlsx 里仍是 `0`/`1`，
  cell parser 会接受。
- **schema 多文件**：`schema:` 目录指向 `schema/`，coflow 自动加载里面的所
  有 `.cft`，所有顶层定义共享同一全局命名空间。

## 转换脚本目前还在做的事

经过 coflow 端的功能扩展之后，转换脚本只剩两类必须的"格式适配"：

- **删除多余表头**。Luban 的 xlsx 有三行表头（`##var`、`##type`、`##` 中文
  描述）+ 一个控制列；coflow 期望单行表头。脚本剥掉这些。
- **删除占位行**。控制列以 `##` 开头的行会被丢；只填了 id、其它列全空的
  行（如 地块tag 表里的 `Drought`/`Toxic`/`Humid`）也会被丢，与 Luban 实
  际不导出这些行的行为一致。
- **数组分隔符** `,` → `|`，匹配 coflow 的单元格语法。

枚举名 → int、bool 转换、嵌套对象打包等之前的 workaround **都已不再需要**——
coflow 端直接支持。

> 注：`AbilityConfig` / `SubstanceConfig` 由于 schema 改用 `id: string` +
> `@KeyAsEnumValue`，导出 JSON 中这两张表的 `id` 字段从整数变成了字符串
> （例如 `"Ability_Eat"`、`"Matter_Nutrition"`）。如果下游运行时硬绑了这些
> 整数 id，需要要么改回 `enum @id` 形式，要么把读取侧迁移到字符串 id。

## 未覆盖的部分

- Luban 项目还有一份 UI 侧的实例：
  `HumanPark/ui/Assets/Plugins/Luban`。本沙盒只迁移了主端的数据集。
- Luban 端的校验脚本（`Tools/LubanFeishuValidator/validator.mjs`）未做迁移。
  对应规则可以用相关 type 的 `check { ... }` 块表达，留作后续工作。
- Luban xlsx 中以中文/英文混合的可选字段（`name` 这种 localizedString）目
  前在 cell 里只填了 key；如果以后要支持双语 fallback，可在转换脚本里把
  cell 写成 `key, "fallback text"` 形式。
