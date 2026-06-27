# Coflow Schema/Data 工作流

## 读取项目

优先从用户给出的路径解析项目；没有路径时使用当前目录。

```powershell
coflow schema inspect <project>
coflow schema files <project>
coflow data sources <project>
```

`schema inspect` 适合读取结构化类型和字段；`schema files` 适合读取注释、check block 和原始 CFT 文本。

## 修改 schema

优先用 CLI 写项目配置包含的 `.cft` 文件，不要让 agent 任意写路径。先可 dry-run：

```powershell
coflow schema write-file <project> --file schema/main.cft --stdin --dry-run --check
```

确认后写入并检查：

```powershell
coflow schema write-file <project> --file schema/main.cft --stdin --check
```

命令只允许写当前 schema 配置展开出的 `.cft` 文件。`--check` 发现诊断时会返回非零；
非 dry-run 模式下文件已经写入，应继续修复后再次运行。命令不可用时才直接编辑 `.cft`，
然后运行 `coflow schema inspect <project>` 或 `coflow cft check <project>`。

如果字段新增、删除或重命名影响表格数据源，对每个受影响文件运行：

```powershell
coflow data sync-header <project> --file data/items.csv --type Item
```

CSV/XLSX 同步表头并保留同名列数据；CFD 不写表头，而是重写匹配类型记录的顶层字段。

## 新建数据文件

```powershell
coflow data create-file <project> --file data/items.csv --type Item --provider csv
coflow data create-file <project> --file data/items.cfd --provider cfd
```

CSV/XLSX 会创建 schema 表头。CFD 只创建空文件，因为 CFD 记录没有表头。

## 添加记录

```json
{
  "ops": [
    {
      "op": "insert_record",
      "file": "data/items.csv",
      "type": "Item",
      "key": "potion",
      "fields": {
        "name": "Potion",
        "price": 25
      }
    }
  ]
}
```

运行：

```powershell
coflow data patch <project> --patch patch.json
```

## 修改记录

```json
{
  "ops": [
    {
      "op": "set_field",
      "record": { "type": "Item", "key": "potion" },
      "file": "data/items.csv",
      "path": ["price"],
      "value": 40
    }
  ]
}
```

## 重写 CFD 文件

复杂嵌套、模板覆盖或批量整理 CFD 文本时，可整文件写入：

```powershell
coflow data write-file <project> --file data/items.cfd --stdin --check
```

先预览是否会改动：

```powershell
coflow data write-file <project> --file data/items.cfd --stdin --dry-run
```

该命令只允许写配置内本地 CFD source 覆盖的 `.cfd` 文件：未指定 `type` 的目录/`.cfd`，
或显式 `type: cfd`。`--check` 只在非 dry-run 写入后运行；发现诊断时文件已经写入，
应继续修复后再次运行。CSV/XLSX 不使用该命令。

## 删除记录

```json
{
  "ops": [
    {
      "op": "delete_record",
      "record": { "type": "Item", "key": "potion" },
      "file": "data/items.csv"
    }
  ]
}
```

## 特殊值

```json
{ "$ref": "Item.sword_01" }
{ "$ref": { "type": "Item", "key": "sword_01" } }
{ "$type": "ItemReward", "item": { "$ref": "Item.sword_01" }, "count": 1 }
{ "$dict": [{ "key": "Fire", "value": 10 }] }
```

`$ref` 写 record 引用；`$type` 写多态 inline object；`$dict` 写非字符串 key 的字典。

## 结束检查

```powershell
coflow check <project>
```

如果在 Coflow 仓库内修改代码，还要运行仓库级 cargo 检查。
