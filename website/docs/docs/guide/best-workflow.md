# 最佳工作流

1. 先修改 CFT schema，再修改对应 CFD 记录。
2. 运行 `coflow cft check` 和 `coflow check`。
3. 检查通过后运行 `coflow codegen`，提交 schema、CFD 和生成源文件变更。
4. 项目 CI 先运行 `coflow check`，通过后再运行 `coflow codegen`。
