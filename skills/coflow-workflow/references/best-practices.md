# Coflow 工作流与最佳实践

## 标准工程闭环

1. 用 CFT 固定数据结构、默认值、引用关系和 `check {}` 业务规则。
2. 根据数据形态选择 source：大量同构记录用 Excel/CSV，复杂嵌套、模板覆盖和少量结构化配置用 CFD。
3. 用 `coflow check <project>` 作为提交前和 CI 的基础 gate。
4. 用 `coflow build <project>` 在构建阶段统一导出 JSON/MessagePack 和可选 C# 代码。
5. 程序只消费生成产物；手写配置只放在 schema/source 中，不放进 `outputs.*.dir`。

## AI agent 工作流

1. 先读取项目结构，而不是直接猜字段：`coflow schema inspect`、`coflow schema files`、`coflow data sources`。
2. 修改 schema 时优先用 `coflow schema write-file --stdin --check`。
3. 修改数据时优先用 `coflow data patch`，复杂 CFD 整理才用 `coflow data write-file --stdin --check`。
4. 字段新增、删除、重命名后，对本地 CSV/XLSX/CFD 文件运行 `coflow data sync-header`。
5. 每轮写入后查看结构化报告中的 `write_ok`、`check_ok`、`applied`、`failed`、`affected_files` 和 `diagnostics`。
6. `data patch` 整批规划、预检并原子写入；writer、重建或提交失败会补偿已写来源，`applied` 为空且旧 generation 保持可用。

## 团队实践

- Schema 是协作契约。新增字段前先明确类型、默认值、nullable、引用形态和业务校验。
- 用 enum 表达固定分类，用 `@idAsEnum` 表达需要进入代码的 record key 集合。
- 用默认值减少表格重复填写，但不要用默认值隐藏必须由策划确认的关键配置。
- 用 `check {}` 表达上线前必须满足的业务规则，例如数值范围、数组唯一性、引用集合约束和多态类型约束。
- 保持 `coflow.enum.lock.json` 随 `@idAsEnum` 产物一起提交，避免生成 enum 值漂移。
- 不把生成目录加入 `sources`，也不在生成目录里放手写文件。
- 维度/本地化文件由引擎维护；配置 `dimensions.language.out_dir` 后不要再手动加入 `sources`。

## Source 选择

| 场景 | 推荐 source |
| --- | --- |
| 大量同构数值和文本 | Excel 或 CSV |
| 复杂嵌套对象、多态数组、模板覆盖 | CFD |
| 简单自动生成或程序维护数据 | CSV 或 CFD |

## 检查策略

- 日常提交：至少运行 `coflow check <project>`。
- 产物更新：运行 `coflow build <project>`，让 check、export、codegen 在同一流程内完成。
- CI：对每个 Coflow 项目运行 `coflow check` 或 `coflow build`，不要只检查单个 source。
- 诊断修复：优先修复最靠近 source 的错误，例如 schema 编译错误、source 配置错误、字段解析错误，再处理业务 `check` 错误。
