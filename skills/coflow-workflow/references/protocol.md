# CLI 响应、请求值和事务约定

具体命令参数由 `coflow describe <点分命令> --json` 查询；JSON Schema 按需用 `--schema input|output|both`，定义位于 `result.definition`。

## 读取响应

`--json` 输出单行 JSON，`--pretty` 启用缩进 JSON，`--details` 控制信息量。响应含 protocol_version、command、status、result，按需提供 project、revision、data_scope、diagnostics、error_category。

| 退出码 | 含义 | 下一步 |
| --- | --- | --- |
| 0 | 完整成功 | 根据 result 区分预览、无变化或已写入 |
| 1 | 校验失败、状态不满足或结果不完整 | 阅读诊断和完整性字段 |
| 2 | 请求错误 | 修正参数、request_path 或操作结构 |
| 3 | 修订、旧值或其他前置条件冲突 | 重读目标与范围，重新形成请求 |
| 4 | I/O 或执行失败 | 核对实际写入和恢复状态 |

不能用 diagnostics 为空代替成功判断。check 的 complete 表示检查执行是否完整，与规则是否通过分别判断。

列表的 truncated 表示输出省略；data_scope.complete 表示输入加载完整性；diagnostics_summary.truncated 表示诊断输出截断。这三者用途不同。默认最多 50 条诊断，details 解除默认上限，显式 limit 仍生效。可选元数据与空附加列表可能省略，业务值中的空态保留。

## 值与路径

优先用 CFD 表达式输入；需要精确保留查询值时用 model 包装：

```json
{ "kind": "expression", "source": "Some(\"备注\")" }
```

```json
{ "kind": "model", "value": { "kind": "int", "value": "9223372036854775807" } }
```

model 的 int.value 为十进制字符串；float 为有限数值。Option 使用 none / some，some 内再放 model 值。ref 带 type、id；object 带实际 type、fields；array 带 items；dict 带 entries，每项含 key、value。模板使用 fstring.source，保留源表达式。

枚举带 type、value 和 variant；variant 仅表示一个命名成员，组合标志没有单一成员名。优先保留查询返回的类型化值，rendered、origin、location 只读，不能直接包装为可写 model。

CLI 路径片段是 `{"kind":"field","name":"price"}`、`{"kind":"index","index":0}`、`{"kind":"dict_key","key":{"kind":"string","value":"title"}}`。文本命令路径可用 `stats.hp`、`items[0]`、`labels["title"]`。

## 修订与发布

读取后保存顶层 revision，写入或候选预览带 `--if-revision`。apply 顶层也可写 if_revision，同时提供时两者必须相同。分页 cursor 绑定查询条件和修订，项目变化后去掉 cursor 重查。

set 的 expected 按字段类型与默认值比较当前基础有效值，模板按 f 源表达式比较。write/delete 的 expected_fingerprint 校验单个文件，指纹从 source read 取得，不能自行猜测。

请求失败可含 result.request_path、从零开始的 operation_index、target。发布失败检查 written、written_files、restored_files、restoration_failed；恢复未完成时先检查实际文件状态，避免把失败当成完全无副作用。预览成功只表示候选通过，尚未落盘。

## 路径与编码

| 路径 | 解析基准 |
| --- | --- |
| coflow.yaml 中的路径 | 项目根目录 |
| source 命令或变更集的 file | 项目根目录 |
| `--input`、`--value-file` | 调用目录 |
| 变更集中的 content_file | 请求 JSON 所在目录；input 为 stdin 时使用调用目录 |

文件使用 UTF-8。`--input -` 或 `--value-file -` 读取 stdin，一次调用只能有一个 stdin 载体。source write/create 的 input 是完整源码；apply、候选 check/diff 的 input 是变更集 JSON；data create 的 input 是字段对象。

源码诊断的行、列从零开始，列按 UTF-16 偏移，范围含起点不含终点。Shell 转义与 CFD 转义分两层，复杂输入优先使用文件。
