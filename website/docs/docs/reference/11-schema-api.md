# Schema API

Schema API 是 `coflow-cft` 编译后的公开语义模型，面向 loader、checker、codegen、LSP、编辑器和 Provider 集成。它不加载数据源，也不构建 DataModel。

## 编译边界

源码与编译结果分开持有：

- `CftModuleSet` 保存 module path、source、AST 和 parse diagnostics。
- `CftSchema` 是编译成功后返回的语义模型。
- schema 编译失败时不返回空 schema 或部分 schema。

```rust
let modules = parse_modules([
    CftFile::from_source(ModuleId::from("schema/main.cft"), source),
]);
let dimensions = CftDimensionInputs::default();
let schema = build_schema(&modules, &dimensions)?;
```

`CftModuleSet` 只提供 `module()`、`modules()` 和 `diagnostics()`。语言工具从这里读取原始 annotation、源码和 AST；语义消费者读取 `CftSchema`。

## 声明对象

`CftSchema` 直接提供以下声明对象：

| 对象 | 主要信息 |
| --- | --- |
| `CftType` | module、name、parent、字段、check、abstract/sealed/struct/singleton/idAsEnum、display 语义 |
| `CftField` | declaring type、name、已解析类型、默认值、expand、dimension、display、span |
| `CftEnum` | module、name、按声明顺序排列的 variants、flag、display 语义、span |
| `CftConst` | module、name、编译期值、span |
| `CftDimension` | name、按配置顺序排列的 variants、绑定字段 |
| `CftTopLevelCheck` | module、稳定 name、check block、静态 record-set dependencies、span |

名称身份使用 `TypeName`、`FieldName`、`EnumName`、`EnumVariantName`、`ConstName`、`DimensionName`、`VariantName`、`BucketName` 和 `RecordKey`。这些类型在构造和反序列化时都会验证名称。

## 查询

常用查询直接返回声明对象：

| API | 用途 |
| --- | --- |
| `resolve_type(name)` | 查找 `CftType` |
| `resolve_enum(name)` | 查找 `CftEnum` |
| `resolve_const(name)` | 查找 `CftConst` |
| `resolve_dimension(name)` | 查找 `CftDimension` |
| `resolve_check(name)` | 查找命名顶层 `CftTopLevelCheck` |
| `all_types()` / `all_enums()` / `all_consts()` / `all_dimensions()` / `all_checks()` | 按稳定名称顺序遍历声明 |
| `is_assignable(actual, expected)` | 沿 parent chain 判断可赋值关系 |
| `children(parent)` | 读取编译时建立的直接子类反向索引 |
| `type_for_id_as_enum(enum)` | 从 idAsEnum enum 反查 owner type |

主声明 map 同时提供名称索引。

## Type 与 Field

`CftType::own_fields()` 保持当前 type 的声明顺序；`CftType::all_fields()` 按父类型到子类型的顺序返回全部字段。

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
| `@label` / `@description` | type、field、enum 和 enum variant 的 `display: Option<CftDisplayMetadata>` |

Provider 不应扫描 annotation 字符串，也不应重复执行 annotation 语法校验。

`CftDisplayMetadata.label` 是面向人展示的别名，`description` 是详细说明；`summary()` 优先返回 description，否则返回 label。它们不改变声明名称或数据身份。

## Dimension

项目配置被规范化为 `CftDimensionInputs` 后参与 schema 编译。`CftField.dimension` 是字段到维度的正向绑定；`CftDimension.fields` 是编译时建立、共享同一字段对象的反向只读视图。

Dimension schema 不包含 `out_dir`、display name 或 Provider 选项；这些信息从项目配置读取。

## Check 语义

type-local 与命名顶层 check 共用 `CftSchemaCheckBlock`、statement/expression AST 和根 statement 的 dimension 元数据。`CftSchemaCheckStmt::Expr` 分别保存 condition 与可选 message；formatted string 保留 text/expression segments，nullable 与双 binding 量词使用独立 typed variant，不需要 consumer 反向解析源码。

每个 type-local 或命名顶层 check 的根 statement 都有 schema 生命周期内稳定的 `CheckStatementId`。`CheckStatementInfo` 暴露 owner、根索引、静态 `CheckDependency` 集合和相关 dimensions。常用查询包括：

- `check_statement(id)`：取得根 statement 及其 canonical 元数据；
- `check_statements_for_dependency(dependency)`：从字段或 record-set token 查询受影响 statement；
- `check_dependency_is_cross_record(statement, dependency)`：区分同一字段 token 的本地访问与经 record reference 的访问；
- `check_statements_for_actual_type(actual_type)`：取得某顶层实际类型需要执行的继承与内联 object statements；
- `check_statements_for_nested_field(actual_type, field)`：取得某个顶层宿主字段内可达的内联 object statements；
- `check_hosts_for_nested_type(type_name)`：取得可包含该内联 object 类型的顶层宿主类型。

`records(Type)` 编译为 `CheckDependency::RecordSet(Type)`；通过其 binding 访问字段时还会产生对应 `CheckDependency::Field`。调用方不需要重新遍历 check AST 或推导表达式类型。

`CftSchema::source(module)` 返回编译时保留的 canonical path/source catalog，用于把 check 的 `ModuleId + Span` 映射为诊断文件位置。host 不应根据 check 名称猜测文件路径。

## 使用边界

Schema API 适合读取编译后的类型、字段、枚举、常量、维度和 check 语义。加载 records、解析记录引用、执行项目检查和写入数据源应使用项目 runtime API。
