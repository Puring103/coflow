# Schema API

Schema API 是 `coflow-cft` 编译后的公开语义模型，面向 loader、checker、codegen、LSP、编辑器和 Provider 集成。它不加载数据源，也不构建 DataModel。

## 编译边界

源码与编译结果分开持有：

- `CftModuleSet` 保存 module path、source、AST 和 parse diagnostics。
- `CftSchema` 是编译成功后返回的语义模型。
- schema 编译失败时不返回空 schema 或部分 schema。
- 保留上一份成功 schema 是 runtime generation 的职责，不属于 `coflow-cft`。

```rust
let modules = parse_modules([
    CftFile::from_source(ModuleId::from("schema/main.cft"), source),
]);
let dimensions = CftDimensionInputs::default();
let schema = build_schema(&modules, &dimensions)?;
```

`CftModuleSet` 只提供 `module()`、`modules()` 和 `diagnostics()`。语言工具从这里读取原始 annotation、源码和 AST；语义消费者读取 `CftSchema`。

## Canonical 对象

`CftSchema` 直接提供以下声明对象：

| 对象 | 主要信息 |
| --- | --- |
| `CftType` | module、name、parent、字段、check、abstract/sealed/struct/singleton/idAsEnum 语义 |
| `CftField` | declaring type、name、已解析类型、默认值、expand、dimension、span |
| `CftEnum` | module、name、按声明顺序排列的 variants、flag 语义、span |
| `CftConst` | module、name、编译期值、span |
| `CftDimension` | name、按配置顺序排列的 variants、绑定字段 |

不存在公开 schema ID、field handle、generation token、reflection/meta 副本或 synthetic dimension type。

名称身份使用 `TypeName`、`FieldName`、`EnumName`、`EnumVariantName`、`ConstName`、`DimensionName`、`VariantName`、`BucketName` 和 `RecordKey`。这些类型在构造和反序列化时都会验证名称。

## 查询

常用查询直接返回 canonical 对象：

| API | 用途 |
| --- | --- |
| `resolve_type(name)` | 查找 `CftType` |
| `resolve_enum(name)` | 查找 `CftEnum` |
| `resolve_const(name)` | 查找 `CftConst` |
| `resolve_dimension(name)` | 查找 `CftDimension` |
| `all_types()` / `all_enums()` / `all_consts()` / `all_dimensions()` | 按稳定名称顺序遍历声明 |
| `is_assignable(actual, expected)` | 沿 parent chain 判断可赋值关系 |
| `children(parent)` | 读取编译时建立的直接子类反向索引 |
| `type_for_id_as_enum(enum)` | 从 idAsEnum enum 反查 owner type |

主声明 map 同时提供名称索引。

## Type 与 Field

`CftType::own_fields()` 保持当前 type 的声明顺序；`CftType::all_fields()` 保持根类型到当前类型的字段顺序。继承字段通过共享的 immutable `CftField` 保存，不复制完整字段对象。

`CftType::field(name)` 查询 type 的有效字段。消费者可以直接读取 `CftType` 的 `is_struct`、`is_singleton`、`id_as_enum` 等语义字段。

字段类型已经完成名称解析：

```text
Int / Float / Bool / String
Object(TypeName)
Enum(EnumName)
RecordRef(TypeName)
Array / Dict / Nullable
```

只有 `RecordRef(TypeName)` 表示 `&Type` 顶层记录引用；`Object(TypeName)` 表示内联对象。消费者不应重新解析类型字符串。

`default: Option<CftSchemaDefaultValue>` 同时表达“是否有默认值”和默认值内容；不存在重复的 `has_default` schema 字段。

## Annotation

原始 annotation 只存在于 AST。成功 schema 不公开通用 annotation bag，而是保存已经验证的明确语义：

| Annotation | 编译后语义 |
| --- | --- |
| `@struct` | `CftType.is_struct` |
| `@singleton` | `CftType.is_singleton` |
| `@idAsEnum` | `CftType.id_as_enum` |
| `@flag` | `CftEnum.is_flag` |
| `@expand` | `CftField.is_expand` |
| `@localized` / `@dimension` | `CftField.dimension` |

Provider 不应扫描 annotation 字符串，也不应重复执行 annotation 语法校验。

## Dimension

项目配置被规范化为 `CftDimensionInputs` 后参与 schema 编译。`CftField.dimension` 是字段到维度的正向绑定；`CftDimension.fields` 是编译时建立、共享同一字段对象的反向只读视图。

Dimension schema 不包含 `out_dir`、display name 或 Provider 选项，这些仍属于 project/runtime。它也不生成 storage type、storage field 或 runtime module。

## 执行计划

`CftSchema` 内部保留两个预编译执行计划：

- 内部 check 执行计划（实现类型为 `TypedCheckPlan`，不属于 public API）：继承 check 顺序、维度相关语句和可到达嵌套 check 的字段。
- `ValueDependencyPlan`：默认值、嵌套对象和物化依赖顺序。

执行计划只引用 canonical typed names 和 check 节点，不复制声明对象，也不出现在 schema inspect 或 wire 数据中。结构预算只作用于本次编译，不存入 schema。

check 内置函数的名称和参数数量由 `coflow-cft` 的共享 builtin contract 定义。schema 静态检查与 `coflow-checker` 运行期分发使用同一份 contract，避免两套注册表在新增或调整 builtin 时产生偏差。

## Runtime 关系

完整项目运行时继续负责：读取 `coflow.yaml`、发现 schema 文件、构造 `CftDimensionInputs`、加载普通 source 和维度 source、构建 record-owned overlay、解析引用并运行 checker。

宿主应使用当前 runtime generation 中的 `CftSchema`。失败的候选 generation 不替换上一份成功 generation。
