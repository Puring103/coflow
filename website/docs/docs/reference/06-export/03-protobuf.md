# Protobuf 导出

Protobuf exporter id 为 `protobuf`，不接受额外配置。一次构建会同时生成表数据和对应契约：

```text
<output>/
  Item.pb
  Reward.pb
  _schema/coflow.proto
```

数据与 `_schema/coflow.proto` 必须来自同一次构建并一起部署。当前不提供跨 schema 构建的 wire
兼容承诺，也不生成 lockfile、schema fingerprint、manifest 或 descriptor set。

记录 `id` 使用字段 1，字段 2 到 15 保留，用户字段按规范字段名排序后从 16 开始编号。
`int` 和 enum 使用 `sint64`，float 使用 `double`，引用使用字符串 key；数组使用 repeated 字段，
字典使用 repeated entry message，多态对象使用 wrapper message 与 `oneof`。

本地化维度会额外生成 variant table。`Item.name` 例如会生成
`Item_nameVariants.pb`，对应的 Protobuf message 为 `ItemNameVariants`：其 `id` 是原记录
key，`default` 和每个维度 variant 都是 optional 字段。因此缺失的翻译在 wire 数据中保持为
字段缺失，而不是空字符串或默认值。

C# Protobuf loader 支持读取这些 variant table。C++ 和 Rust 的 Protobuf loader 目前仍会拒绝
带本地化维度的 schema，因为它们的生成运行时尚未提供安全、可用的 variant table 查询 API；这类
目标暂时使用 JSON。Lua 和 GDScript 目前没有 Protobuf loader。

