# Provider API

Provider 是 Coflow 接入外部数据和产物格式的扩展点。当前公开 Provider 分为 loader、writer、exporter 和 codegen 四类。

## ProviderRegistry

Provider 通过 `ProviderRegistry` 注册。默认 registry 由 `coflow-builtins` 组装，CLI 和编辑器使用它获得内置 provider。

| 类别 | 内置 id |
| --- | --- |
| loader / writer | `excel` |
| loader / writer | `csv` |
| loader / writer | `cfd` |
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

JSON 和 MessagePack 的共享遍历规则在 `coflow-exporter-core` 中实现。具体 exporter 只负责格式编码。

宿主先调用 exporter 的 `decode_options`，把 `coflow.yaml` 中 project-facing JSON
解码为带 provider identity 的 opaque `DecodedOutputOptions`。`export` 只接收这份
typed options，不接收 provider id 或宿主解析出的 output directory。JSON 和
MessagePack 当前都不接受自定义 options，未知字段会在 generation 前返回诊断。

## Codegen

Codegen 实现 `CodeGenerator`，负责根据 schema 或 model 生成与数据格式无关的运行时代码。

当前内置 codegen 是 `csharp`。它生成公共声明和 table API，不负责选择数据格式。

Codegen 使用与 exporter 相同的 output option contract：`decode_options` 只处理
project-facing 配置，`generate` 只接收 `DecodedOutputOptions`。`@idAsEnum` variants
由宿主通过 `CodegenContext` 提供，不伪装成用户 options。provider identity 或具体
option 类型不匹配属于 contract diagnostic。

## Loader generator

Loader generator 实现 `LoaderGenerator`，声明一个精确的 `(code, data)` 兼容组合，并生成对应的加载代码。宿主可以按 `loader.type` 显式选择，也可以从注册顺序中选择第一个兼容 provider。loader 同时接收 code/data/loader 三组 decoded options 和 schema；完整 build 还提供绑定到同一次 runtime generation 的 DataModel，schema-only codegen 则不提供 model。

内置组合为 `csharp-json` 和 `csharp-messagepack`。公共 C# codegen 因此不再包含数据格式判断；新增 exporter 或 codegen 不会隐式改变已有组合。

## 边界

Provider 不负责：

- 读取或发现 `coflow.yaml`。
- 持有项目运行时状态。
- 直接替换导出目录。
- 从 raw JSON 反复解析 output options。
- 各自实现业务合法性校验。

项目生命周期由 `coflow-project` 和 `coflow-runtime` 编排；产物落盘由 CLI 宿主负责；业务合法性由 CFT schema、DataModel 和 checker 统一判断。
