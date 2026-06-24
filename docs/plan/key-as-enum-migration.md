# `@IdAsEnum` 迁移计划

> 历史归档：本文记录的是旧的字段级 `@id/@ref/@IdAsEnum` 设计迁移方案，不代表当前实现。当前语义见 `docs/spec/01-cft.md`、`docs/spec/02-data-model.md`、`docs/spec/06-csharp-codegen.md`：记录 key 来自数据记录的保留 `id`，引用使用显式 `@Type.key`，类型级注解为 `@idAsEnum("EnumName")`。

把当前的 `@IdAsEnumValue`（type 级，仅生成 `<Type>Id.cs` 字符串常量类）
完全替换成字段级 `@IdAsEnum("EnumName")`，让 codegen 阶段生成真正的
C# `enum` 类型，并把对应的字段类型从 `string` 提升为该 enum 类型。

## 目标

```cft
@IdAsEnum("GeneId")
@id
id: string;
```

效果：
- schema 检查 / 数据加载 / JSON 导出 / `@ref` 解析全部仍然按 `string`
  处理；JSON 数据格式不变（仍写 `"Gene_Spore"` 这样的字符串）。
- C# codegen 阶段生成 `public enum GeneId { Gene_Spore, Gene_Mating, … }`，
  并把 `GeneConfig.Id` 的 C# 类型从 `string` 改为 `GeneId`。
- 任何 `@ref(GeneConfig)` 字段（包括 `string?`）的 C# 类型也跟随变成
  `GeneId` / `GeneId?`，做到端到端的强类型。
- `<EnumName>` 与对应 type 在同一命名空间。

## 设计要点

### 注解形态

- 注解名：`@IdAsEnum`
- 目标：字段（与 `@id` 同位置）
- 参数：单个字符串，作为 enum 的 C# 类型名
- 限制：
  - 必须配 `@id`（schema 规则：「`@IdAsEnum` 必须出现在 `@id` 字段上」）
  - 字段静态类型必须是 `string`（CFT 规则：「`@IdAsEnum` 字段类型必须为
    `string`」）
  - `EnumName` 必须是合法 C# 标识符
  - `EnumName` 不能与 schema 中已存在的 type / enum / const 同名，
    避免后续 codegen 端冲突。

### 对每个层的影响

| 层 | 改动 |
|---|---|
| CFT lexer/parser | 不改；OneString 注解已支持 |
| CFT compiler | `support.rs` 中 `AnnotationSpec` 替换 `IdAsEnumValue` 为 `IdAsEnum`（target=Field, args=OneString）；`compiler.rs` 加上述三条语义校验 |
| schema-api | `CftSchemaField.annotations` 透传不动，下游用 `find_annotation("IdAsEnum")` 取参数即可 |
| 数据模型 / cell parser / excel loader | 不改。`@IdAsEnum` 字段在数据层完全等价于普通 `string` |
| `@ref` 解析 | 不改。引用解析仍按 string id 进行 |
| JSON 导出 | 不改，输出 `"Gene_Spore"` 字符串 |
| C# codegen | 主要工作（见下） |
| pipeline | 删 `id_as_enum.rs`；改为 build 阶段先扫 schema 找出所有 `@IdAsEnum` 字段，按 `<table>` 收集 records 的 id 列表，作为附加输入喂给 codegen |
| `cargo run -- codegen csharp` 命令（无数据） | 仍生成 enum 占位（空变体集），编译期合法 |

### codegen 内部要改的位置

1. **`schema_view.rs`**
   - `FieldMeta` 加字段 `csharp_enum_override: Option<String>`，从
     `@IdAsEnum` 注解填入。
   - `SchemaView` 加方法 `field_csharp_enum_override(type_name, field_name)`，
     给 emit 用。
   - 加方法：给定 `@ref(Target)` 的字段，返回 Target 的 @id 字段是否带
     override；如果有，跟随返回 enum 名。

2. **`emit.rs::csharp_type` / `csharp_property_type`**
   - 字段是 `string`（含 `string?`）、且字段或其 ref-target 的 @id 带
     override 时，把 C# 类型从 `string` 替换为 `EnumName` / `EnumName?`。
   - 默认值 `""` 也得跟着调整：现在 `string` 字段有 `default = ""`；
     enum 字段在 C# 端默认是 `default(EnumName)`（变体 0）。`@id` 字段
     无 default、必填，无影响；`@ref` 的 nullable 字段 default null，
     无影响。

3. **`emit.rs::read_token_expr` / `read_messagepack_expr`**
   - `string` 字段如果 override 存在，读 token 字符串后还得 `Enum.Parse`。
     需要在生成代码里调用一个新的 helper：`ReadStringEnum<EnumName>(token, path)`
     和 messagepack 版本。helper 实现放进 database 模板，本质就是
     `(EnumName)Enum.Parse(typeof(EnumName), value)` 加个错误包装。

4. **`names.rs`**
   - 加 `annotation_string_arg(annotations, name) -> Option<String>`，与现
     有 `annotation_name_arg` 对偶。

5. **`ir.rs::build_project`**
   - 现在 `enums` 来自 `schema.all_enums()`。在此基础上把 `@IdAsEnum`
     字段的 enum 也合并进 `enums`：变体集合由数据驱动，`build_project`
     需要接受额外参数 `id_as_enum_variants: BTreeMap<String, Vec<String>>`，
     形成 `CsharpEnum`（无 flag、无 display、变体值用插入顺序的整数）。

6. **新增模板（可选）**
   - 不必新增。所有 `@IdAsEnum` 生成的 enum 与普通 enum 走同一个
     emit 路径即可。

7. **collision 检查**
   - 多个表用相同 `@IdAsEnum("Foo")`：合并变体（如果完全一致），不一致
     时报 codegen 错误。`Foo` 名和已有 enum / type 冲突时报错。

### pipeline 改动

- `coflow-pipeline/src/id_as_enum.rs` 全删。
- `coflow-pipeline/src/lib.rs::build_project`：build 阶段在 codegen 调用
  之前，扫描 `schema.all_types()` 找带 `@IdAsEnum` 的字段，从
  `model.records_of_type(type)` 收集 id 字符串集合（已经构建好的
  CfdValue::String），构造 `id_as_enum_variants` map 传给
  `generate_csharp_*`。
- `coflow-pipeline/src/lib.rs::generate_project_code`：纯 codegen 路径
  没有数据，直接传空 map。生成的 enum 空变体集合，C# 编译合法。

### 测试矩阵

- `coflow-cft`：`@IdAsEnum("GeneId")` 配 `@id: string` 通过；缺 `@id`
  报错；字段类型非 string 报错；EnumName 非法 ident 报错；EnumName 与
  现有 type/enum 冲突报错。
- `coflow-codegen-csharp`：构造一个含 `@IdAsEnum("GeneId")` 的 schema +
  注入变体集合 + `@ref(GeneConfig) parent: string?` 字段，断言生成的 C#：
  - `enum GeneId { ... }` 被生成
  - `GeneConfig.Id` 类型为 `GeneId`
  - 引用方字段类型为 `GeneId?`
- `coflow-pipeline`：humanpark 例子端到端跑通，生成 6 个真 enum 文件。

### 落地步骤（按改动顺序）

1. CFT 注解层（`support.rs` + `compiler.rs`）：替换注解定义、加三条
   校验、补 cft 测试。**这是用户预期的"先确认方向"步骤；先单独提交**。
2. codegen `schema_view.rs` 加 override 字段、传播到 ref 字段。
3. codegen `emit.rs` 把 `string` 翻译位置全部接上 override；C# helper
   `ReadStringEnum` 加进 JSON / messagepack database 模板。
4. codegen `ir.rs` 接受 `id_as_enum_variants` 参数，合并成 `CsharpEnum`。
5. pipeline 删 `id_as_enum.rs`，build 时收集变体集合喂给 codegen。
6. humanpark example 替换为 `@IdAsEnum("...")` 形式：
   - `GeneConfig` -> `@IdAsEnum("GeneId")`
   - `SkinConfig` -> `@IdAsEnum("SkinId")`
   - `PhaseConfig` -> `@IdAsEnum("PhaseId")`
   - `AbilityConfig` -> `@IdAsEnum("AbilityId")`
   - `SubstanceConfig` -> `@IdAsEnum("SubstanceId")`
   - `BioRemainsConfig.geneId` 是 `@ref(GeneConfig)`，C# 类型自动跟着
     变成 `GeneId`。
7. 全 workspace `cargo test` + clippy 全绿；两个 example check + build。
8. 删除/重写 `coflow-cft/tests/new_annotations.rs` 中关于 `IdAsEnumValue`
   的旧用例。
9. 更新 `docs/spec/06-csharp-codegen.md` 描述 `@IdAsEnum` 行为。

### 不在本次范围

- `@IdAsEnum` 不试图反过来影响 schema 类型系统（即 cft 端 `id` 还是
  `string`，type checker 仍按 `string` 处理）。
- 不支持把 `@IdAsEnum` 用在非 @id 字段（避免无主索引情况下的语义不清）。
- 不试图自动从 enum 变体里推回 cft enum 类型（这就是用户原方案的反向，
  没必要）。

## 风险与权衡

- **变体来自数据**：codegen 输出依赖运行时数据，意味着 `codegen csharp`
  命令在没有数据时拿到的 enum 是空的；C# 编译合法但运行时谁也用不上。
  `build` 路径才有完整变体。这点要写清楚到 spec 06。
- **collision 难发现**：两个表用同名 `@IdAsEnum("Foo")` 但变体集合不同，
  build 时才能 catch；可在 CFT compiler 阶段加一条「同名 @IdAsEnum 必须
  来自同一个表」的硬约束作为兜底。
- **变体顺序**：`Enum.Parse` 不依赖整数值，所以变体定义顺序对运行时没影响；
  但为了生成稳定输出，应按 records 出现顺序赋值，重复时去重。
