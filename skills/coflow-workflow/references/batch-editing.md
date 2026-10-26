# 批量修改与集合操作

适用于多记录关联更新、数组或字典操作，以及按条件赋值。先核对目标和旧值，保存查询 revision；完整输入以 `coflow describe apply --schema input --json` 为准。

## 请求与执行

```json
{
  "protocol_version": 1,
  "kind": "operations",
  "ops": [{
    "op": "set_field",
    "record": { "type": "Item", "id": "sword" },
    "path": [{ "kind": "field", "name": "price" }],
    "value": { "kind": "expression", "source": "100" },
    "expected": { "kind": "expression", "source": "90" }
  }]
}
```

保存为 changes.json 后：

```sh
coflow apply --project <项目> --input changes.json --rules --if-revision <revision> --json
```

需要审阅补丁时加 `--dry-run`，成功预览后正式写入仍使用原读取修订。`check --input changes.json --rules` 仅校验候选。操作依次作用于候选，全部完成后验证引用和规则，失败候选不发布。

## 明确目标和集合修改

记录目标使用实际类型与 ID。CLI 结构化路径分别为：

```json
[
  { "kind": "field", "name": "labels" },
  { "kind": "dict_key", "key": { "kind": "string", "value": "title" } }
]
```

数组片段为 `{ "kind": "index", "index": 0 }`。dict_key.key 是 model 值，dict_set.key 则是 expression 或 model 输入包装，二者层级不同。

| 操作 | 参数（除 op 外） |
| --- | --- |
| `set_field` | record、path、value，可选 expected |
| `unset_field` | record、path |
| `create_record` | record、file，可选 fields 字段对象 |
| `rename_record` / `delete_record` | record；rename 另需 new_id |
| `transfer_record` | record、to_file |
| `array_insert` / `array_remove` | record、path、index；insert 另需 value |
| `array_move` | record、path、from、to |
| `dict_set` / `dict_remove` | record、path、key；set 另需 value |

索引从零开始，前面的数组操作会改变后面的索引。嵌套 Option 和对象层级以 schema 与当前值为准。数组、字典路径指向集合字段。

[record-changes.json](../examples/record-changes.json) 演示字段预期值、数组插入和字典赋值。前提是 `Item::sword.price` 当前为 90，tags 为 `[string]`，labels 为 `{string: string}`；按实际目标修改后再使用。

## 按条件赋值

先查匹配集合：

```sh
coflow data list --project <项目> --type Item --where 'price < 100' --fields price --json
```

核对 `data_scope.complete`、total 和全部必要目标，构造[batch-changes.json](../examples/batch-changes.json)。该示例将 price < 100 的两条 Item 统一设为 100；`expected_count: 2` 只是示例项目的数量，应替换为核对后的实际数量。

JSON selector 使用 `filter`，CLI 使用 `--where`。需要派生记录时，两边一致设置 include_derived / include-derived。selector 的 limit 不是写入上限，set_where 作用于全部匹配；前面的操作可能改变后面的匹配结果。

set_where 提供一个共同值。若需求为“各自涨价 10%”，先读取每条记录的数值，按业务舍入规则计算，生成逐记录 set_field 和 expected，不能把算术表达式假设成 CFD 值或通用更新公式。

## 结果与范围保护

expected_count 不匹配或 revision 变化会返回 conflict。重新核对范围与业务计划，不能只替换数量或修订重试。默认成功响应汇总操作计数，`--details` 才展开成功操作及语义差异。事务 I/O 失败另查实际写入和恢复状态，见[协议](protocol.md)。
