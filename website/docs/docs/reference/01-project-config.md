# 项目配置

`coflow.yaml` 描述 CFT schema、CFD 输入、维度目录和代码生成目标：

```yaml
schema: schema/
data:
  - data/
  - overlays/base.cfd
dimensions:
  language:
    display_name: 本地化
    dir: dimensions/language/
codegen:
  - language: csharp
    dir: generated/csharp
```

## 字段

`schema` 是 `.cft` 文件或目录，也可以使用列表配置多个路径。目录递归发现 `.cft`，相对路径按项目根目录解析，也可以声明项目目录外的路径。

`data` 是 `.cfd` 文件或目录列表。目录只递归发现 `.cfd` 并忽略其他扩展名；显式配置非 `.cfd` 文件或对象形状会产生诊断。项目会加载并追踪所有声明的路径，包括通过 `../` 或绝对路径声明的项目目录外数据。

`dimensions` 按维度名配置显示名称 `display_name` 和覆盖目录 `dir`。目录路径相对项目根目录解析，也可以使用绝对路径；递归加载 `.cfd`，无需在 `data` 重复声明，同一来源只加载一次。目录尚不存在时视为空目录，编辑器首次保存覆盖时创建。不同维度目录互不重叠，并与 schema 输入分离。仅配置显示名称的维度仍可读取已有分散覆盖；新增推荐覆盖文件前需配置 `dir`。

编辑器在维度目录内保留基础文件相对项目根目录的完整路径：`data/equipment/items.cfd` 对应 `dimensions/language/data/equipment/items.cfd`。同一文件、同一维度的记录、字段和变体集中保存，已有覆盖始终写回原来源。

`codegen` 是唯一产物列表。每项必须有 `language` 和 `dir`。C# 目标支持可选的 `namespace`，例如 `namespace: Game.Config`；未配置或为空字符串时生成类型位于全局命名空间。命名空间由点分隔的合法 C# 标识符组成。目标目录可以位于项目目录内或目录外，使用相对项目根目录的路径配置，例如 `../Scripts/Config`。目标目录必须互不重叠，并与 schema、data 和维度输入目录分离。

schema、data、dimensions 和 codegen 的路径统一使用相对项目根目录的路径保存，项目外目录使用 `../`。

配置解析拒绝未知字段，输入和产物合同不会隐式转换。
