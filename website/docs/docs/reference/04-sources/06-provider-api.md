# Provider API

Provider 是 Coflow 接入外部数据和产物格式的扩展点。当前公开 Provider 分为 loader、writer、exporter 和 codegen 四类。

## ProviderRegistry

Provider 通过 `ProviderRegistry` 注册。默认 registry 由 `coflow-builtins` 组装，CLI 和编辑器使用它获得内置 provider。

| 类别 | 内置 id |
| --- | --- |
| loader / writer | `excel` |
| loader / writer | `csv` |
| loader / writer | `cfd` |
| loader / writer | `lark-sheet` |
| exporter | `json` |
| exporter | `messagepack` |
| codegen | `csharp` |

Engine 只依赖 registry 和 trait，不依赖具体 Provider crate 的实现细节。

## Loader

Loader 实现 `DataLoader`，负责把 source 解析为 input records：

```text
ProjectSourceRef
  -> probe
  -> resolve
  -> preflight
  -> load
  -> LoadedRecords
```

Loader 不负责发现 `coflow.yaml`，不构建 DataModel，也不运行 `check {}`。它只把来源格式转换成来源无关的 input records，并提供可定位的来源信息。

## Writer

Writer 实现 `DataWriter`，负责根据 record origin 写回原始数据源。

常见能力包括：

| 能力 | 说明 |
| --- | --- |
| `can_edit_field` | 能修改字段值 |
| `can_edit_key` | 能修改 record key |
| `can_insert_record` | 能插入新记录 |
| `can_delete_record` | 能删除记录 |
| `requires_full_refresh_after_write` | 写后需要重新加载项目 |
| `is_remote` | 数据源是远端 source |

CLI 写入命令和编辑器都应通过 writer，而不是直接修改 DataModel。

表格类 Provider 还可以注册 table manager，用于 `data create-file`、`data create-table` 和 `data sync-header` 这类 schema-guided 表头维护命令。

内置 writer 当前报告的能力：

| Provider | 编辑字段 | 修改 key | 插入记录 | 删除记录 | 创建表格 | 写后刷新 | 远端 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `excel` | 是 | 是 | 是 | 是 | 是 | 是 | 否 |
| `csv` | 是 | 是 | 是 | 是 | 否 | 是 | 否 |
| `cfd` | 是 | 是 | 是 | 是 | 否 | 是 | 否 |
| `lark-sheet` | 是 | 是 | 是 | 是 | 是 | 是 | 是 |

## Exporter

Exporter 实现 `DataExporter`，负责把已经验证的 DataModel 导出为 artifact 文件集合。

JSON 和 MessagePack 的共享遍历规则在 `coflow-exporter-core` 中实现。具体 exporter 只负责格式编码。

## Codegen

Codegen 实现 `CodeGenerator`，负责根据 schema 或 model 生成运行时代码。

当前内置 codegen 是 `csharp`。C# codegen 读取 schema，并根据 `outputs.data.type` 选择 JSON 或 MessagePack loader。

## 边界

Provider 不负责：

- 读取或发现 `coflow.yaml`。
- 持有项目运行时状态。
- 直接替换导出目录。
- 各自实现业务合法性校验。

项目生命周期由 `coflow-project` 和 `coflow-runtime` 编排；产物落盘由 CLI 宿主负责；业务合法性由 CFT schema、DataModel 和 checker 统一判断。
