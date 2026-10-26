# 结构变更与关联数据更新

用于新增必填字段、改类型或名称、改变继承、调整维度和修复损坏源码。目标是 CFT 与 CFD 一起满足新约束。

## 读取影响范围

```sh
coflow project files --project <项目> --json
coflow context --project <项目> --type Item --json
coflow schema refs --project <项目> --symbol Item --json
coflow data list --project <项目> --type Item --include-derived --json
coflow source read --project <项目> --file schema/main.cft --json
```

字段改动可查 `schema refs --symbol Item.price`。读取相关规则、内联对象、基础数据、覆盖和模板；list 要检查 data_scope.complete 并按 cursor 翻页。保存 revision 和需要的 source read 指纹。

| 变化 | 必须核对的影响 |
| --- | --- |
| 新增必填字段 | 所有具体记录、嵌套实例；提供真实值或有依据的默认值 |
| 修改默认值 | 省略字段的有效值、规则、模板、覆盖回退 |
| 改名字段或类型 | CFT 使用点、CFD 实际类型/字段/引用、覆盖目标、模板、规则 |
| 改字段类型 | 基础值与覆盖值、Option 层级、数值范围、集合键和多态类型 |
| 改继承 | ID 唯一域、继承字段冲突、引用可赋值性、规则名称 |
| 删除声明 | 剩余类型、数据、规则与应用调用的依赖 |

## 构造完整候选

候选文件放在项目输入目录之外，例如工作目录下的 proposal/candidates，避免被 schema/data 目录自动发现。每个 write/create 包含完整文件内容，不能只给变更片段。

```json
{
  "protocol_version": 1,
  "kind": "files",
  "changes": [
    { "op": "write", "file": "schema/main.cft", "content_file": "candidates/main.cft" },
    { "op": "write", "file": "data/items.cfd", "content_file": "candidates/items.cfd" }
  ]
}
```

file 相对项目根；content_file 相对请求 JSON 所在目录，input 为 stdin 时相对调用目录。write/delete 可带 expected_fingerprint；content 与 content_file 恰好提供一个。

新增文件使用 create，删除文件使用 delete；输入选择调整使用 `{"op":"add_input","kind":"schema","path":"schema/extra.cft"}` 或 remove_input。目录输入下新建同扩展名文件会自动被发现；remove_input 仅移除配置项，不删除文件。文件模式不与 operations 混在同一请求。

## 可运行变更示例

[schema-changes.json](../examples/schema-changes.json) 以 [minimal-project](../examples/minimal-project/coflow.yaml) 为起点，为 Item 新增必填 weight:int，配套 [候选 CFT](../examples/candidates/main.cft) 和 [候选 CFD](../examples/candidates/items.cfd) 同时补齐 sword=3、shield=5，保留覆盖、引用和原有规则。

将整个 examples 目录复制到临时工作目录，项目指向其中 minimal-project，请求指向外层 schema-changes.json；候选位于项目输入范围之外。新字段无默认值，因此仅更新 CFT 必须校验失败，联合请求才通过。

## 校验与发布

```sh
coflow apply --project <项目> --input proposal/changes.json --rules --if-revision <revision> --dry-run --json
coflow apply --project <项目> --input proposal/changes.json --rules --if-revision <revision> --json
```

按需求选择预览或直接发布。预览与正式写入的校验相同，预览成功尚未落盘；也可用 check --input --rules 仅校验。成功已包含所选检查，不重复相同全量检查；需要应用产物时再 build，并编译相关应用。

## 损坏源码与异常

模型查询不可用时，用 project files 和 source read 读取可用源文件，按诊断位置构造文件候选。配置本身无法解析时直接读取本地 coflow.yaml，修正后重新发现输入；任意 YAML 配置变更可直接编辑，再运行 check --rules。

候选校验失败时不发布，依据 code/help/record/field_path 修复；请求结构错误看 request_path/operation_index。conflict 后重新核对源文件、数据及范围，不能只更换 revision。I/O 失败检查 written_files、restored_files、restoration_failed，确认实际状态后再行动。

完成要求为：新结构和全部受影响数据有效，所选业务检查 complete 且通过，正式写入成功；若任务包含应用接入，生成与应用编译也需完成。
