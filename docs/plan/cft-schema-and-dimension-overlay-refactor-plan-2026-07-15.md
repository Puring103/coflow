# CFT Schema 与维度字段 Overlay 整体重构迁移计划 - 2026-07-15

## 文档状态

- 状态：已完成；迁移、文档同步、完整 release gate、最终独立审查、分批提交和 PR 更新均已完成。
- 范围：`coflow-cft` 整个 crate、canonical schema、维度 schema、DataModel overlay，以及直接依赖这些结构的 runtime/provider/checker/export/codegen/editor/LSP。
- 迁移策略：允许破坏内部 Rust API，旧 reflection/meta、synthetic dimension type/record 和兼容别名最终全部删除。
- 语言语义：不修改 CFT 语法、annotation 可用范围、继承、多态、默认值、check、记录引用、维度文件格式和导出格式。
- 兼容性决定：同时移除内置 Lark provider、远程 `url` source 和 URI source location；这是已确认的用户可见 breaking change，本地 Excel/CSV/CFD source 不受影响。
- 本计划替代此前所有 ID-first、generation、独立 dimension store 和 synthetic storage 方案。

## 最终设计结论

本次重构采用以下不可再并列保留旧方案的结论：

1. `CftModuleSet` 是源码、路径、AST 和 parse diagnostics 的唯一所有者。
2. 删除 `CftContainer`；编译是无状态函数，不在 `coflow-cft` 内保存“上一次成功 schema”。
3. 删除 `CftSchemaModule`；编译后的顶层声明直接保存 `ModuleId + Span`，不再建立第二套 module 对象。
4. `CftSchema` 是唯一编译后 semantic model，删除 `SchemaReflection` 和全部 `*Meta` 副本。
5. 外部直接读取 `CftType`、`CftField`、`CftEnum`、`CftConst` 和 `CftDimension`，不经过公开 ID 或 handle。
6. semantic identity 使用 validated typed names，不使用裸 `String`。
7. 不引入 `TypeId`、`FieldId`、`DimensionId`、schema generation token 或 arena。
8. 不引入 `CftFieldRef`、`CftFieldLocation`、`derived` 聚合层或新的 representation 概念。
9. annotation 只存在于 AST；编译成功后只保留 annotation 已有的具体语义，不公开通用 annotation bag。
10. 自身字段和继承展开字段共享同一个 immutable `CftField`，不复制完整字段对象。
11. 查询索引在编译时构建，但只保留确有反向查询价值的索引和对象内部位置表。
12. dimension 是 schema 中的一等对象，不生成 synthetic storage type。
13. variant value 直接存入 owner `CfdRecord`，不生成 synthetic record，也不建立独立长期 dimension store。
14. 普通 source field 是 default 的唯一语义值；物理 dimension 文件中的 `default` 只是 provider 管理的镜像。

## 实施结果

截至 2026-07-15，核心代码已按本计划完成迁移：

- `CftModuleSet` 单独持有 source/path/AST/parse diagnostics，`CftSchema` 不再复制 module source。
- 删除 `CftContainer`、`CftSchemaModule`、`SchemaReflection`、全部 `*Meta`、public empty schema 和重复 field/name 查询。
- canonical type/field/enum/const/dimension 使用 validated typed names；wire 反序列化重新执行名称校验。
- 成功 check IR 中已解析的字段访问和类型谓词使用 `FieldName` / `TypeName`；局部 binding、内建方法名和多命名空间语法标识符继续使用 `String`。
- 继承字段共享同一 `Arc<CftField>`；type 内只保留一个 `field_by_name` 位置表。
- `children_by_parent` 使用有序 `Vec<TypeName>`；schema 只保留 children 和 idAsEnum owner 两个跨对象反向索引。
- annotation 仅保留在 AST，成功 schema 只公开 `is_struct/is_singleton/id_as_enum/is_flag/is_expand/dimension` 等明确语义。
- dimension-sensitive check blocks 已从 `CftType` 移入 `TypedCheckPlan`，声明对象不保存执行期派生副本。
- 删除 synthetic dimension storage type/record/runtime module 和独立 dimension store；variant value 直接附着 owner record overlay。
- CSV/CFD Provider 直接加载和写入维度值，并保留 CSV cell/CFD span origin。
- dimension refs 进入预构建正反向索引、rename rewrite、checker、增量影响和 transaction publication。
- owner record rename/delete 会在同一 transaction 内更新 managed dimension 行并保留 rename 前的 variant 值。
- mutation/editor 使用强类型稳定坐标和 expected-state stale-write 防护；undo/redo/coalescing/rollback 共用同一坐标。
- `data patch` 继续使用 `coordinate.record.{type,key}` 既有 JSON 形态，只在命令边界转换为内部强类型坐标。
- `dimensions/synthesize.rs` 已改为只负责 source discovery 的 `dimensions/sources.rs`。

实施提交按 module、typed names、canonical schema、record overlay、mutation/index、crate 边界和最终清理分批完成。用户工作区中的 `examples/**` 改动不属于本迁移提交。

最终独立审查确认旧 container/module/reflection/meta、schema ID/handle、synthetic dimension storage 和独立 dimension store 均无残留。完整 release gate 已通过：skill reference 同步与校验、workspace check、fmt check、全目标 clippy 和 workspace tests。

## 目标架构

```text
CftModuleSet
  source/path/AST/parse diagnostics
        |
        v
compile_schema(modules, dimensions, options)
        |
        v
CftSchema
  unique compiled declarations
  precomputed local/reverse indexes
  typed check/value dependency plans
        |
        v
coflow-runtime
  ordinary source batches + direct dimension batches
        |
        v
CfdDataModel
  ordinary records with record-owned dimension overlays
```

## 涉及范围

- `coflow-cft`：module、AST、compiler、唯一 schema、查询 API、执行计划和结构预算测试。
- `coflow-project`：dimension 配置校验、来源诊断和规范化编译输入。
- `coflow-api`：dimension source 的同步、加载和写入契约。
- `coflow-loader-csv`、`coflow-loader-cfd`、`coflow-loader-table-core`：直接解析维度值。
- `coflow-data-model`：record-owned overlay、类型校验、引用、来源和遍历。
- `coflow-checker`：default/variant round、依赖和诊断定位。
- `coflow-runtime`：source resolution、load、cache、mutation、增量失效和 publication。
- JSON、MessagePack、C# codegen：保持既有外部产物契约。
- CFD Editor、CLI data patch、LSP、schema inspect：迁移查询和稳定坐标。
- 网站 reference、示例、skill references 和测试。

## 非目标

- 不修改 `.cft` 语法。
- 不新增 annotation。
- 不修改 `@struct`、`@singleton`、`@idAsEnum`、`@flag`、`@expand`、`@localized` 或 `@dimension` 的既有含义。
- 不修改 `&Type` 是顶层记录引用、普通 `Type` 是内联对象的规则。
- 不修改 check 的表达式语义和执行顺序。
- 不修改 dimension CSV/CFD 的用户可见结构。
- 不修改 JSON、MessagePack 和 C# 的用户可见格式。
- 不把 project 的 `display_name`、`out_dir` 或 provider 配置放入 `CftSchema`。
- 不为未来可能出现的开放 annotation/plugin 机制预留 generic semantic bag。

## 必须保持的行为

| 行为 | 迁移后要求 |
| --- | --- |
| module parse failure | 保留 source/path/diagnostics，`ast = None`。 |
| 全局命名空间 | type/enum/const 名称继续全项目唯一。 |
| 字段顺序 | `own_fields` 保持声明顺序，`all_fields` 保持父到子顺序。 |
| 引用 | 只有 `RecordRef(TypeName)` 表示 `&Type`。 |
| 默认值 | 仍由 schema 编译，DataModel 不重解析 CFT 源码。 |
| annotation | 语法对象留在 AST，schema 只保留编译后的具体含义。 |
| `@idAsEnum` | enum lock、codegen 和输出行为保持不变。 |
| dimension missing | 缺失 variant 不回退 default。 |
| dimension null | 显式 null 保持 checker skip 语义。 |
| nested dimension value | 完整对象/数组/字典子树继续执行嵌套检查。 |
| physical origin | 维度诊断仍定位到 CSV cell 或 CFD span。 |
| incremental check | 维度变更归一化到 owner record，并与 full check 等价。 |
| mutation | 普通值和维度值继续进入同一 transaction/publication 生命周期。 |

## `coflow-cft` 最终模型

### 单一 Module 表示

```rust
pub struct CftModuleSet {
    modules: BTreeMap<ModuleId, CftModule>,
    diagnostics: CftDiagnostics,
}

pub struct CftModule {
    pub path: PathBuf,
    pub source: String,
    pub ast: Option<ModuleAst>,
}
```

规则：

- 一个 `ModuleId` 只对应一个 `CftModule`。
- duplicate module 在插入 map 前产生诊断。
- parse success 保存 `Some(ast)`；parse failure 保存 `None`。
- compiler 只读取 `CftModuleSet`，不复制 source/path/AST。
- LSP 直接使用 module source/AST，不重新 parse。
- `CftModuleSet` 不保存成功 schema。

公开入口收敛为：

```rust
pub fn module(&self, id: &ModuleId) -> Option<&CftModule>;
pub fn modules(&self) -> impl Iterator<Item = (&ModuleId, &CftModule)>;
pub fn diagnostics(&self) -> &CftDiagnostics;
```

删除 `file/files` 与 `module/modules` 双 API。

### 无状态编译入口

```rust
pub fn compile_schema(
    modules: &CftModuleSet,
    dimensions: &CftDimensionInputs,
    options: CftCompileOptions,
) -> Result<CftSchema, CftDiagnostics>;
```

约束：

- 每次调用独立构建一个 immutable `CftSchema`。
- parse diagnostics 存在时拒绝 semantic publication。
- compiler pass 失败时不返回部分 schema。
- “保留上一份成功 schema”属于 runtime session publication，不属于 `coflow-cft`。
- `CftCompileOptions.structural_limits` 只影响当前 build，不存入 `CftSchema`。

### 强类型名称

```rust
pub struct TypeName(/* private */);
pub struct FieldName(/* private */);
pub struct EnumName(/* private */);
pub struct EnumVariantName(/* private */);
pub struct ConstName(/* private */);
pub struct DimensionName(/* private */);
pub struct VariantName(/* private */);
pub struct BucketName(/* private */);
pub struct RecordKey(/* private */);
```

规则：

- 通过 parser lowering、`TryFrom<&str>` 或受控构造器创建。
- 构造时执行各自的 identifier、保留字或 record key 校验。
- 对外提供 `as_str()`、`Display`、比较和 hash 能力。
- wire 反序列化必须重新校验，不能绕过 invariant。
- 不使用一个通用 `Name(String)` 混合不同名称空间。
- 底层选择 `String`、`Box<str>` 或 `Arc<str>` 属于实现细节，按 profiling 决定。

允许继续使用裸 `String` 的内容：module source、注释、CFT string literal、用户 string value、诊断消息和 formatter 结果。

禁止裸 `String` 的身份：type、field、enum、variant、const、dimension、bucket、record key，以及稳定 mutation/cache/diagnostic 坐标。

### 唯一 Canonical Schema

```rust
pub struct CftSchema {
    types: BTreeMap<TypeName, CftType>,
    enums: BTreeMap<EnumName, CftEnum>,
    consts: BTreeMap<ConstName, CftConst>,
    dimensions: BTreeMap<DimensionName, CftDimension>,

    children_by_parent: BTreeMap<TypeName, Vec<TypeName>>,
    type_by_id_as_enum: BTreeMap<EnumName, TypeName>,

    typed_checks: TypedCheckPlan,
    value_dependencies: ValueDependencyPlan,
}
```

`CftSchema` 不保存：

- module source/path/AST；
- `CftSchemaModule`；
- `SchemaReflection`；
- `CftTypeMeta` 等查询副本；
- structural limits；
- synthetic extension types；
- schema declaration ID 或 generation token。

type/enum/const 等顶层声明直接包含 `ModuleId + Span`。field/enum variant 保存自身 `Span`，并通过 declaring owner 获取 `ModuleId`，避免重复保存 module。

### Type 与 Field

```rust
pub struct CftType {
    pub module: ModuleId,
    pub name: TypeName,
    pub parent: Option<TypeName>,

    own_fields: Vec<Arc<CftField>>,
    all_fields: Vec<Arc<CftField>>,
    field_by_name: BTreeMap<FieldName, usize>,

    pub is_abstract: bool,
    pub is_sealed: bool,
    pub is_struct: bool,
    pub is_singleton: bool,
    pub id_as_enum: Option<EnumName>,
    pub check: Option<CftSchemaCheckBlock>,
    pub span: Span,
}

pub struct CftField {
    pub declaring_type: TypeName,
    pub name: FieldName,
    pub ty: CftSchemaTypeRef,
    pub default: Option<CftSchemaDefaultValue>,
    pub is_expand: bool,
    pub dimension: Option<CftFieldDimension>,
    pub span: Span,
}
```

所有权和查询规则：

- 每个完整 `CftField` 只构造一次。
- `own_fields` 与 `all_fields` 只共享 `Arc<CftField>`。
- `own_fields` 保持当前 type 的声明顺序。
- `all_fields` 保持根 type 到当前 type 的字段顺序。
- 子类继承字段指向父类的同一个 `CftField`。
- `field_by_name` 是 type 内部的预构建位置表，不是第二套字段模型。
- `field_by_name` 只索引 `all_fields`，使 loader/checker/codegen 的常用查询直接命中。
- `CftField.declaring_type + span` 保留字段真实声明来源；module 从 declaring `CftType` 获取。

删除：

- 完整对象形式的重复 `all_fields`；
- `fields: BTreeMap<String, CftSchemaTypeRef>` 副本；
- `raw_type/ty` 双存储；
- `CftFieldRef`；
- `CftFieldLocation`；
- `declaring_type + field_index` 间接 handle；
- `FieldId` arena。

### 字段类型

保留现有 `CftSchemaTypeRef` 公共概念，但完成名称解析并消除 `Named(String)` 歧义：

```rust
pub enum CftSchemaTypeRef {
    Int,
    Float,
    Bool,
    String,
    Object(TypeName),
    Enum(EnumName),
    RecordRef(TypeName),
    Array(Box<CftSchemaTypeRef>),
    Dict(Box<CftSchemaTypeRef>, Box<CftSchemaTypeRef>),
    Nullable(Box<CftSchemaTypeRef>),
}
```

规则：

- `Object(TypeName)` 表示普通 `Type` 内联对象。
- `Enum(EnumName)` 表示 enum value。
- `RecordRef(TypeName)` 表示 `&Type` 顶层记录引用。
- compiler-only checked type 可以暂存 unresolved/unknown 状态。
- 成功 `CftSchema` 不允许 unresolved name、unknown 或 namespace-only type。
- 删除永久保存的原始类型字符串；展示统一使用 formatter。

### Enum 与 Const

```rust
pub struct CftEnum {
    pub module: ModuleId,
    pub name: EnumName,
    pub variants: Vec<CftEnumVariant>,
    variant_by_name: BTreeMap<EnumVariantName, usize>,
    variant_by_value: BTreeMap<i64, usize>,
    pub is_flag: bool,
    pub span: Span,
}

pub struct CftEnumVariant {
    pub name: EnumVariantName,
    pub value: i64,
    pub span: Span,
}

pub struct CftConst {
    pub module: ModuleId,
    pub name: ConstName,
    pub value: CftConstValue,
    pub span: Span,
}
```

规则：

- variants 的 `Vec` 保持声明顺序。
- `variant_by_name` 和 `variant_by_value` 在编译时构建。
- index 只保存位置，不复制 variant 对象。
- flag 组合值可以继续返回“无显式 variant name”的临时查询结果。
- 删除 `CftEnumMeta`、`CftEnumVariantMeta` 和 enum value map 副本。

### Annotation 编译边界

annotation 是语法，不是编译后公开模型。

AST 继续保存原始 annotation name、arguments 和 span，用于 parser/compiler diagnostics、LSP、formatter 和源码编辑。成功 schema 只保存已验证的现有含义：

| CFT annotation | 编译后字段 |
| --- | --- |
| `@struct` | `CftType.is_struct` |
| `@singleton` | `CftType.is_singleton` |
| `@idAsEnum(EnumName)` | `CftType.id_as_enum` |
| `@flag` | `CftEnum.is_flag` |
| `@expand` | `CftField.is_expand` |
| `@localized` / `@localized(bucket)` | `CftField.dimension`，dimension 为 `language` |
| `@dimension(name)` | `CftField.dimension` |
| `@__coflow_dimension_storage` | 删除，不再进入成功 schema |

删除编译后：

- `CftAnnotation`；
- `CftAnnotationValue`；
- type/field/enum/variant 的 `annotations` 字段；
- `annotation_exists`、`annotation_name_arg`；
- consumer 中按 annotation 字符串判断的查询。

新增 annotation 时必须显式扩展 AST validation 和对应 semantic 字段；不能恢复 generic annotation bag。

### 查询索引原则

主声明 map 本身已经是名称索引，不再重复构建 `type_by_name`、`enum_by_name`、`const_by_name` 或 `dimension_by_name`。

对象内部已经拥有的查询：

- `CftType.field_by_name`；
- `CftEnum.variant_by_name`；
- `CftEnum.variant_by_value`；
- `CftDimension.variant_by_name`。

Schema 只保留两个跨对象反向索引：

```rust
children_by_parent: BTreeMap<TypeName, Vec<TypeName>>,
type_by_id_as_enum: BTreeMap<EnumName, TypeName>,
```

理由：

- parent 正向关系已在 `CftType.parent`，只有 parent 到 children 需要反向表。
- type 到 enum 正向关系已在 `CftType.id_as_enum`，只有 enum 到 type 需要反向表。
- field 到 dimension 已在 `CftField.dimension`。
- dimension 到 fields 已在 `CftDimension.fields`。
- enum 到 variants 已在 `CftEnum.variants`。

`is_assignable(actual, expected)` 沿已验证 parent chain 查询，不在调用时构建新 map。若 profiling 证明该路径是热点，再增加单一 closure index；第一阶段不同时保存 ancestors、children、descendants 和 concrete types 四套关系。

具体 record 的继承命名域属于 `coflow-data-model` 的 record domain index，不放入 `coflow-cft`。

### 查询 API

```rust
pub fn resolve_type(&self, name: &str) -> Option<&CftType>;
pub fn resolve_enum(&self, name: &str) -> Option<&CftEnum>;
pub fn resolve_const(&self, name: &str) -> Option<&CftConst>;
pub fn resolve_dimension(&self, name: &str) -> Option<&CftDimension>;

pub fn all_types(&self) -> impl Iterator<Item = &CftType>;
pub fn all_enums(&self) -> impl Iterator<Item = &CftEnum>;
pub fn all_consts(&self) -> impl Iterator<Item = &CftConst>;
pub fn all_dimensions(&self) -> impl Iterator<Item = &CftDimension>;

pub fn is_assignable(&self, actual: &str, expected: &str) -> bool;
pub fn children(&self, parent: &TypeName) -> &[TypeName];
pub fn id_as_enum_owner(&self, enum_name: &EnumName) -> Option<&CftType>;
```

```rust
impl CftType {
    pub fn own_fields(&self) -> impl Iterator<Item = &CftField>;
    pub fn all_fields(&self) -> impl Iterator<Item = &CftField>;
    pub fn field(&self, name: &str) -> Option<&CftField>;
}
```

这些 `&str` 参数只是无所有权的查找 adapter：底层 map key 和返回声明仍是 typed name，不保存裸字符串身份；无效名称只会查找失败。稳定坐标和 wire 反序列化仍必须构造并验证 typed name。不得为同一查询再保留 reflection/meta 或 typed/string 两组同义 API。

### 执行计划

保留现有两个执行计划及其职责，不重命名、不扩展为新的 schema 层：

- `TypedCheckPlan`：预编译每个实际 type 的 check 执行顺序、继承顺序和 dimension-sensitive statements。
- `ValueDependencyPlan`：预编译默认值、嵌套值和检查所需的依赖/物化顺序。

两者：

- 由 compiler 在成功 publication 前构建；
- 只引用 canonical typed names、字段语义和 check 节点；
- 不复制 `CftType` 或 `CftField`；
- 不作为 wire/schema inspect 数据；
- 结构预算来自当前 `CftCompileOptions`。

### Compiler Pass

```text
1. collect_symbols
2. validate_annotations
3. resolve_enums_and_consts
4. validate_type_headers
5. resolve_field_types
6. validate_inheritance
7. validate_defaults
8. type_check_checks
9. lower_canonical_declarations
10. bind_dimensions
11. build_local_and_reverse_indexes
12. compile_typed_checks
13. compile_value_dependencies
14. publish CftSchema
```

compiler-only state 不 public re-export，也不进入成功 schema。所有 pass 成功后才构造最终 publication。

### Empty Schema

删除 public `CftSchema::empty()`。调用方使用 `Option<Arc<CftSchema>>` 或显式错误状态表达“尚无成功 schema”。不允许 empty schema 成为第二个 mutable build 入口。

## `coflow-cft` Dimension 模型

### 编译输入

```rust
pub struct CftDimensionInputs {
    dimensions: BTreeMap<DimensionName, CftDimensionInput>,
}

pub struct CftDimensionInput {
    pub variants: Vec<VariantName>,
}
```

`coflow-project` 负责：

- dimension/variant 名称合法性；
- duplicate、空列表和保留 variant `default`；
- `display_name`、`out_dir` 和 provider 配置；
- config source location 和 `DIM-CONFIG-*` 诊断。

`coflow-cft` 只接收已规范化输入，并校验字段 binding 是否引用已配置 dimension。

### 字段绑定与 Dimension

```rust
pub struct CftFieldDimension {
    pub dimension: DimensionName,
    pub bucket: Option<BucketName>,
}

pub struct CftDimension {
    pub name: DimensionName,
    pub variants: Vec<VariantName>,
    variant_by_name: BTreeMap<VariantName, usize>,
    pub fields: Vec<Arc<CftField>>,
}
```

规范化：

```text
@localized          -> dimension = language, bucket = None
@localized("ui")    -> dimension = language, bucket = ui
@dimension("theme") -> dimension = theme, bucket = None
```

规则：

- `CftField.dimension` 是正向权威 binding。
- `CftDimension.fields` 是编译时生成的反向只读视图，只共享 field 对象。
- variants 保持 project 配置顺序。
- `variant_by_name` 支持直接存在性查询。
- owner type、field type、singleton 和 span 直接从 `CftField/CftType` 查询；module 由 declaring type 提供。
- child type 的 `all_fields` 自然包含父类 dimension field。
- 不生成 storage type、storage field、hidden annotation 或 runtime module。

## `coflow-data-model` 最终模型

### Record-Owned Overlay

```rust
pub struct CfdRecord {
    pub actual_type: TypeName,
    pub key: RecordKey,
    fields: BTreeMap<FieldName, CfdValue>,
    dimension_fields: BTreeMap<FieldName, CfdDimensionFieldValues>,
}

pub struct CfdDimensionFieldValues {
    pub dimension: DimensionName,
    pub variants: BTreeMap<VariantName, CfdDimensionValue>,
}

pub struct CfdDimensionValue {
    pub value: CfdValue,
    pub origin: RecordOrigin,
}
```

所有权规则：

- `record.fields[field]` 是 default 的唯一语义值。
- `record.dimension_fields[field]` 只保存额外 variants。
- 普通字段不创建空 overlay。
- dimension field 的 default 不复制到 overlay。
- 删除 record 时 overlay 自动删除。
- clone/publication record 时 overlay 与 record 同生命周期。
- rename record 不需要维护第二套 dimension owner map。
- model build 完成后释放 provider dimension batch。
- 不存在 `CfdDimensionStore` 或其他与 records 平行的长期值容器。

允许 DataModel 内部继续使用 `CfdRecordId`、table index 和 reference edge ID；这些是当前 model generation 的记录索引，不是 schema declaration ID。

### Missing、Null 与查询

variant map 中没有 key 表示 missing；存在且 `value == CfdValue::Null` 表示 explicit null。两者不得合并。

```rust
pub enum DimensionValueLookup<'a> {
    Value {
        value: &'a CfdValue,
        origin: &'a RecordOrigin,
    },
    ExplicitNull {
        origin: &'a RecordOrigin,
    },
    Missing,
}
```

查询输入使用 `FieldName + DimensionName + VariantName`，并验证：

- field 属于 record actual type 的 `all_fields`；
- field 的 dimension 与请求一致；
- variant 已配置；
- overlay 中保存的 dimension 与 field binding 一致。

### 引用和遍历

统一 record value walker 按顺序遍历：

1. 普通 `fields`；
2. `dimension_fields[*].variants`。

维度值中的 `RecordRef` 必须进入：

- ref resolution；
- `ref_by_site`；
- `ref_by_host`；
- `ref_by_target`；
- rename/delete rewrite；
- structural depth/node/work budget；
- exporter/codegen value traversal。

## Provider 契约

### Direct Dimension Load

Provider schema context 直接借用编译后对象：

```rust
pub struct DimensionSourceSchema<'a> {
    pub dimension: &'a CftDimension,
    pub source_type: &'a CftType,
    pub source_field: &'a CftField,
}

pub struct CfdInputDimensionValue {
    pub source_type: TypeName,
    pub source_key: RecordKey,
    pub field: FieldName,
    pub dimension: DimensionName,
    pub variant: VariantName,
    pub value: CfdInputValue,
    pub origin: RecordOrigin,
}
```

Provider 不接收 synthesized type name，不解析 annotation，不从字符串重建 field type。

### Physical Default Mirror

CSV 继续保持：

```text
id | default | <variant 1> | ... | <variant N>
```

规则：

- `id` 是 source record key。
- `default` 由普通 source record 刷新。
- `default` 不加载为 dimension value。
- variant cell 按 `source_field.ty` 解析。
- sync 增删/排序配置 variants，同时保留用户已有 variant value。
- 未配置列按现有 policy 保留或诊断，但不进入 model。
- read-only session 不写文件。
- build session 先 sync，再读取同步后的文件。

singleton 继续使用现有 CFD 物理形式，但不再伪装成 synthetic CFT record。

### Write

```rust
pub struct DimensionValueCoordinate {
    pub actual_type: TypeName,
    pub record_key: RecordKey,
    pub field: FieldName,
    pub dimension: DimensionName,
    pub variant: VariantName,
}
```

该稳定坐标用于 mutation、patch、editor、diagnostic 和 cache impact。dimension write 加入现有 preflight、transaction enlistment、staging、compensation、commit 和 publication 流程。

## Runtime Pipeline

```text
discover/parse CFT modules
  -> compile_schema
  -> resolve ordinary sources
  -> load ordinary records
  -> build intent 下 sync dimension files
  -> resolve dimension sources
  -> direct load dimension batches
  -> locate owner records
  -> validate against CftField.ty and CftField.dimension
  -> attach to CfdRecord.dimension_fields
  -> release batches
  -> resolve refs across defaults and variants
  -> run default/variant checks
  -> publish immutable runtime session
```

Source resolution 继续是唯一负责 provider 选择、options decode、目录展开和 target override 的模块。`ResolvedDimensionSource` 只补充稳定 dimension/field 坐标并复用已有 `ResolvedSource`，不重复保存 provider identity。

Cache key 使用：provider/options、规范化路径、`DimensionName`、`TypeName`、`FieldName`、variant 列表和 field type fingerprint。不得保存已删除的 schema ID 或 synthetic type name。

## Checker 迁移

Checker 从 source record 构建逻辑轮次：

- default round：读取 `record.fields`，执行完整继承 check schedule。
- variant round：仅覆盖当前 dimension 的已绑定字段，其他字段继续读取 source record。
- explicit null：跳过依赖该字段的 expression/method/nested check，保持现有语义。
- missing：产生既有 missing diagnostic，不回退 default。
- dimension-independent statement：只在 default round 执行一次。
- inherited dimension field：child record 通过 `CftType.all_fields` 读取同一个父字段。
- nested object/array/dict：variant 完整子树继续执行 nested type checks。

逻辑位置是 source record field path；物理位置是 dimension cell origin。删除 synthetic storage record cursor 和 storage-record skip 分支。

维度读取依赖归一化到 owner source record。维度值内部引用其他 record 时，继续记录目标 record dependency。full/incremental diagnostics 必须差分等价。

## Export 与 Codegen

- JSON exporter 从同一个 record 的 default + overlay variants 组合输出。
- MessagePack 使用同一逻辑。
- C# `Localized<T>` 或现有 wrapper 输出保持不变。
- codegen 直接读取 `CftType/CftField/CftEnum/CftDimension`。
- `@idAsEnum` 使用 `CftType.id_as_enum` 和 reverse index。
- `@flag` 使用 `CftEnum.is_flag`。
- `@struct` 使用 `CftType.is_struct`。
- `@expand` 使用 `CftField.is_expand`。
- exporter/codegen 不遍历 hidden type/record。

所有已有 JSON、MessagePack、C# golden 必须逐项保持。

## Mutation、Patch 与 Editor

新增/保留专用 dimension mutation，不再把它伪装成 generated record field mutation：

```text
SetDimensionValue {
  coordinate: DimensionValueCoordinate,
  expected,
  value
}
```

要求：

- stale-write 防护继续使用 expected value。
- source record rename/delete 同时处理 overlay 和 dimension physical rows。
- ref rewrite 遍历 default 与 variants。
- patch JSON 使用稳定名称坐标。
- 旧 synthetic type addressing 返回明确诊断，不保留 fallback。
- editor read DTO 展示 source record + field + selected variant。
- optimistic update、undo/redo、coalescing 和 rollback 使用同一稳定坐标。
- file tree 显示 managed dimension files，但不显示 synthetic rows/graph nodes。

## LSP 与 Schema Inspect

- LSP 的 source、annotation、原始拼写和 AST 导航来自 `CftModuleSet`。
- 编译后类型、字段、默认值、继承和 dimension 语义来自 `CftSchema`。
- LSP 不依赖 `SchemaReflection`。
- schema inspect 不输出原始 annotation bag，而输出 `is_struct/is_singleton/id_as_enum/is_flag/is_expand/dimension` 等编译结果。
- schema inspect type count 只包含用户声明 type。
- 不暴露 synthetic storage type、runtime module 或 generated annotation。

## `coflow-cft` 文件结构

```text
src/
  lib.rs
  diagnostics/
    mod.rs
    codes.rs
  syntax/
    mod.rs
    ast.rs
    span.rs
    identifier.rs
    lexer/
    parser/
  module/
    mod.rs
    module_id.rs
    module_set.rs
  schema/
    mod.rs
    names.rs
    type_ref.rs
    declarations.rs
    dimensions.rs
    queries.rs
    compiler/
      mod.rs
      symbols.rs
      annotations.rs
      enums.rs
      types.rs
      inheritance.rs
      defaults.rs
      checks.rs
      checked_type.rs
      budget.rs
      lower.rs
    plans/
      typed_checks.rs
      value_dependencies.rs
```

职责：

- `syntax` 只处理 token、AST 和 syntax diagnostics。
- `module` 只处理输入 module collection 和 parse publication。
- `schema/declarations` 定义唯一公开 semantic objects。
- `schema/compiler` 只保存短生命周期 compiler state。
- `schema/plans` 保存 checker/materialization 执行计划。
- `queries` 只围绕唯一对象模型提供读取，不创建第二套 view。
- 删除并吸收当前 `schema.rs` / `cft_schema.rs` 双模型职责。

## 迁移阶段

### 阶段 0：行为固化

1. 盘点全部 `coflow-cft` public API 和 workspace consumer。
2. 固化 module parse success/failure/duplicate 行为。
3. 固化 type/field/enum/const、字段顺序、inheritance 和 assignability。
4. 固化 annotation validation 与 semantic query。
5. 固化 structural budget、cycle priority 和 failed publication。
6. 固化 dimension missing/null/nested/ref/singleton/multiple-dimension 行为。
7. 建立 JSON、MessagePack、C# golden。
8. 建立 full/incremental checker differential baseline。
9. 记录迁移前 schema type count 和 model record count。

退出条件：现有行为均有可执行 characterization tests，尚未修改生产结构。

### 阶段 1：统一 Module，删除 Container

1. 合并 `CftModuleFile` 与 `ParsedCftModule` 为 `CftModule`。
2. `CftModuleSet` 只保留单一 module map。
3. 建立无状态 `compile_schema`。
4. 迁移 runtime/LSP 调用方分别持有 module set 和成功 schema。
5. 删除 `CftContainer` 和 container stateful compile API。
6. 删除 file/module 双查询。

退出条件：module source/path/AST 只存一份；compile failure 不污染 runtime 已发布 schema。

### 阶段 2：强类型名称

1. 引入最小 typed names 集合。
2. 迁移 type/field/enum/variant/const/dimension/bucket/record key identity。
3. 增加构造和反序列化校验测试。
4. 边界 adapter 将 `&str` 立即转换为 typed name。
5. 不修改 string literal、diagnostic message 和用户 string value。

退出条件：semantic identity 不再使用裸 `String`，没有 schema ID/generation 方案。

### 阶段 3：建立唯一声明对象

1. 将 `CftSchemaType` 收敛为 `CftType`。
2. 将 `CftSchemaField` 收敛为 `CftField`。
3. 收敛 enum、variant 和 const 对象。
4. 让最终 `CftSchema` 直接拥有声明 map。
5. 顶层声明保存 `ModuleId + Span`；field/variant 保存自身 span 并引用 declaring owner。
6. 删除 `CftSchemaModule` 和 schema module map。
7. 删除 `SchemaReflection`。
8. 删除 `CftTypeMeta/CftFieldMeta/CftEnumMeta/CftEnumVariantMeta`。
9. 旧 API 临时 adapter 只能借用 canonical object，禁止 clone 完整声明。
10. consumer 迁移后立即删除 adapter。

退出条件：workspace 只有一套编译后 type/field/enum/const 对象。

### 阶段 4：字段共享与类型解析

1. 每个 own field 构造一个 `Arc<CftField>`。
2. `all_fields` 共享父类和自身 field。
3. 构建 type-local `field_by_name`。
4. 保持父到子的字段顺序。
5. 将 `Named/Ref(String)` lower 为 `Object/Enum/RecordRef` typed names。
6. 删除 raw type 字符串副本。
7. 迁移 loader/checker/codegen 到直接 `CftField`。

退出条件：继承不复制完整字段，只有 `RecordRef` 表示记录引用。

### 阶段 5：Annotation Semantic Lowering

1. AST annotation 保持不变。
2. compiler validation 后填充现有具体 semantic 字段。
3. 迁移所有 annotation consumer。
4. 构建 `type_by_id_as_enum` reverse index。
5. 删除编译后 annotation bag 和字符串扫描 helper。
6. 删除 hidden dimension storage annotation 支持。

退出条件：成功 schema 不含通用 annotation；schema inspect 输出具体语义。

### 阶段 6：查询与 Compiler 收敛

1. 只保留主声明 map、对象局部位置表和两个跨对象反向索引。
2. 删除重复 names/metas/resolve/has/field query 组合。
3. consumer 使用唯一对象 API。
4. 明确 compiler passes 和唯一 publication 入口。
5. 删除 public empty schema。
6. 恢复完整 budget test seam。
7. 稳定模型后再执行文件移动。

退出条件：查询不创建临时 map，不存在 reflection/meta 双轨。

### 阶段 7：一等 Dimension Schema

1. project 产生规范化 `CftDimensionInputs`。
2. compiler lower `@localized/@dimension` 到 `CftField.dimension`。
3. 构建 `CftDimension`、variant 位置表和 fields 反向视图。
4. runtime dimension discovery 改用 schema 对象。
5. 不再扫描 annotation。
6. 暂时只允许由 canonical dimension descriptor 派生 legacy adapter；它不能独立修改或发布。

退出条件：dimension/variant/field/bucket 只有一套 schema 权威。

### 阶段 8：Provider Direct Load

1. 增加 direct dimension load DTO/trait。
2. table core value parser 接收 `&CftField` 或 `&CftSchemaTypeRef`。
3. 实现 CSV direct load。
4. 实现 CFD direct load。
5. 保持 physical default mirror sync。
6. 增加 provider conformance tests。

退出条件：provider 不需要 synthetic type 即可产生 typed dimension values。

### 阶段 9：Record-Owned Overlay

1. `CfdRecord` 增加 `dimension_fields`。
2. 普通 `fields` 保持 default 唯一权威。
3. direct batch 定位并附着 owner record。
4. 区分 missing 与 explicit null。
5. 接入 origin、refs、reverse refs、budgets 和 traversal。
6. model publication 后释放 batch。
7. 删除独立/平行 dimension value store 设计。

退出条件：variant value 唯一存于 owner record overlay。

### 阶段 10：Checker、Output 与增量

1. checker 从 record overlay 构造 variant round。
2. 删除 synthetic record cursor/skip/dependency。
3. 更新 incremental impact 到 owner record。
4. 迁移 JSON、MessagePack、C#。
5. 对比 full/incremental diagnostics 和 output golden。

退出条件：检查和输出不依赖 synthetic record，外部格式不变。

### 阶段 11：Mutation、Editor 与 Runtime Publication

1. 接入专用 dimension mutation/write。
2. 接入 transaction/staging/compensation/publication。
3. 迁移 rename/delete/ref rewrite。
4. 迁移 patch 和 editor 稳定坐标。
5. 迁移 cache/watcher impact。
6. 删除 graph/file/table 中 synthetic nodes。

退出条件：普通值与维度值混合写保持原子，editor 不使用 generated coordinate。

### 阶段 12：删除 Synthetic 路径

1. 停止生成 storage type/field/record。
2. 删除 `with_extension_types`。
3. 删除 storage type indexes、helpers 和 naming fallback。
4. 删除 runtime synthesize module。
5. 删除 legacy adapter、fixture、diagnostic 和测试。
6. repo scan 确认 production/active tests 无旧符号。

退出条件：schema type count 只含用户 type，model record count 只含用户 record。

### 阶段 13：文档与 Release 准备

更新：

- `website/docs/docs/reference/03-language/01-cft.md`；
- `website/docs/docs/reference/05-data-model.md`；
- `website/docs/docs/reference/10-localization.md`；
- `website/docs/docs/reference/11-schema-api.md`；
- `website/docs/docs/reference/12-architecture.md`；
- provider、diagnostics、pipeline references；
- examples 和 dimension fixtures。

release/packaging 时执行 skill reference sync 并提交同步文件。

## 最终删除清单

### `coflow-cft`

- `CftContainer`；
- `CftModuleFile`；
- `ParsedCftModule`；
- `CftSchemaModule`；
- `SchemaReflection`；
- `CftTypeMeta`；
- `CftFieldMeta`；
- `CftEnumMeta`；
- `CftEnumVariantMeta`；
- 编译后的 `CftAnnotation` / `CftAnnotationValue`；
- 完整对象副本形式的 `all_fields`；
- `raw_type`；
- `Named(String)` / `Ref(String)`；
- `CftFieldRef` / `CftFieldLocation` / `field_index` handle；
- schema declaration IDs、arenas 和 generation token 草案；
- `Dimension::Localized` / `Dimension::Custom`；
- `DimensionSpec` 和 storage metadata；
- `dimensions::add_dimension_storage`；
- `CftSchema::with_extension_types`；
- `dimension_storage_types` 及全部 helper；
- `@__coflow_dimension_storage` semantic 支持；
- `__runtime__` dimension module；
- `__coflow_dimension_*` naming/collision logic；
- reflection/meta 重复查询；
- public `CftSchema::empty()`。

### Runtime / DataModel / Checker / Host

- `dimensions/synthesize.rs`；
- synthesized type/record/source indexes；
- `MissingStorageRecord`；
- storage record lookup 和 backing record ID；
- synthetic records 参与普通 iteration；
- 从 dimension storage record 加载 default；
- checker storage-record cursor 和 skip；
- synthetic dependency assertions；
- patch/editor generated type addressing；
- exporter/codegen generated record traversal；
- 描述 runtime type injection 的公开文档。

## 测试矩阵

### Module 与 Compiler

- 单一 source/path/AST 存储；
- parse success/failure/duplicate；
- stateless compile；
- compile failure 不发布半成品；
- runtime 保留上一次成功 session；
- syntax/schema/check/dependency structural budgets；
- inheritance cycle 与 depth diagnostic priority。

### Schema Objects

- typed name validation 和反序列化；
- type/field/enum/const 直接查询；
- module/span 来源；
- own/all field 顺序；
- inherited field `Arc` identity 相同；
- field lookup 位置表正确；
- parent/children；
- idAsEnum 正向/反向；
- enum name/value lookup；
- `Object/Enum/RecordRef` lowering；
- schema 无 reflection/meta/annotation bag/synthetic type。

### Annotation

- struct/singleton/idAsEnum/flag/expand/localized/dimension lowering；
- invalid target/arg/duplicate/unknown diagnostics；
- AST 保留 annotation span；
- LSP annotation hover/definition 行为；
- schema inspect 只输出具体 semantic fields。

### Dimension Schema

- localized/custom dimension；
- bucket；
- inherited field on child；
- singleton；
- configured but unused dimension；
- binding missing config；
- empty/duplicate/reserved/invalid variants；
- multiple invalid config diagnostics；
- variant order；
- schema inspect 无 hidden type。

### DataModel

- primitive/enum/ref/nullable/array/dict/object dimension value；
- missing 与 explicit null；
- duplicate coordinate；
- unknown record/field/dimension/variant；
- inherited field；
- nested subtree；
- reverse refs 和 rename rewrite；
- structural budgets；
- CSV cell/CFD span origin；
- record clone/delete/rename 与 overlay 同生命周期。

### Provider / Runtime

- CSV/CFD direct load；
- default mirror refresh；
- variant value preservation；
- variant add/remove/reorder；
- source record insert/delete/rename；
- stale managed file；
- read-only 不写；
- build sync 后 load；
- cache hit/targeted reload/full fallback；
- failed transaction 不发布；
- watcher attribution。

### Checker / Output / Host

- variant pass/fail；
- explicit-null skip；
- missing diagnostic；
- non-dimension check 只执行 default round；
- inherited check；
- nested check；
- dependency 到 owner record；
- full/incremental differential；
- JSON/MessagePack deterministic golden；
- C# compile/load；
- patch set dimension；
- editor read/write/undo/redo/rollback；
- LSP completion/hover/diagnostics/type list；
- file tree/graph 无 synthetic record。

## 建议提交序列

1. `test: characterize cft schema and dimension behavior`
2. `refactor: unify cft module storage and remove container`
3. `refactor: introduce typed schema names`
4. `refactor: establish canonical cft declarations`
5. `refactor: share inherited cft fields`
6. `refactor: resolve cft field type relations`
7. `refactor: lower annotations into schema semantics`
8. `refactor: remove schema reflection and meta views`
9. `refactor: simplify cft queries and compiler phases`
10. `refactor: model dimensions directly in cft schema`
11. `feat: load dimension sources without storage types`
12. `feat: attach dimension overlays to cfd records`
13. `refactor: check dimension values from record overlays`
14. `refactor: export dimension values from record overlays`
15. `feat: add direct dimension mutations`
16. `refactor: migrate editor and runtime dimension workflows`
17. `refactor: remove synthetic dimension types and records`
18. `docs: document canonical schema and dimension overlays`

临时 adapter 必须在明确提交中引入和删除。最终不留 deprecated alias、dual authority 或 dead path。

## 验证要求

普通开发提交从仓库根目录执行：

```powershell
cargo check --workspace
cargo test --workspace
```

不把 fmt/clippy 变成普通开发 gate。

release/packaging 提交按 `AGENTS.md` 执行：

```powershell
pwsh scripts/sync-skill-references.ps1
pwsh scripts/sync-skill-references.ps1 -Check
cargo check --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

最终 repo scan：

```powershell
rg "CftContainer|CftSchemaModule|SchemaReflection" crates src editors tests
rg "CftTypeMeta|CftFieldMeta|CftEnumMeta|CftEnumVariantMeta" crates src editors tests
rg "CftAnnotation|CftAnnotationValue" crates/coflow-cft/src
rg "CftFieldRef|CftFieldLocation|TypeId|FieldId|SchemaGenerationId" crates/coflow-cft/src
rg "__coflow_dimension_storage|__coflow_dimension_" crates src editors tests
rg "dimension_storage_type|is_dimension_storage_type|with_extension_types" crates src editors tests
rg "synthesized_type|MissingStorageRecord" crates src editors tests
```

AST annotation 类型若仍使用不同的语法层名称，应确保 scan 只匹配已删除的编译后类型，不误删 AST。

## 完成验收条件

1. 每个 module 只保存一份 source/path/AST。
2. `CftContainer` 和 `CftSchemaModule` 已删除。
3. compile 是无状态函数，failed build 不发布 schema。
4. `CftSchema` 是唯一编译后 semantic model。
5. 外部直接查询 `CftType/CftField/CftEnum/CftConst/CftDimension`。
6. semantic identity 使用 typed names，不使用裸 `String`。
7. 不存在 schema declaration ID、arena 或 generation token。
8. 每个完整 `CftField` 只构造一次，继承只共享字段对象。
9. 字段和 enum variant 顺序保持。
10. 只有必要的局部位置表和两个跨对象反向索引。
11. 最终字段类型区分 `Object/Enum/RecordRef`。
12. 编译后 schema 不含 generic annotation bag。
13. annotation 既有行为全部由具体 semantic 字段表达。
14. typed checks 和 value dependencies 只引用唯一 schema 对象。
15. structural limits 不存入 schema。
16. dimension/variant/field binding 是一等 schema 数据。
17. 不生成或暴露 dimension storage type。
18. 每个 record 直接拥有 dimension overlay。
19. default 只有普通 source field 一个语义所有者。
20. physical default mirror 不进入 DataModel。
21. missing/explicit-null 行为保持。
22. refs、checks、diagnostics、mutations 覆盖 dimension variants。
23. incremental/full diagnostics 等价。
24. JSON、MessagePack 和 C# output 保持。
25. editor/LSP/schema inspect 不暴露 synthetic 对象。
26. 最终删除清单无 production/active-test 残留。
27. 公开文档描述最终架构。
28. `cargo check --workspace` 和 `cargo test --workspace` 通过。

## 主要风险与缓解

### 字段顺序变化

风险：map 化或继承重建可能改变 loader/codegen/export 顺序。

缓解：声明使用 `Vec` 保序，位置表只保存 index；阶段 0 固化多层继承顺序 golden。

### `Arc<CftField>` 引入意外对象身份判断

风险：consumer 使用 pointer identity 代替 `declaring_type + FieldName` 语义。

缓解：`Arc` 只解决共享所有权；公开 equality、diagnostic 和稳定坐标继续使用 typed names。增加继承共享测试，但禁止 wire/cache 保存指针身份。

### Annotation consumer 漏迁移

风险：codegen/provider/LSP 仍按字符串扫描 annotation。

缓解：先盘点全部 annotation consumer，逐项迁移到具体字段，再删除编译后 annotation 类型并用 repo scan 封口。

### LSP 依赖 Reflection

风险：LSP 既需要原始 AST，又需要编译后语义。

缓解：原始内容统一来自 `CftModuleSet`，语义统一来自 `CftSchema`；不以保留 reflection 解决边界问题。

### Assignability 查询性能

风险：沿 parent chain 查询在极大继承图中成为热点。

缓解：先保留简单、清晰的 parent traversal；使用 benchmark/profiling 证明后再增加单一 closure index，不预先复制四套继承关系。

### Dimension 反向视图双权威

风险：`CftField.dimension` 与 `CftDimension.fields` 不一致。

缓解：两者在一次 compiler publication 中构造，之后 immutable；添加双向一致性 invariant test，不提供增量 mutation API。

### 临时双模型

风险：迁移期 reflection/meta 或 synthetic/overlay 同时可写。

缓解：adapter 只能从当前权威只读派生；任何阶段只能有一个 publication authority；adapter 不保存独立副本。

### Physical origin 丢失

风险：删除 synthetic record 后诊断落到默认 source field。

缓解：每个 variant value 强制携带 `RecordOrigin`，端到端测试 CSV cell 和 CFD span。

### Reference rewrite 漏掉 overlay

风险：walker 只遍历普通 fields。

缓解：统一 record value walker，并在删除 synthetic path 前覆盖 edge construction、rename/delete 和 mutation planning。

### 回滚

- module/schema 对象重构可通过代码回滚，不涉及用户数据格式。
- record-owned overlay 合并前必须具备 direct/legacy fixture 等价测试。
- synthetic 删除是不可逆代码清理点，只在所有 consumer/output 完成迁移后合并。
- physical dimension 文件格式始终保持，因此回滚不能丢失用户 variant cell。
