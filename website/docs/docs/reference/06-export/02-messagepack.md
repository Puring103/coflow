# MessagePack 导出格式

MessagePack 导出是 JSON 导出的二进制等价格式。它与 JSON 导出使用相同的 schema-aware 遍历规则：table 选择、字段顺序、多态 `$type`、字典 key 和引用 key 语义保持一致。

## 文件布局

MessagePack 导出产物是一个不可变 generation 目录。每个非 `abstract` CFT type 导出为一个文件：

```text
generated/data/
  Item.msgpack
  Monster.msgpack
  DropTable.msgpack
```

文件名为 `<TypeName>.msgpack`。每个文件内容是裸 MessagePack array，array 中每个元素是一条 record map。

MessagePack 文件没有 Coflow envelope、文件头或 manifest。

## 编码规则

| CFT 类型 | MessagePack 表示 |
| --- | --- |
| `int` | integer |
| `float` | float64 |
| `bool` | boolean |
| `string` | UTF-8 string |
| `T?` 且值为 null | nil |
| enum | integer，底层整数值 |
| object | map |
| polymorphic object | map，带 `$type` string |
| `[T]` | array |
| `{K: V}` | map，key 转 string |
| 记录引用 | 目标 record key string |

每条顶层记录都会导出 `id`，值来自 record key。所有 CFT 字段都会显式导出。

## 和 JSON 的一致性

MessagePack 不是另一套数据语义，而是 JSON 结构的二进制编码：

- 字段名仍是 schema 字段名。
- enum 仍导出底层整数值。
- 字典 key 仍转成字符串。
- 多态对象仍使用 `$type`。
- 引用仍保存目标 record key。

因此同一个项目切换 JSON / MessagePack 时，运行时看到的逻辑数据应保持一致。

## 空表

MessagePack exporter 会为非 `abstract` type 写出 `<TypeName>.msgpack`，即使该 table 没有记录。空表文件内容是空 array。

这一点与 JSON 导出不同：JSON 导出不会为没有记录的 table 写空 `[]` 文件。

## 字段顺序

record map 的字段顺序遵循 CFT schema 的继承展开顺序：父类字段先于子类字段，同一 type 内按声明顺序输出。

多态对象中 `$type` 是 map 的第一项，后面再写实际类型字段。生成的 C# MessagePack loader 会先读取 `$type`，再分发到具体类型 reader。

## 输出目录

`outputs.data.dir` 或 `--out` 是 generation 的放置锚点。Coflow 在同级位置写入并验证新的不可变 generation，再通过项目目录下 `.coflow/artifacts/active.json` 单点激活完整 snapshot。命令成功信息会输出实际 generation 目录；程序也可读取 manifest 中 `outputs.data.generation_dir`。

不要修改 generation 中的文件。后续导出会创建新 generation，旧 generation 保持不变。

## 示例结构

逻辑上，一个 `Item.msgpack` 文件等价于下面的 JSON 结构：

```json
[
  {
    "id": "sword_fire",
    "name": "Fire Sword",
    "rarity": 10,
    "tags": ["weapon", "melee"]
  }
]
```

一个多态字段等价于：

```json
{
  "reward": {
    "$type": "ItemReward",
    "item": "sword_fire",
    "count": 1
  }
}
```
