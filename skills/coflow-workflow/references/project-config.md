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
```

## 字段

`schema` 是 `.cft` 文件或目录。目录递归发现 `.cft`，相对路径按项目根目录解析，也可以声明项目目录外的路径。

`data` 是 `.cfd` 文件或目录列表。目录只递归发现 `.cfd` 并忽略其他扩展名；显式配置非 `.cfd` 文件或对象形状会产生诊断。项目会加载并追踪所有声明的路径，包括通过 `../` 或绝对路径声明的项目目录外数据。

`dimensions` 描述变体名称和生成目录。维度文件仍然是 CFD；它们在同一数据模型中参与检查和代码生成。

`codegen` 是唯一产物列表。每项必须有 `language` 和 `dir`。C# 目标不接受额外选项，生成类型位于全局命名空间。目标目录必须互不重叠并位于项目根目录内。

配置解析拒绝未知字段，输入和产物合同不会隐式转换。
