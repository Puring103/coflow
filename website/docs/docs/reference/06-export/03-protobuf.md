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

当前 Protobuf 对带本地化维度的 schema 返回明确诊断；这类项目暂时使用 JSON 或 MessagePack。

