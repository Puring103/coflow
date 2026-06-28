# Coflow 编辑手册

## 编辑决策

| 目标 | 首选方式 |
|------|----------|
| 修改 `coflow.yaml` | 直接编辑 YAML，随后运行 `coflow schema inspect` 或 `coflow check` |
| 修改 CFT schema | `coflow schema write-file <project> --file <x.cft> --stdin --check` |
| 新建 CSV/XLSX | `coflow data create-file --file <file> --type <Type>` |
| 新建 CFD | `coflow data create-file --file <file> --provider cfd` |
| schema 字段变更后同步文件 | `coflow data sync-header --file <file> --type <Type>` |
| 少量记录增删改 | `coflow data patch --patch patch.json` |
| 复杂 CFD 整理 | `coflow data write-file --file <file.cfd> --stdin --check` |
| 远端 Lark/飞书表格 | 只用 `data patch`，不要整文件写入 |

## `coflow.yaml`

配置文件路径相对 `coflow.yaml` 所在目录解析。编辑时保留用户原有缩进和顺序，不把生成输出目录加入 `sources`。

```yaml
schema:
  - schema/main.cft

sources:
  - path: data/items.csv
    type: csv
    sheets:
      - sheet: Items
        type: Item
        key: Item ID
        columns:
          Display Name: name
  - path: data/story.cfd
  - type: lark-sheet
    url: lark:<spreadsheet_token>
    app_id: ${LARK_APP_ID}
    app_secret: ${LARK_APP_SECRET}
    sheets:
      - sheet: Item
        type: Item

outputs:
  data:
    type: json
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Game.Config
```

规则：

- `schema` 是精确小写 `.cft` 文件、目录或列表。
- `sources[].path` 是本地文件或目录；`sources[].url` 是远端 source；二者只能选一个。
- `type` 可省略，由 provider probe 推断；远端表格通常显式写 `lark-sheet`。
- `sheets[].sheet` 是工作表名；`type` 是 CFT 类型；`key` 是 record key 列；`columns` 是表头到字段名的重命名映射，不是白名单。
- `outputs.data.type` 支持 `json`、`messagepack`；`outputs.code.type` 当前为 `csharp`。
- `outputs.*.dir` 是生成目录，不要放手写文件，不要作为数据 source。

改配置后至少运行：

```powershell
coflow schema inspect <project>
coflow check <project>
```

## 维度和变体配置

schema 中出现 `@localized` 字段时，配置 `dimensions.language`：

```yaml
dimensions:
  language:
    variants: [zh, en, ja]
    out_dir: data/dimensions/language
```

规则：

- `variants` 必须非空、不能重复、必须是合法 CFT 标识符，不能包含保留名 `default`。
- `out_dir` 必须显式配置。维度文件由引擎维护，不需要再写进 `sources`。
- 旧 `localization:` 顶层配置已移除，不要新增。
- 修改 `variants` 后运行 `coflow check` 或 `coflow build`，让维度文件同步列。

## 复杂 patch

引用、字典、多态和 nullable 用结构化 JSON 表达：

```json
{
  "ops": [
    {
      "op": "insert_record",
      "file": "data/items.csv",
      "type": "Item",
      "key": "fire_sword",
      "fields": {
        "name": "Fire Sword",
        "rarity": "Rare",
        "tags": ["weapon", "fire"],
        "stats": { "attack": 30, "speed": 1.2 },
        "owner": { "$ref": "Character.hero" },
        "weights": { "$dict": [{ "key": "Fire", "value": 10 }] },
        "reward": { "$type": "ItemReward", "item": { "$ref": "Item.fire_sword" }, "count": 1 },
        "next": null
      }
    }
  ]
}
```

对象字段默认既可写引用也可写内联对象；schema 字段带 `@ref` 时 patch 必须使用
`{ "$ref": "Type.key" }`，带 `@inline` 时必须使用 inline object，例如
`{ "$type": "ItemReward", ... }` 或普通对象字段 map。`@ref` / `@inline` 标在
`[Item]` 或 `{string: Item}` 字段上时，会约束数组元素或字典 value。

字段修改使用路径：

```json
{
  "ops": [
    {
      "op": "set_field",
      "record": { "type": "Item", "key": "fire_sword" },
      "file": "data/items.csv",
      "path": ["stats", "attack"],
      "value": 45
    },
    {
      "op": "set_field",
      "record": { "type": "DropTable", "key": "forest" },
      "path": ["rewards", 0, "count"],
      "value": 3
    }
  ]
}
```

`set_field.path` 只支持字段名和数组索引，例如 `["stats", "attack"]`、`["rewards", 0, "count"]`。当前不支持用 path 直接定位字典 key；需要改字典项时，重写拥有该字典的字段值。

执行后检查 JSON：`write_ok`、`check_ok`、`applied`、`failed`、`diagnostics`。`data patch` 可能部分落盘，出现 `failed` 必须继续修复。

## Excel、CSV 和 Lark

- CSV/XLSX/Lark 表格编辑都走 `data patch`，不要直接改行文本。
- 多 sheet 文件先看 `coflow data sources <project>` 和 `coflow.yaml` 的 `sheets` 映射。
- 新建本地表格用 `data create-file`，字段变化用 `data sync-header`。
- `data sync-header` 不支持远端 Lark；远端表格只通过 patch writer 做记录级写入。
- 对用户指定文件或 sheet 的修改，patch 里尽量带 `file` guard。
- 表格对象单元格可写 `TypeName{field: value}`；多态对象用 `ConcreteType{...}`。

## 收尾

常规编辑后运行：

```powershell
coflow check <project>
```

需要生成产物时再运行：

```powershell
coflow build <project>
```
