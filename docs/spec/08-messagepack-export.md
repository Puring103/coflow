# MessagePack 导出格式

**依赖文档**：[02-data-model.md](02-data-model.md)

MessagePack 导出是 JSON 导出的二进制等价格式。它的输入是已经通过 schema 编译、Excel loader 加载、`DataModel` 构建和 check 检查的 `CfdDataModel`，`@ref` 保留原始 ID，由运行时加载器负责解析引用。

MessagePack exporter 位于 `coflow-exporter-messagepack`。它与 `coflow-exporter-json` 共用 `coflow-exporter-core` 的 schema-aware 遍历规则，因此表选择、字段顺序、多态 `$type`、字典 key 和 `@ref` ID 保留语义与 JSON 导出一致。

---

## 文件结构

MessagePack 导出产物是一个输出目录。每个 table（对应 CFT 类型名）导出为一个 `<TypeName>.msgpack` 文件：

```text
out/
  Item.msgpack
  Monster.msgpack
  DropTable.msgpack
```

每个文件内容是裸 MessagePack array。array 中每个元素是一条 record map，记录顺序保持数据源顺序：

```text
Item.msgpack:
[
  { "id": "sword_01", "name": "铁剑" },
  { "id": "potion_01", "name": "药水" }
]
```

“裸 MessagePack”表示文件没有 Coflow envelope，也没有额外文件头。根值必须直接是 table array。

---

## 编码规则

| CFT 类型 | MessagePack 表示 | 说明 |
|---------|------------------|------|
| `int` | integer | 使用整数编码 |
| `float` | float64 | 使用 64 位浮点编码 |
| `bool` | boolean | `true` / `false` |
| `string` | string | UTF-8 string |
| `null` / nullable 且为 null | nil | MessagePack nil |
| `enum` | integer | 枚举底层整数值 |
| `type`（非多态） | map | 字段名为 string key |
| `type`（多态） | map，带 `$type` string | `$type` 写实际类型名 |
| `[T]` | array | 元素按数组顺序写出 |
| `{K: V}` | map | key 统一编码为 string |
| `@ref` 字段 | 原始 ID 值 | string 或 integer，不内联目标对象 |

所有字段均显式导出，含有默认值的字段也写出，不依赖消费端自行填充默认值。

---

## 字段顺序

record map 的字段顺序遵循 schema 的继承展开顺序：父类字段先于子类字段，同一类型内按 schema 声明顺序输出。实现不能依赖 `CfdRecord.fields` 的 map 迭代顺序。

多态对象必须把 `$type` 作为 map 中第一项，然后再写继承展开后的字段。这样生成的 C# MessagePack loader 可以先读取 `$type`，再分发到实际类型的字段 reader。

---

## 字典和引用

MessagePack map key 可以不是 string，但 Coflow 导出为了和 JSON 保持同一语义，字典 key 统一写为 string：

| CFT key 类型 | MessagePack key 示例 | 说明 |
|-------------|----------------------|------|
| `string` | `"alice"` | 直接作为 string key |
| `int` | `"1"`、`"42"` | 十进制数字字符串 |
| `enum` | `"1"`、`"10"` | 枚举底层整数值的十进制字符串 |

`@ref` 字段导出为原始 ID 值，不内联目标对象，避免数据膨胀和循环引用问题。运行时加载器根据 ID 和 schema 中 `@ref` 的目标类型解析引用。

---

## 第一版非目标

第一版 MessagePack 文件不包含：

- 文件头或 magic number
- manifest
- schema hash
- 加密
- 完整性校验或 checksum
- 压缩

这些能力可以在未来版本通过独立 envelope 或 manifest 引入，但当前 `<TypeName>.msgpack` 文件本身只包含裸 MessagePack array。
