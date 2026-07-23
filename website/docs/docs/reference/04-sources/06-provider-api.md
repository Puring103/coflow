# Provider API

Provider 是 Coflow 接入外部数据和产物格式的扩展点。当前公开 Provider 分为 loader、writer、exporter 和 codegen 四类。

## ProviderRegistry

Provider 通过 `ProviderRegistry` 注册。CLI 和编辑器默认提供以下 Provider：

| 类别 | 内置 id |
| --- | --- |
| loader / writer | `excel` |
| loader / writer | `csv` |
| loader / writer | `cfd` |
| exporter | `json` |
| exporter | `messagepack` |
| codegen | `csharp` |

## Loader

Loader 实现 `DataLoader`，负责识别 source、校验配置并读取 records。

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

CLI 写入命令和编辑器都应通过 writer，而不是直接修改 DataModel。

表格类 Provider 还可以注册 table manager，用于 `data create-file`、`data create-table` 和 `data sync-header` 这类 schema-guided 表头维护命令。

内置 writer 当前报告的能力：

| Provider | 编辑字段 | 修改 key | 插入记录 | 删除记录 | 创建表格 | 写后刷新 |
| --- | --- | --- | --- | --- | --- | --- |
| `excel` | 是 | 是 | 是 | 是 | 是 | 是 |
| `csv` | 是 | 是 | 是 | 是 | 否 | 是 |
| `cfd` | 是 | 是 | 是 | 是 | 否 | 是 |

## Exporter

Exporter 实现 `DataExporter`，负责把已经验证的 DataModel 导出为 artifact 文件集合。

Exporter 应校验自己的配置并返回相对路径明确、内容完整的产物集合。内置 JSON 和 MessagePack exporter 不接受额外 options，未知字段会返回诊断。

## Codegen

Codegen 实现 `CodeGenerator`，负责根据 schema 或 model 生成与数据格式无关的运行时代码。

当前内置 codegen 是 `csharp`。它生成公共声明和 table API，不负责选择数据格式。

Codegen 应校验自己的配置，并根据调用方提供的 schema 生成完整代码产物。

## Loader generator

Loader generator 实现 `LoaderGenerator`，声明支持的 code/data 组合并生成对应加载代码。项目可以通过 `loader.type` 显式选择；省略时，宿主选择已注册且匹配该组合的 loader。

内置组合为 `csharp-json` 和 `csharp-messagepack`。

## 边界

Provider 不负责：

- 读取或发现 `coflow.yaml`。
- 持有项目运行时状态。
- 直接替换导出目录。
- 各自实现业务合法性校验。

宿主负责项目加载和产物落盘；Coflow 的 schema、DataModel 和 checker 负责统一的数据合法性校验。
