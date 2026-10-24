# 协作与 CI

Schema、CFD 与生成代码分别存放；生成目录由 `codegen` 或 `build` 管理。应用项目的 CI 使用 `coflow build --project <项目>` 检查并生成，或 `check --rules` 只检查数据，`codegen --check` 只核对产物一致性。

提交多个依赖修改时用一份 `apply` 变更集。操作模式按数组顺序处理候选值，并在全部操作后检查引用；Schema 与 CFD 联合更新使用文件模式。`--rules` 在候选项目中验证业务规则，成功后无需重复相同检查。预览适合需要审阅补丁的任务，不是固定前置步骤。

读取后保存 `revision`，写入携带 `--if-revision`，或在变更集顶层填写 `if_revision`。冲突后重读受影响目标，根据当前状态重新形成修改；新修订不能直接替换旧修订后盲目重试。分页游标与读取修订绑定，数据变化后从第一页重新查询。

使用字段选择、分页、定向 `context` 和默认简明响应控制上下文；完整 Schema、成功任务明细、规则解释分别由 `describe --schema`、`--details`、`--explain` 按需取得。不要把输出被裁剪解释为没有剩余问题，先查看 `diagnostics_summary` 和适用的 `complete`；数据是否完整加载另看 `data_scope.complete`。
