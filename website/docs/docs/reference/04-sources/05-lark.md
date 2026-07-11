# 飞书/Lark Source

飞书/Lark source 使用 `lark-sheet` Provider 读取远端电子表格。它与 Excel 共享表格语义：sheet、type、key、columns 和单元格值解析规则一致。

## 配置示例

```yaml
sources:
  - url: lark:sht_xxxxx
    type: lark-sheet
    app_id: cli_xxx
    app_secret: xxx
    sheets:
      - sheet: Item
        type: Item
```

`url` 支持三类格式：

| 格式 | 说明 |
| --- | --- |
| `lark:<spreadsheet_token>` | 直接指定电子表格 token |
| `https://.../sheets/<token>` | 从飞书/Lark 表格 URL 中提取电子表格 token |
| `https://.../wiki/<token>` | 先解析 wiki 节点，再读取其指向的电子表格 |

HTTPS URL 需要是包含 `feishu` 或 `larksuite` 的飞书/Lark 地址。`app_id` 和 `app_secret` 是必填 Provider options，用于获取 tenant access token。远端表格的 sheet 映射规则见 [表格 Source](./02-table.md)。

已知电子表格 token 时，推荐直接使用 `lark:<spreadsheet_token>`：

```yaml
sources:
  - type: lark-sheet
    url: lark:sht_xxxxx
    app_id: cli_xxx
    app_secret: xxx
```

## 加载边界

Lark Provider 负责：

- 解析 Lark URL 或 token。
- 获取 tenant access token。
- 读取 wiki / spreadsheet / sheet 元数据。
- 读取表格值。
- 将远端表格值转换成共享表格模型。

表头、key、column 和 cell 语义与 Excel/CSV 一致。远端 API、鉴权、URL 和 wiki 解析问题使用 `LARK-*` 诊断。

## 写回

Lark 的远端 mutation 暂不对 runtime 和编辑器开放。原子 mutation 要求远端 provider 提供可验证的 compensation handle；当前 Lark adapter 尚未实现该契约，因此不会广告必然失败的写能力：

| 能力 | 支持 |
| --- | --- |
| 编辑字段 | 否 |
| 修改 record key | 否 |
| 插入记录 | 否 |
| 删除记录 | 否 |
| 创建 sheet/table | 是 |
| 写后完整刷新 | 否 |
| 远端 source | 是 |

自动化命令和编辑器会根据 writer 报告的能力禁用 mutation。绕过 capability 直接发起 mutation 会在任何远端写入前返回 `WRITE-TXN-UNSUPPORTED`。创建 sheet/table 走独立的 `TableManager` interface，不受该限制。

### 创建远端 sheet

`coflow data create-table` 可以在已配置的 Lark spreadsheet 中创建 sheet，并写入目标 CFT type 的表头：

```powershell
coflow data create-table <project> --source lark:sht_xxxxx --type Item --provider lark-sheet --sheet Item
```

`--source` 必须能匹配 `coflow.yaml` 中的 Lark source；命令会复用该 source 的 `app_id`、`app_secret` 和 `sheets` 映射配置。若目标 sheet 已存在，命令会返回诊断而不是覆盖。
