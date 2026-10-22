# 最佳工作流

1. 先修改 CFT schema，再修改对应 CFD 记录。
2. 使用编辑器、LSP 或 `coflow schema check` 及时检查声明问题。
3. 运行 `coflow check --rules`，验证数据、引用、维度和业务约束。
4. 需要生成代码时运行 `coflow build`，在全部检查通过后发布产物。
5. 将 CFT、CFD、项目配置与稳定枚举映射纳入版本管理；生成代码是否提交由应用项目的构建流程决定。

应用项目的 CI 可以直接运行 `coflow build --project <项目目录>`。只验证数据时使用 `coflow check --rules`，只核对生成文件是否最新时使用 `coflow codegen --check`。AI 批量修改推荐 `apply --rules --if-revision <revision> --json`，需要审阅时加 `--dry-run`；成功结果已包含所选校验。
