# 插件查询与值协议

仅编写数据驱动或可编辑插件时读取，入口和生命周期见[插件编写](plugin-authoring.md)。

`host.data.current()` 返回 `{ sessionId, revision }` 或 null。异步查询返回 `{ sessionId, revision, data }`，使用返回结果前比较当前身份，避免显示旧项目或旧修订的数据。

| 查询 | 返回的 data |
| --- | --- |
| getSchema() | 类型定义数组 |
| getRecordsByType(typeName, { includeFieldValues }) | 记录数组 |
| getRecord(filePath, coordinate) | 单条记录或 null |
| getField(filePath, coordinate, fieldPath) | 字段值或 null |
| searchRecords(query, { mode, limit }) | hits 和 truncated；mode 为 id/full_text |

getSchema 的类型项包含 name、fields、is_singleton、record_count，字段项为 name/type_label。记录的 fields 是单元格数组，单元格有 name、value、missing、annotation；missing 为 true 时 value 是编辑器提供的初值，尚未作为有效赋值写入模型。search hits 含 filePath、coordinate、fieldPath、preview，truncated 表示结果不完整。

coordinate 为 `{ actual_type: 'Item', id: 'sword' }`。路径片段为 `{ kind: 'field', value: 'name' }`、`{ kind: 'index', value: 0 }`、`{ kind: 'dict_key', value: '"key"' }`；字典路径 value 是 CFD 键文本。

```js
// 在用户编辑事件中执行，目标来自当前上下文。
if (!context.readOnly) {
  await host.data.mutate({
    kind: 'write_field',
    filePath: context.filePath,
    coordinate: { actual_type: 'Item', id: 'sword' },
    fieldPath: [{ kind: 'field', value: 'name' }],
    value: { kind: 'string', value: '长剑' }
  })
}
```

mutate 返回新身份，失败拒绝 Promise；捕获并呈现错误，成功后由新上下文刷新展示。

| 修改 kind | 其他参数 |
| --- | --- |
| write_field | filePath、coordinate、fieldPath、value |
| write_fields | filePath、coordinates、fieldPath、value |
| write_field_batch | filePath、writes；每项 coordinate、field_path、new_value |
| edit_collection | filePath、coordinate、fieldPath、edit |
| rename_record | filePath、coordinate、newId |
| insert_record | filePath、recordId、actualType、fields |
| delete_record | filePath、coordinate |
| swap_records | filePath、first、second（记录坐标） |
| move_record | filePath、coordinate、targetIndex |
| transfer_record | sourceFile、destinationFile、coordinate、targetIndex |

edit 支持 array_append（可选 value）、array_remove（index）、array_move（from/to）、dict_insert（key、可选 value）、dict_remove（key）。字典键沿用查询得到的类型化键，例如 `{ kind: 'string', value: 'key' }`。

## 字段值

| kind | value 结构 |
| --- | --- |
| option_none | 无 value |
| option_some | 再嵌套一个字段值 |
| bool / string | JavaScript boolean / string |
| int / float | bigint / 有限 number |
| formatted_string | `{ source, rendered }`，保留模板 source |
| enum | `{ enum_name, variant, value }`；variant 为成员名或 null，value 为 bigint |
| object | `{ actual_type, fields }`；fields 为字段名到字段值的对象 |
| ref | 引用文本身份字符串，保留查询返回的形式 |
| array | 字段值数组 |
| dict | `[字典键, 字段值]` 二元组数组 |

例如整数使用 `{ kind: 'int', value: 100n }`，数组使用 `{ kind: 'array', value: [] }`。不要把 bigint 通过普通 JSON.stringify 转换后再作为宿主值使用。复杂值优先从查询结果保留结构后修改所需部分，避免从展示文本重建而丢失 Option 层级、枚举或对象类型。

insert_record.fields 为 object 字段值，例如 `{ kind: 'object', value: { actual_type: 'Item', fields: { name: { kind: 'string', value: '长剑' } } } }`，还需按目标类型补齐其他必填字段。它不是 CLI 的字段输入对象；CLI expression/model 包装不能传给插件。
