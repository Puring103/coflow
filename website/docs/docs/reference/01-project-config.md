# 项目配置

`coflow.yaml` 只描述 CFT schema、CFD 输入、维度和代码生成目标：

```yaml
schema: schema/
data:
  - data/
  - overlays/base.cfd
dimensions:
  language:
    variants: [en, zh]
    out_dir: data/dimensions/language
codegen:
  - language: csharp
    dir: generated/csharp
    namespace: Game.Config
```

## 字段

`schema` 是 `.cft` 文件或目录。目录递归发现 `.cft`，路径按项目根目录解析。

`data` 是 `.cfd` 文件或目录列表。目录只递归发现 `.cfd` 并忽略其他扩展名；显式配置非 `.cfd` 文件、对象形状或路径穿越会产生诊断。

`dimensions` 描述变体名称和生成目录。维度文件仍然是 CFD；它们在同一数据模型中参与检查和代码生成。

`codegen` 是唯一产物列表。每项必须有 `language` 和 `dir`，其余键作为目标语言 options 传给 generator。目标目录必须互不重叠并位于项目根目录内。

配置解析拒绝未知字段，输入和产物合同不会隐式转换。
