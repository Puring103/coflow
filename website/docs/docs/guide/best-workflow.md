# 最佳工作流

## 1. 先固定数据合同

先在 CFT 中定义类型、默认值、引用、注解和 `check {}`，再开始批量录入数据。Schema 是编辑器、CLI、产物和运行时代码的共同合同。

## 2. 按数据形态选择 Source

- Excel 适合批量数值、策划日常编辑和多 sheet 数据。
- CSV 适合单表、自动生成或强调 Git diff 的数据。
- CFD 适合嵌套对象、多态、集合和 spread 等结构化内容。

不同 source 可以在同一项目中交叉引用，无需为每种格式建立独立运行时模型。

## 3. 用结构化命令修改

工具和 AI agent 应优先使用 `schema write-file`、`data write-file`、`data patch` 和 `data sync-header`。这些命令能够在写入前规划变更，并统一返回诊断。

## 4. 提交前只做校验

```powershell
coflow check .
```

`check` 不写产物，适合本地 pre-commit 和 pull request CI。诊断应作为结构化错误处理，不要只解析终端文本。

## 5. 交付时统一构建

```powershell
coflow build .
```

`build` 生成并验证所有产物，全部成功后才替换输出目录。详细安全边界见 [项目流水线](../reference/02-project-pipeline.md)。
