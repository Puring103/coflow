# Coflow 数据维护手册

## 编辑决策

| 目标 | 首选方式 |
| --- | --- |
| 查看 source 和 writer 能力 | `coflow data sources <project>` |
| 查记录索引 | `coflow data list <project> --type <Type>` |
| 查完整记录 | `coflow data get <project> Type.key` |
| 新建 CSV/XLSX | `coflow data create-file --file <file> --type <Type>` |
| 新建 CFD | `coflow data create-file --file <file> --provider cfd` |
| schema 字段变更后同步本地文件 | `coflow data sync-header --file <file> --type <Type>` |
| 少量记录增删改/重命名 | `coflow data patch --patch '<json>'` |
| 复杂 CFD 整理 | `coflow data write-file --file <file.cfd> --stdin --check` |
| 远端飞书/Lark 表格 | 只用 `data patch`，不要整文件写入 |

## 新增记录

```json
{
  "stop_on_write_error": true,
  "ops": [
    {
      "op": "insert_record",
      "file": "data/items.csv",
      "type": "Item",
      "key": "potion",
      "fields": {
        "name": "Potion",
        "price": 25,
        "tags": ["shop", "consumable"]
      }
    }
  ]
}
```

## 修改字段

```json
{
  "stop_on_write_error": true,
  "ops": [
    {
      "op": "set_field",
      "record": { "type": "Item", "key": "potion" },
      "file": "data/items.csv",
      "path": [{ "kind": "field", "value": "price" }],
      "value": 40
    },
    {
      "op": "set_field",
      "record": { "type": "DropTable", "key": "forest" },
      "path": [
        { "kind": "field", "value": "rewards" },
        { "kind": "index", "value": 0 },
        { "kind": "field", "value": "count" }
      ],
      "value": 3
    }
  ]
}
```

字典 key 使用 `dict_key`：

```json
{
  "op": "set_field",
  "record": { "type": "Monster", "key": "slime" },
  "path": [
    { "kind": "field", "value": "resistances" },
    { "kind": "dict_key", "value": "Element.Fire" }
  ],
  "value": 0.5
}
```

## 引用、多态和字典值

```json
{
  "$ref": "sword_fire"
}
```

```json
{
  "$type": "ItemReward",
  "item": { "$ref": "sword_fire" },
  "count": 1
}
```

```json
{
  "$dict": [
    { "key": "Fire", "value": 10 },
    { "key": "Ice", "value": 5 }
  ]
}
```

- CFT 字段类型为 `&Type`、`[&Type]` 或 `{key: &Type}` 时使用 `$ref`。
- 普通 `Type`、`[Type]` 或 `{key: Type}` 字段使用 inline object；多态对象加 `$type`。
- 非字符串 key 的字典用 `$dict`，避免 JSON object key 无法表达 enum/int 类型。

## 删除和重命名

```json
{
  "ops": [
    {
      "op": "rename_record",
      "record": { "type": "Item", "key": "steel_sword" },
      "new_key": "steel_blade"
    },
    {
      "op": "delete_record",
      "record": { "type": "Item", "key": "old_sword" },
      "file": "data/items.csv"
    }
  ]
}
```

## CFD 编写要点

- 顶层记录写 `key: Type { ... }`，或放在 `Type { key { ... } }` 分组中。
- record key 承担 id 语义，不要在顶层记录里再写 `id` 字段。
- 记录引用只写 `&key`，不支持 `&key.field` 或索引路径。
- 字段类型为 `&Type` 时写引用；字段类型为普通 `Type` 时写内联对象。
- 数组、对象、字典条目用逗号分隔；表格单元格数组才使用 `|`。
- spread 使用 `...&key` 或字典/object spread，后出现的值覆盖前面的值。

## 收尾检查

```powershell
coflow check <project>
```

如果需要更新导出产物，再运行：

```powershell
coflow build <project>
```

出现写入失败或诊断时，先读取 JSON 报告，不要只看退出码。重点字段是 `write_ok`、`check_ok`、`applied`、`failed`、`remaining_ops` 和 `diagnostics`。
