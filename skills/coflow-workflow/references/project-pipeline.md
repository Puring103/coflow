# 项目流水线

`coflow.yaml` 指定 CFT schema、CFD 数据和代码生成目标。项目加载后，可以分别检查数据或生成目标语言源文件。

## 命令

- `coflow cft check` 只检查 schema。
- `coflow check` 加载全部 CFD、解析引用并执行 `check {}`，不写产物。
- `coflow codegen` 加载 schema 和 CFD 数据，但不执行 `check {}`；输入与生成均无诊断时，原子发布所有配置的目标语言源文件。

交付前先运行 `coflow check`，通过后再运行 `coflow codegen`。生成失败不会替换已有代码目录。
