# 项目流水线

`coflow.yaml` 指定 CFT、CFD 输入和代码生成目标。

- `coflow schema check` 检查 Schema。
- `coflow check` 检查 Schema、CFD、默认值、引用和维度。
- `coflow check --rules` 额外执行全部 CFT 业务规则。
- `coflow codegen` 在结构与数据有效后发布代码，业务规则独立检查。
- `coflow build` 在结构、数据和全部业务规则通过后发布代码。

查询与预览保持只读。写入先校验候选项目，发布前检查外部修改。代码生成在所有目标准备完成后统一发布，失败保留既有产物。
