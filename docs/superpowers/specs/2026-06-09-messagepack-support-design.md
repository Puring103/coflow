# MessagePack 支持设计

## 目标

为 Coflow 增加一等的 MessagePack 运行时数据支持，同时保留现有 JSON 支持。这个功能包含 MessagePack 数据导出、面向普通 .NET 和 Unity/IL2CPP/AOT 的 C# 运行时加载器生成，以及一次 crate 命名整理，让 loader、exporter、codegen 的职责边界更清晰。

本版本暂不加入加密、完整性校验、文件头或 manifest。

## 已确认决策

- MessagePack 数据文件使用 `.msgpack` 扩展名。
- 仍然保持一张表一个文件：`<TypeName>.msgpack`。
- 文件内容是裸 MessagePack，不包 Coflow envelope。
- 本版本不生成 manifest。
- 运行时代码生成直接根据 `outputs.data.type` 决定数据格式；`outputs.code` 不再配置独立的数据格式。
- MessagePack C# 加载器使用生成出来的显式 reader，不使用 typeless、反射式或动态 resolver 反序列化。
- 本次一起整理 crate 命名。

## Workspace 布局

workspace 应整理为按职责命名的 crate：

```text
crates/
  coflow-cft/
  coflow-cell-value/
  coflow-data-model/
  coflow-checker/
  coflow-loader-excel/
  coflow-exporter-core/
  coflow-exporter-json/
  coflow-exporter-messagepack/
  coflow-codegen-csharp/
  coflow-project/
  coflow-cft-lsp/
```

现有 crate 重命名如下：

| 当前 crate | 新 crate |
| --- | --- |
| `coflow-excel-loader` | `coflow-loader-excel` |
| `coflow-json-export` | `coflow-exporter-json` |

`coflow-codegen-csharp` 已经符合目标命名风格，不需要改名。`coflow-cft`、`coflow-data-model`、`coflow-checker`、`coflow-project` 这类核心 crate 不属于 loader、exporter 或 codegen，也保持现名。

## Exporter 架构

`coflow-exporter-core` 负责复用已经验证过的 `CfdDataModel` 和编译后的 `CftContainer`，提供格式无关的 schema-aware 导出遍历。这样 JSON exporter 和 MessagePack exporter 不会重复实现 schema 遍历、表选择、字段顺序、多态 `$type`、`@ref` ID 保留和字典 key 处理。

不新增一套公共 `ExportValue` 数据模型。`CfdDataModel` 已经是 source-neutral 的验证后数据模型，里面有 `CfdValue`、`CfdRecord`、`CfdTable`、`CfdDictKey`、`CfdIdValue` 等结构。再引入一个公共 `ExportValue` 会让项目出现第二套“数据模型”，增加同步和命名成本。

导出层仍然需要公共逻辑，但公共逻辑应该是“如何按导出语义遍历 `CfdDataModel`”，而不是“把 `CfdDataModel` 复制成另一个树”。

`coflow-exporter-core` 应封装这些共享规则：

- 只导出非 abstract 且带 `@id` 的 concrete table，并为缺失数据的 table 导出空表；
- 按 schema 的继承展开顺序输出字段，而不是按 `CfdRecord.fields` 的 `BTreeMap` 字典序；
- 在声明类型是多态范围时插入 `$type`；
- 把 `CfdValue::Ref { id, target }` 按 `@ref` 字段导出为原始 ID，而不是内联目标记录；
- 把 enum 导出为底层整数值；
- 把 dict key 统一导出为 string key；
- 按声明类型处理 nullable、array、dict 和 object 的递归编码。

具体 API 可以是 visitor/encoder 风格，例如让 `coflow-exporter-core` 驱动遍历，JSON 和 MessagePack 分别实现自己的 writer。JSON exporter 可以在 writer 中构建 `serde_json::Value`，MessagePack exporter 可以直接写 MessagePack bytes。核心约束是：公共 crate 复用导出遍历和语义判断，但不拥有第二套完整数据表示。

`coflow-exporter-json` 使用 core traversal 生成并写出 pretty JSON，保持现有 JSON 文件形状不变。

`coflow-exporter-messagepack` 使用同一套 core traversal 直接写裸 MessagePack bytes，不经过 JSON 文本。实现可以使用 `rmp` / `rmp-serde`，但公开 API 应暴露表 bytes 或 writer-oriented API，不应该让 CLI 层了解具体编码细节。

## MessagePack 格式

MessagePack 表格式语义上对齐现有 JSON 导出模型：

| CFT 概念 | MessagePack 表示 |
| --- | --- |
| 表文件 | array |
| 记录 | map |
| 字段 key | 使用 CFT 源字段名的 string |
| `int` | integer |
| `float` | floating point |
| `bool` | boolean |
| `string` | string |
| nullable null | nil |
| enum | 底层 integer 值 |
| object | map |
| 多态 object | 带 `$type` string 的 map |
| array | array |
| dict | string key 的 map |
| `@ref` | 原始 ID 值 |

这个格式已经比 JSON 紧凑，因为去掉了文本标点和字符串转义开销；但第一版不把 record 改成位置数组。位置数组会更小，但会降低版本兼容性、可检查性和生成加载器的错误定位能力。第一版保留字段名 map，让 MessagePack 和 JSON 在语义上保持一致。

逻辑结构示例：

```text
Item.msgpack:
[
  { "id": "sword_01", "name": "Iron Sword", "rarity": 10 }
]
```

本版本文件中没有 magic、version、schema hash、compression marker、encryption marker 或 checksum。

## Project 配置和 CLI

项目配置继续使用 `outputs.data.type`：

```yaml
outputs:
  data:
    type: messagepack
    dir: generated/data
  code:
    type: csharp
    dir: generated/csharp
    namespace: Example.Rpg.Config
```

支持的数据输出类型：

- `json`
- `messagepack`

CLI 命令：

```bash
coflow export json examples/rpg
coflow export messagepack examples/rpg
coflow codegen csharp examples/rpg
```

`coflow export messagepack` 要求 `outputs.data.type: messagepack`，与 `coflow export json` 要求 `outputs.data.type: json` 的行为一致。

`coflow codegen csharp` 读取 `outputs.data.type` 并生成匹配的运行时加载器：

- `json` 生成基于 Newtonsoft.Json 的 `.json` 加载器。
- `messagepack` 生成基于 MessagePack 的 `.msgpack` 加载器。

如果 `outputs.data` 缺失，或使用了不支持的类型，`coflow codegen csharp` 返回清晰错误。不存在 `outputs.code.data_format` override。

`coflow init` 可以暂时继续默认生成 JSON 配置，以保留最容易上手的初始项目。

## C# 运行时加载器

生成的 C# 运行时仍然是 trusted artifact loader。它加载 Coflow exporter 产出的数据，而这些数据已经经过 Rust pipeline 的解析、检查和数据模型构建。加载器应对运行时常见问题给出有用错误，例如文件缺失、MessagePack 格式损坏、重复 ID、`@ref` 解析失败；但它不是任意手写二进制数据的完整 validator。

对于 MessagePack，生成的 C# 代码必须兼容普通 .NET 和 Unity/IL2CPP/AOT。为了避免反射和运行时生成 resolver，加载器使用基于低层 `MessagePackReader` API 的显式生成读取方法。

生成的 MessagePack loader 与 JSON loader 保持同样的高层流程：

1. 加载每个 `<TypeName>.msgpack` 表文件。
2. 把根节点读取为 array。
3. 通过生成的 `Load<Type>` 函数构造强类型记录。
4. 构建 `@id` 唯一索引。
5. 构建 `@index` 多值索引。
6. 需要时构建多态 `@ref` 索引。
7. 第二遍解析 `@ref` 字段。
8. 返回生成的数据库对象。

MessagePack reader helper 应像 JSON helper 一样由模板或生成代码提供，但它们直接操作 `MessagePackReader`：

```csharp
private delegate T MessagePackRowLoader<T>(
    ref MessagePackReader reader,
    string path);

private static List<T> LoadTable<T>(
    string file,
    string tableName,
    MessagePackRowLoader<T> loadRow)
```

生成的 object loader 使用类似签名：

```csharp
private static Item LoadItem(ref MessagePackReader reader, string path)
```

每个 object loader 读取 MessagePack map header，遍历 map entry，先读取 string 字段 key，再用 `switch` 分发已知字段到生成的字段 reader。未知字段用 `reader.Skip()` 跳过，这样在必填字段仍然存在的情况下，旧版生成代码可以容忍未来 exporter 增加的新字段。必填字段用生成的 `has<Field>` boolean 跟踪；重复的已知字段 key 抛出 `CftLoadException`。

这种方式能保持 MessagePack loader 紧凑且 AOT-safe，同时不需要把数据物化成动态 dictionary，也不依赖运行时生成 resolver。

多态字段继续使用 object map 里的 `$type` entry。MessagePack 加载器根据这个 string 分发，行为与 JSON 加载器读取 `$type` JSON property 后分发一致。

导出的 MessagePack dict key 仍然是 string。生成的 C# loader 根据字段 schema 把 string key 转回 `string`、`long` 或 enum key。

## C# 依赖

生成的 JSON loader 继续依赖 `Newtonsoft.Json`。

生成的 MessagePack loader 依赖 MessagePack-CSharp。生成器不 vendor 依赖包，也不输出依赖包文件。项目集成由用户负责：

- 普通 .NET：添加 NuGet package。
- Unity：通过项目已有的包管理流程安装 Unity 兼容的 MessagePack-CSharp 包或 NuGet 包。

生成代码不使用 typeless API，也不使用需要运行时代码生成的 resolver 功能。

## 文档

新增 MessagePack 规格文档，例如：

```text
docs/spec/08-messagepack-export.md
```

更新现有只提到 JSON 的文档，说明 project pipeline 现在支持 JSON 或 MessagePack。

C# codegen 规格应说明 loader generation 跟随 `outputs.data.type`。

## 测试策略

实现改动前应先补测试。

Exporter core 测试：

- 为所有非 abstract 且带 `@id` 的类型导出 table，包括空表。
- 保留字段顺序、默认值展开、`@ref` ID、`$type`、nullable、array、dict。
- 当 model 数据无法匹配编译后的 schema 时返回错误。

JSON exporter 测试：

- 共享逻辑迁移到 `coflow-exporter-core` 后，现有 JSON 期望仍然通过。
- JSON 输出语义保持不变。

MessagePack exporter 测试：

- 使用包含 scalar、enum、nullable、nested object、polymorphic object、array、dict、`@ref` 的 schema/model 导出。
- 在 Rust 测试中 decode 产出的 MessagePack，并断言它匹配预期导出结构。
- 验证 CLI 导出使用 `.msgpack` 文件。

CLI 测试：

- `coflow export messagepack examples/rpg --out <dir>` 写出预期 `.msgpack` 表文件。
- `coflow export messagepack` 在 `outputs.data.type: json` 时拒绝执行。
- `coflow codegen csharp` 在 `outputs.data.type: messagepack` 时生成 `.msgpack` 文件路径和 MessagePack reader 代码。
- `coflow codegen csharp` 在 `outputs.data.type: json` 时仍生成 JSON loader 代码。

C# codegen 测试：

- 现有 JSON loader 生成测试继续覆盖。
- MessagePack loader 生成结果包含 MessagePack-CSharp imports、`.msgpack` 路径、生成字段 reader、`$type` 分发和 `@ref` 解析。
- 生成的 MessagePack loader 不包含 Newtonsoft.Json imports。

如果本地 CI 暂时没有 .NET/Unity 构建环境，第一版不强制加入完整 C# 编译测试；但如果环境可用，加入编译测试是更好的。

## 延后工作

以下内容明确不在第一版 MessagePack 实现范围内：

- 加密；
- 完整性校验；
- 数字签名；
- 文件头或格式 envelope；
- manifest 文件；
- schema hash 校验；
- 压缩；
- 位置数组形式的 record；
- 自动安装 Unity package；
- C# 以外语言的运行时加载器。
