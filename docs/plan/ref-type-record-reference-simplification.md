# `&Type` 引用类型与记录引用简化计划

## 背景

当前 Coflow 的对象字段同时承载三种语义：

1. 内联对象：字段值是一个没有独立身份的对象，只属于父记录或父对象。
2. 记录引用：字段值指向某条顶层 record，当前可写 `&key` 或 `@Type.key`。
3. 路径引用：字段值从某条 record 的内部路径取值，例如 `@Item.sword.name`。

这让 schema、data model、writer、editor 和 codegen 都需要处理“同一个字段到底是值还是关系”的动态分支。路径引用还会引入 record 内部路径级依赖，但最终解析后又不保留为引用关系，导致源码依赖和模型依赖不一致。

本计划记录一次不考虑兼容性的破坏性简化：把引用变成 CFT 类型系统的一部分，并把数据引用限制为只能用 `&key` 指向顶层 record。

## 决策

1. 移除 `@ref` 和 `@inline`。
2. 使用 `&Type` 表示记录引用类型，不引入 `ref` 关键字。
3. 暂不支持同一字段既可内联又可引用的语义。
4. CFD 和表格单元格中只保留 `&key` 引用值。
5. 移除 `@Type.key`。
6. 完全移除路径引用，包括 `@Type.key.field`、`@Type.key[index]` 和 nested path spread。
7. `check {}` 块继续支持路径访问和引用对象字段访问。
8. spread 必须有外层期望类型，且 spread source 的实际类型必须是期望类型本身或其子类。
9. 同一个继承连通分量内，所有可实例化 type 的 record key 共享同一个命名域。
10. 不做兼容迁移；旧语法直接报错。
11. 借本次修改同步简化底层模型、编辑器元数据、writer 和 codegen 分支。

## 核心不变量

修改后模型只保留三类概念：

| 概念 | 身份 | 可被引用 | 语义 |
| --- | --- | --- | --- |
| record | 有 record key | 是 | 顶层共享实体 |
| inline object | 无 | 否 | 父对象拥有的值 |
| ref value | 无 | 否 | 指向 record 的关系边 |

关键规则：

- `&Type` 只表示 record-level relationship。
- `&T` 可以引用实际类型为 `T` 或 `T` 子类的 record；不能引用父类、兄弟类型或无继承关系的 record。
- inline object 永远不是引用目标。
- 数据输入中不存在“从某个 record 内部路径取值”的引用表达式。
- 数据输入中不存在带显式类型的引用字面量，`&key` 的目标类型完全来自 schema 上下文。
- 直接 `&T` 引用关系必须出现在 `ref_edges` 中；spread source 和 check 读依赖分别使用独立索引/图。
- `check` 表达式的路径访问是校验期求值能力，不是数据引用语法。
- 继承只应用于同一身份空间的实体族；不要用巨大 abstract base type 只为复用字段。

## 新旧方案对比

### CFT 字段类型

旧方案：

```cft
type Drop {
  item: Item;          # 默认可内联，也可引用
  @ref
  owner: Npc;          # 强制引用
  @inline
  stats: Stats;        # 强制内联
}
```

新方案：

```cft
type Drop {
  item: &Item;         # 只能引用 record
  owner: &Npc;         # 只能引用 record
  stats: Stats;        # 只能内联
}
```

字段形态由类型决定，不再由 annotation 决定。

### 类型语法

建议采用以下语义：

```cft
Item                 # inline object
&Item                # record reference
&Item?               # nullable record reference, equivalent to (&Item)?
[&Item]              # array of record references
{string: &Item}      # dict value is record reference
Item?                # nullable inline object
[Item]               # array of inline objects
{string: Item}       # dict value is inline object
```

限制：

- `&` 只能作用于 CFT type，不能作用于 primitive 或 enum。
- 不支持 `&int`、`&string`、`&Rarity`。
- 不支持 `&[Item]`、`&{string: Item}`；集合应写成 `[&Item]` 或 `{string: &Item}`。
- 暂不支持 `Item | &Item` 或任何 mixed object/reference 字段。
- `&Item?` 固定解释为 nullable reference，不解释为 reference to nullable object。

### CFD 引用语法

旧方案：

```cfd
item: &sword
item: @Item.sword
label: @Item.sword.name
reward: @DropTable.default.rewards[0]
weight: @DropTable.default.weights[Fire]
```

新方案：

```cfd
item: &sword
```

移除：

```cfd
@Type.key
@Type.key.field
@Type.key.array[0]
@Type.key.dict["name"]
@Type.key.dict[EnumValue]
```

`&key` 只能出现在期望类型为 `&T` 的位置。解析时在 `T` 所属继承命名域中查找 `key`，再检查命中的 record 实际类型是否为 `T` 或 `T` 的子类。

### 内联对象

旧方案中 `Item` 字段默认可写：

```cfd
item: @Item.sword
item: &sword
item: { name: "Inline Sword" }
```

新方案中二者由 schema 区分：

```cft
item: &Item;          # 只能写 &sword
inline_item: Item;    # 只能写 {...}
```

对应 CFD：

```cfd
item: &sword,
inline_item: { name: "Inline Sword" },
```

### 继承命名域

旧方案中 `@Item.sword` 和 `@Skill.sword` 可以通过显式类型区分。

新方案移除显式类型引用后，必须让继承域内 key 不歧义。规则定义为：

```text
同一个继承连通分量内，所有可实例化 type 的 record key 必须唯一。
```

例如：

```cft
abstract type Reward {}
sealed type ItemReward : Reward {}
sealed type CurrencyReward : Reward {}
```

`ItemReward.coin_01` 和 `CurrencyReward.coin_01` 不能同时存在。

无继承关系的 type 仍可复用 key：

```cft
type Item {}
type Skill {}
```

`Item.sword` 和 `Skill.sword` 可以共存，因为它们属于不同命名域。

### Spread

保留有外层期望类型的 whole-record / whole-object spread：

```cfd
elite: Monster {
  ...&base,
  level: 20,
}
```

这里 `elite` 的期望类型是 `Monster`，所以 `...&base` 在 `Monster` 的继承命名域中查找 `base`，且命中 record 的实际类型必须是 `Monster` 或 `Monster` 的子类。

移除 nested path spread：

```cfd
stats: {
  ...@Monster.base.stats
}
```

如果嵌套对象需要复用，应提升为具名 record，并通过 `&Type` 字段引用。

### Check 块

旧方案和新方案都保留 check 路径访问：

```cft
type Drop {
  item: &Item;
  rewards: [Reward];

  check {
    item.price > 0;
    all reward in rewards {
      reward.count > 0;
    }
  }
}
```

这里 `item.price` 是 check runner 对引用对象的字段访问，不是 CFD 数据里的路径引用。

## 目标底层模型

### Schema type

建议内部 schema type 增加引用构造：

```rust
pub enum CftSchemaTypeRef {
    Int,
    Float,
    Bool,
    String,
    Named(String),
    Ref(String),
    Array(Box<CftSchemaTypeRef>),
    Dict(Box<CftSchemaTypeRef>, Box<CftSchemaTypeRef>),
    Nullable(Box<CftSchemaTypeRef>),
}
```

AST 层可以保留更宽的 `Ref(Box<TypeRef>)` 以便给 `&int`、`&[Item]` 等非法输入报精确错误；编译后的 schema type 应收敛为 `Ref(String)`，直接表达“引用目标一定是 object type”这个不变量。编译校验阶段必须保证 `Ref` 指向非 enum、非 singleton 的 CFT object type。

### Input value

旧输入引用：

```rust
CfdInputValue::RecordRef {
    target_type: String,
    key: String,
}
```

新输入引用：

```rust
CfdInputValue::RecordRef(String)
```

`RecordRef` 只承载输入里写出的 key。目标类型来自外层 schema 上下文，解析结果写入 `RefEdge`。

删除：

```rust
CfdInputValue::PathRef
CfdRefPathSegment
CfdInputRefIndex
```

### Model value

旧模型引用：

```rust
CfdValue::Ref {
    target_type: String,
    target_key: String,
}
```

新模型引用：

```rust
CfdValue::Ref(String)
```

`CfdValue::Ref` 只保留目标 key。引用的 host、field path、期望类型、domain、目标 record id 都由 `RefEdge` 承载。这样 value tree 保持轻量，所有关系语义集中在引用索引中。

真实目标由 `ref_edges` 决定：

```text
RefSite(host_record, field_path) -> RefEdge

RefEdge {
  host: CfdRecordId,
  path: CfdPath,
  expected_type: TypeId,
  domain: DomainId,
  target_key: String,
  target: CfdRecordId,
}
```

展示、跳转、导出和 codegen 需要目标实际类型时，从 `target_record_id` 读取 record 的 `actual_type`。

inline object 不再复用完整 `CfdRecord`。建议拆成：

```rust
pub struct CfdRecord {
    pub key: String,
    pub object: CfdObject,
    pub origin: RecordOrigin,
}

pub struct CfdObject {
    pub actual_type: String,
    pub fields: BTreeMap<String, CfdValue>,
}

pub enum CfdValue {
    Object(Box<CfdObject>),
    Ref(String),
    // ...
}
```

这样 record 表示有身份的顶层实体，inline object 只表示父值拥有的对象。字段容器暂时继续使用 `BTreeMap<String, CfdValue>`，不在本次计划中改成 `Vec<CfdValue>`。如果编辑器 wire JSON 仍需要 `{ "target_key": "sword" }` 这类具名形状，可以在 DTO / serde adapter 层转换，核心模型不为 wire 形状保留单字段 struct variant。

spread 来源不再挂在 `CfdRecord` 或 `CfdObject` 上，统一由独立边索引承载：

```text
SpreadSite(host_record, object_path) -> SpreadEdge

SpreadEdge {
  host: CfdRecordId,
  path: CfdPath,
  expected_type: TypeId,
  domain: DomainId,
  source_key: String,
  source: CfdRecordId,
}
```

`path` 指向发生 spread 的 whole-record 或 inline object 位置，而不是 Graph 边。这样来源标注、rename 和写回可以共享同一结构，record/value tree 不需要携带 spread 专属元数据。

### Domain index

schema 编译后生成：

```rust
TypeId
DomainId
type_id_by_name: name -> TypeId
type_name: TypeId -> name
type_domain: TypeId -> DomainId
domain_members: DomainId -> Vec<TypeId>
ancestors_by_type: TypeId -> Vec<TypeId>
```

data model build 时生成：

```rust
records_by_type: TypeId -> Vec<CfdRecordId>
record_by_type_key: (TypeId, key) -> CfdRecordId
record_by_domain_key: (DomainId, key) -> CfdRecordId
ref_edges: Vec<RefEdge>
ref_by_site: RefSite -> RefEdgeId
ref_by_host: CfdRecordId -> Vec<RefEdgeId>
ref_by_target: CfdRecordId -> Vec<RefEdgeId>
spread_edges: Vec<SpreadEdge>
spread_by_site: SpreadSite -> SpreadEdgeId
spread_by_source: CfdRecordId -> Vec<SpreadEdgeId>
```

引用解析：

```text
expected type = &T
domain = type_domain(T)
candidate = record_by_domain_key(domain, key)
candidate.actual_type must be T or a subtype of T
```

这可以替代 per-base polymorphic key index。`ref_by_host` 服务图视图和按 record 遍历出边；`ref_by_target` 服务 rename 和目标删除检查，避免全量扫描所有引用点。`spread_edges` 不参与图显示，只服务 whole-record/object spread 的写回、rename 和来源标注；`spread_by_site` 则让编辑器按对象位置 O(1) 取来源信息。

## 模块级影响

### `coflow-cft`

目标：

- 在 CFT 类型系统中新增 `&Type` 引用类型。
- 删除 `@ref` / `@inline` annotation spec 和语义校验。
- 输出 schema API 时将引用作为类型结构，而不是 annotation。

需要删除或替换：

- `@ref` annotation spec。
- `@inline` annotation spec。
- `@ref` / `@inline` 互斥检查。
- `@ref` 与 `@expand` 冲突检查，替换为 `@expand` 不允许 `&T`。
- “字段类型是否包含 object 才允许 @ref/@inline”的校验。
- singleton 与 `@inline` / `@ref` 的特殊交互，替换为类型级规则。

### `coflow-data-model`

目标：

- 输入值不再有 path ref。
- 引用值不再携带 target type。
- draft 构建不再跨 record 取内部路径值。
- 继承连通分量 key 唯一性成为硬约束。
- 顶层 record 与 inline object 拆分，inline object 不再携带 key、origin 或 spread 写回元数据。
- 引用解析时直接生成 `RefEdge`，并维护 `ref_by_site`、`ref_by_host`、`ref_by_target` 三个索引视图。
- whole-record/object spread 生成独立 `SpreadEdge` / `spread_by_source`，但不进入直接引用图。

删除：

```rust
CfdInputValue::PathRef
CfdValueDraft::PathRef
CfdRefPathSegment
CfdInputRefIndex
path_ref_result_type
resolve_path_ref
path_record_draft
flatten_dict_draft_entries 为 path ref 服务的分支
```

验证规则收敛为：

| 期望类型 | 允许输入 |
| --- | --- |
| `&T` | `RecordRef(key)` |
| `T` object | inline object / object spread |
| primitive / enum | 对应标量 |
| array / dict / nullable | 递归按内层类型验证 |

`ref_edges` 应成为直接 `&T` 关系图的唯一真相。其他跨 record 关系不混入直接引用图：`spread_edges` 只描述 spread 展开和写回来源，checker dependency graph 只描述 check 求值读依赖。

### `coflow-loader-cfd`

目标：

- 只解析 `&key` 引用值。
- `@Type.key` 和路径引用直接作为语法错误。

保留：

```cfd
&key
```

移除：

```text
@Type.key
parse_ref_path
parse_ref_index
reference path field/index diagnostics
```

### `coflow-loader-table-core`

目标：

- 表格单元格引用语法与 CFD 一致。
- 删除 typed reference 和 path reference parser。

保留：

```text
&sword
```

移除：

```text
@Item.sword
@Item.sword.name
@Drop.default.rewards[0]
```

对于目标类型是 `string` 的单元格，`&sword` 可以继续作为普通字符串，或要求字符串显式加引号。建议维持当前“目标类型决定解析”的原则：只有目标类型是 `&T` 时才按引用解析。

### `coflow-engine`

目标：

- mutation 和 writer 调度不再维护 `FieldMode`。
- 默认值和 shape materialization 直接由 `CftSchemaTypeRef` 判断。
- 重命名引用只处理 `&key`。
- rename 通过 `ref_by_target` 找到结构化引用点，通过 `spread_by_source` 找到 spread source rewrite 点，不再全量扫描值树或源文本。

可删除或重写：

```text
FieldMode::Any
FieldMode::Ref
FieldMode::Inline
field_mode(...)
@inline field does not allow record refs
@ref field does not allow inline objects
```

替代为：

```text
is_ref_type(ty)
ref_expected_type_id(ty)
is_inline_object_type(ty)
```

mutation coercion 分支：

```text
&T -> 只接受 ref payload
T  -> 只接受 object payload
```

### Provider writers

受影响 provider：

- CFD writer。
- Excel writer。
- CSV/table-core writer。
- Lark writer。

简化目标：

- `RewriteRecordReferencesRequest` 不再需要覆盖 `@Type.old.path` 或 `@Type.old`。
- 只重写结构化引用点中的 `&old`。
- `WriteCellRequest.new_value` 中 `CfdValue::Ref` 就是唯一引用形态。
- 本地 provider 优先走 `ref_by_target` / `RefEdge` 精确写回引用点，并走 `spread_by_source` / `SpreadEdge` 精确写回 spread source；remote provider 可保留 source-level rewrite 兜底能力，但请求语义只包含 key 和结构化站点上下文。

### `coflow-checker`

保留 check 路径访问。

需要确保：

- `CfdValue::Ref` 在 check 中仍可通过 `ref_by_site` / `RefEdge` 解引用到目标 record。
- inline object 仍按当前嵌套路径执行 check。
- `&T?` 的 null access 规则不变。

### C# codegen

目标：

- 由 schema type 直接生成 ref loader 或 inline loader。
- 删除“同一个 object 字段 token 是 string 就当 ref，否则当 inline”的运行时分支。

旧逻辑：

```text
TypeName field -> JSON token string ? context.GetType(key) : Type.LoadInline(token)
```

新逻辑：

```text
&TypeName field -> read key and context.GetType(key)
TypeName field  -> Type.LoadInline(token)
```

### Schema inspect / schema API

目标：

- `&T` 作为类型结构输出，而不是作为 annotation 输出。

建议 JSON：

```json
{ "kind": "ref", "target": { "kind": "named", "name": "Item" } }
```

或在约束后简化为：

```json
{ "kind": "ref", "target": "Item" }
```

### LSP / VSCode 插件

目标：

- CFT 高亮和补全支持 `&Type`。
- 移除 `@ref` / `@inline` 补全。
- CFD semantic token 移除 typed reference 和 path segment。
- CFD 补全只补 `&key`。
- 跳转引用只跳到 record。

### CFD Editor

目标：

- 删除 field mode 元数据。
- UI 控件由 schema type 直接决定。

旧数据：

```text
FieldMode
FieldModeIndex
FieldAnnotation.field_mode
GraphData.field_modes
FileRecords.field_modes
```

新行为：

```text
&T         -> record picker
T          -> inline object editor
[&T]       -> ref list editor
[T]        -> inline array editor
{K: &T}    -> dict value record picker
{K: T}     -> dict value inline editor
```

Graph 视图只画 `RefEdge` 中的真实 record edge。
Graph 数据只消费直接引用索引（`ref_by_host` / `RefEdge`），不消费 `spread_edges`。`...&key` 只影响字段来源、写回和 rename，不在图中显示为边。

## 分阶段实施计划

每个阶段应能独立编译并有明确测试目标。阶段内可以更新必要的相邻测试，但不要临时保留旧语义来追求兼容。

### 阶段 1：CFT 类型系统与 schema API

目标：让 schema 能表达 `&T`，并删除 `@ref` / `@inline`。

范围：

1. CFT parser 支持 `&` 类型前缀。
2. `CftSchemaTypeRef` 增加引用类型。
3. compiler 校验 `&` 内层必须是 object type。
4. `&T?` 固定解释为 nullable reference。
5. 删除 `@ref` / `@inline` annotation spec。
6. `@expand` 只允许 inline object，不允许 `&T`。
7. schema inspect / schema API 输出 ref type。

阶段测试计划：

- `coflow-cft` parser/compiler tests：
  - `item: &Item;` 编译通过。
  - `backup: &Item? = null;` 编译通过并输出 nullable ref。
  - `items: [&Item];`、`map: {string: &Item};` 编译通过。
  - `bad: &int;`、`bad: &Rarity;` 报错。
  - `bad: &[Item];`、`bad: &{string: Item};` 报错。
  - `@ref` / `@inline` 报 unknown/deprecated annotation。
  - `@expand field: &Stats;` 报错。
- `schema inspect` tests：
  - ref type JSON 稳定输出。
  - nullable / array / dict 中的 ref type 结构正确。

阶段验收：

```cft
type Drop {
  item: &Item;
  backup: &Item? = null;
  stats: Stats;
}
```

能通过 schema 编译；旧 annotation 不能通过。

### 阶段 2：继承命名域与 record index

目标：建立继承连通分量级 record key 命名域，为 `&key` 无类型引用做基础。

范围：

1. schema/runtime 层计算 `DomainId`。
2. 同一继承连通分量内所有 concrete/plain/sealed records 的 key 必须唯一。
3. `records_by_type` 保留按实际类型访问。
4. 新增 `record_by_type_key` 和 `record_by_domain_key`。
5. schema 层建立 `TypeId`、`DomainId`、`type_domain`、`domain_members`、`ancestors_by_type`。
6. 替换原有 per-base polymorphic key index。

阶段测试计划：

- data model tests：
  - 无继承关系的 `Item.sword` 和 `Skill.sword` 可共存。
  - 同一 abstract base 下两个子类重复 key 报错。
  - 普通父类和子类重复 key 报错。
  - 多层继承链上重复 key 报错。
  - 同一 domain 中唯一 key 可通过 domain lookup 找到。
  - concrete `&Child` 期望类型命中 sibling record 时触发 assignability error。
  - `record_by_type_key` 只按实际类型命中 record。
  - `record_by_domain_key` 在继承连通分量内命中任一成员 record。
- diagnostics tests：
  - duplicate key diagnostic 指出 domain root 或相关 type。
  - duplicate key diagnostic 标出冲突 record。

阶段验收：

继承域 key 唯一成为 data model build 的硬约束，且完全无继承关系的 type 仍保持局部 key 空间。

### 阶段 3：Data model 引用值重构

目标：引用值只存 key，引用解析由期望类型和 domain index 决定。

范围：

1. `CfdInputValue::RecordRef` 删除 `target_type`。
2. `CfdValue::Ref` 删除 `target_type`。
3. `CfdInputValue::PathRef`、path segment、path index 全部删除。
4. `CfdRecord` 与 `CfdObject` 拆分，inline object 使用 `CfdObject`。
5. value validation：
   - `&T` 只接受 `RecordRef(key)`。
   - `T` 只接受 inline object / object spread。
6. ref resolution：
   - `expected &T + key -> domain lookup -> assignability check -> CfdRecordId`。
7. 解析时直接生成 `RefEdge`，包含 host、path、expected type、domain、target key 和 target id。
8. 维护 `ref_by_site`、`ref_by_host`、`ref_by_target`。
9. whole-record/object spread 维护独立 `SpreadEdge` / `spread_by_source`，不并入 `ref_edges`。

阶段测试计划：

- data model tests：
  - `&Item` 字段接受 `RecordRef("sword")`。
  - `&Item` 字段拒绝 inline object。
  - `Item` 字段接受 inline object。
  - `Item` 字段拒绝 record ref。
  - `&Reward` 可引用任一子类 record。
  - `&Reward` 可引用 `Reward` 自身的可实例化 record。
  - `&ItemReward` 拒绝 `CurrencyReward` record。
  - `&ItemReward` 拒绝父类 `Reward` record。
  - missing key 按 domain 报 `RefTargetNotFound`。
  - `ref_edges` 包含所有 `CfdValue::Ref`。
  - 每个 `CfdValue::Ref` 都能通过 `RefSite` resolve 到 `RefEdge`。
  - `ref_by_host` 能列出一个 record 的全部直接出边。
  - `ref_by_target` 能列出一个 record 的全部直接入边。
  - core model 中 `CfdValue::Ref` 和 `CfdInputValue::RecordRef` 不再使用单字段 struct variant。
  - wire/serde DTO 如需具名字段，由 adapter 层负责转换。
  - inline object 不携带 key、origin 或 spread 写回元数据。
  - spread source 进入 `spread_by_source`，但不进入 `ref_edges`。
- checker smoke tests：
  - check 中 `item.price` 可通过 `ref_by_site` 解引用。
  - nullable reference 的 null access 规则不变。

阶段验收：

底层模型中不存在 typed record ref 和 path ref。所有直接引用点都通过 `RefEdge` 解析，spread 与 check 读依赖使用独立索引/图。

### 阶段 4：CFD loader 与 table cell parser

目标：输入语法只保留 `&key`。

范围：

1. CFD loader 删除 `@Type.key` parser。
2. CFD loader 删除 path ref parser。
3. Table cell parser 删除 typed reference 和 path reference parser。
4. `&key` 只能在目标类型为 `&T` 时解析为 ref；目标类型为 string 时仍按字符串处理。
5. object spread source 支持 `...&key`，且必须由外层期望 object 类型提供上下文。

阶段测试计划：

- CFD loader tests：
  - `item: &sword` 在 `&Item` 字段中通过。
  - `item: @Item.sword` 报错。
  - `item: @Item.sword.name` 报错。
  - `item: &sword` 在 inline `Item` 字段中报 type mismatch。
  - `stats: { ...&base_stats }` 在期望 `Stats` 上下文中通过。
  - `reward: Reward { ...&item_reward }` 在期望 `Reward` 上下文中允许子类 spread source。
  - `stats: { ...&wrong_type }` 报 assignability error。
  - `item_reward: ItemReward { ...&reward_base }` 在期望 `ItemReward` 上下文中拒绝父类 spread source。
  - `item_reward: ItemReward { ...&currency_reward }` 在期望 `ItemReward` 上下文中拒绝兄弟类型 spread source。
  - 顶层或无期望类型位置的 `...&key` 报错。
- table-core cell tests：
  - `&sword` 在 `&Item` 列中解析为 ref。
  - `&sword` 在 `string` 列中保留字符串。
  - `@Item.sword` 在 `&Item` 列中报错。
  - path ref 输入报错。

阶段验收：

任何非 string 上下文中的 `@Type.key` / path ref 都不能进入 data model。

### 阶段 5：Engine mutation、写回和 rename

目标：写入路径完全由 schema type 驱动，不再维护 field mode，不再重写 typed/path ref。

范围：

1. 删除 engine mutation 中的 `FieldMode`。
2. mutation coercion 改为按 `&T` / `T` 分支。
3. default materialization：
   - `&T` 不自动生成假引用。
   - `&T?` 默认可为 null。
   - inline object 仍可按 editable shape 生成 UI 草稿。
4. record rename 通过 `ref_by_target` 更新指向目标 record 的结构化 `&old`。
5. provider writer 只写 `&key` 引用值。
6. `RewriteRecordReferencesRequest` 语义从 typed/path refs 收敛到 key refs。
7. record rename 通过 `spread_by_source` 更新 `...&old` spread source；这类关系不进入 Graph。
8. object 来源标注通过 `spread_by_site` 从对象路径取 `SpreadEdge`，不再从 `CfdRecord` 上读取 `spread_field_sources`。

阶段测试计划：

- engine mutation tests：
  - set `&Item` 字段为 `&sword` 成功。
  - set `&Item` 字段为 inline object 报错。
  - set `Item` 字段为 `&sword` 报错。
  - insert minimal 不为 required `&T` 伪造引用。
  - insert minimal 对 `&T? = null` 使用 null 或省略默认。
  - editable shape 对 inline object 生成结构，对 ref 字段生成 null/empty ref UI placeholder。
- writer tests：
  - CFD writer 写出 `&new_key`。
  - CSV/Excel/table writer 写出 `&new_key` 或 provider 约定的 key ref 文本。
  - rename record 更新所有 model ref sites。
  - rename record 更新所有 spread source sites。
  - 编辑器来源标注可通过 `spread_by_site` 定位对象 spread 来源。
  - rename 不触碰同名普通 string。
  - rename 不需要处理 `@Type.old` 或 `@Type.old.path`。
- remote writer tests：
  - rewrite request 不再携带 target type token rewrite 语义。
  - key 唯一/domain 上下文足以判断可改引用。

阶段验收：

record rename 和 field writes 只处理结构化 `&key` 引用点以及 whole-record/object spread source，旧 typed/path rewrite 分支删除。

### 阶段 6：Export 与 C# codegen

目标：导出和生成代码显式区分 `&T` 与 inline `T`。

范围：

1. JSON export 对 `&T` 输出引用 key。
2. JSON export 对 inline `T` 输出 object。
3. MessagePack export 做同样区分。
4. C# JSON loader：
   - `&T` 只读 key 并 `GetT(key)`。
   - `T` 只读 inline object。
5. C# MessagePack loader 同步拆分。
6. 删除 string-or-object object field 分支。

阶段测试计划：

- exporter tests：
  - `&Item` 导出为 key/ref payload。
  - `Item` 导出为 inline object。
  - `[&Item]`、`{string: &Item}` 导出正确。
  - 多态 `&Reward` 导出 key，并可由 load context resolve 到具体子类。
- C# codegen tests：
  - `&Item` 字段生成 reference loader。
  - `Item` 字段生成 inline loader。
  - 生成代码不包含 object 字段 string-or-object 分支。
  - JSON roundtrip。
  - MessagePack roundtrip。
  - 多态 inline object 仍要求 concrete type marker。

阶段验收：

生成代码和导出格式不再依赖“object 字段可能是 string ref 或 inline object”的运行时判断。

### 阶段 7：Editor、LSP 和 VSCode 插件

目标：删除 field mode 元数据和 typed/path reference UI。

范围：

1. Editor backend DTO 删除 `FieldMode`、`FieldModeIndex` 和 `field_modes`。
2. 前端组件由 schema type 决定控件。
3. Graph 只使用 `ref_by_host` / `RefEdge` 构图。
4. LSP 更新 `&Type` type syntax。
5. VSCode grammar/snippets 删除 `@ref`、`@inline`、`@Type.key` path semantics。
6. CFD completion 只补 `&key`。

阶段测试计划：

- editor backend tests：
  - `FileRecords` / `GraphData` 不包含 field modes。
  - graph edges 来自 `ref_by_host` / `RefEdge`。
  - graph 不显示 `...&key` spread source。
  - ref value 可定位 target file/type/key。
- frontend tests 或 build checks：
  - `&T` 字段渲染 record picker。
  - `T` 字段渲染 inline editor。
  - UI 不再出现 inline/ref 切换按钮。
  - path ref 不再有跳转入口。
- LSP / VSCode tests:
  - CFT `&Item` semantic token 正确。
  - CFD `&sword` completion 正常。
  - `@Type.key` 不再作为引用补全。
  - reference path segment token 测试删除或改写。

阶段验收：

编辑器 UI 和 LSP 行为完全由 schema type 驱动，不再传输 field mode 或处理 path ref。

### 阶段 8：文档、示例和全仓清理

目标：公开文档和 examples 只描述新语义。

范围：

1. 更新网站 reference docs。
2. 更新 examples。
3. 删除或重写旧 path ref 示例文件。
4. 更新 diagnostics code index。
5. 全仓搜索并清理旧术语。

阶段测试计划：

- docs/examples checks：
  - example projects `coflow check` 通过。
  - docs 中新 CFT/CFD 示例使用 `&Type` / `&key`。
  - 公开 reference 不再推荐 `@ref`、`@inline`、`@Type.key`。
- hygiene searches：

```powershell
rg "@ref|@inline|PathRef|CfdRefPathSegment|CfdInputRefIndex|reference path|路径引用|FieldMode"
rg "@[A-Za-z_][A-Za-z0-9_]*\\.[A-Za-z_][A-Za-z0-9_]*" examples website/docs/docs/reference
```

允许保留的位置应仅限历史计划文档或明确旧方案归档说明。

阶段验收：

用户面对的 docs、examples、snippets 和 diagnostics 都只呈现 `&Type` / `&key` 新模型。

## 性能与架构收益

### 构建性能

旧方案 path ref 需要：

```text
resolve root record -> walk draft field/index/dict path -> resolve nested value -> type check result
```

新方案 ref 只需要：

```text
expected type -> domain id -> (domain, key) -> record id -> assignability check
```

这更适合索引、缓存和批量解析。解析 `&key` 时直接生成 `RefEdge`，避免先 resolve 一次、再遍历整棵 value tree 构建引用索引。

Graph 构建也从扫描 record value tree 变为：

```text
start record ids -> ref_by_host -> target record ids
```

rename 从扫描所有引用点变为：

```text
target/source record id -> ref_by_target / spread_by_source -> rewrite sites
```

### 内存占用

可以删除或压缩：

```text
CfdInputValue::PathRef
CfdRefPathSegment
CfdInputRefIndex
CfdInputValue::RecordRef.target_type
CfdValue::Ref.target_type
per-base polymorphic key indexes
FieldMode metadata
```

大量类型名字符串可以替换为 `TypeId` / `DomainId`。`CfdRecord` 与 `CfdObject` 拆分后，inline object 不再携带 record key、origin、spread metadata 等顶层 record 专属字段。

### 增量更新

旧方案中修改 `Item.sword.name` 可能影响所有 `@Item.sword.name` 使用点。

新方案中：

- 修改普通字段只影响所在 record。
- 修改 record key 影响直接 refs 和 spread source。
- 修改 schema 继承关系重建 domain index 并检查 key collision。
- 修改被引用 record 的普通字段不需要重算普通引用方；如果 check 读取了该 record，仍按 checker dependency graph 重算读方。

增量 invalidation 可以按 record/domain 粒度实现，不需要数据输入层面的 path-ref dependency graph。直接引用、spread source 和 check 读依赖分别用 `ref_edges`、`spread_edges`、checker dependency graph 表达，避免一个索引承担多种语义。

### 编辑器性能

字段控件由 schema type 静态决定：

```text
&T -> picker
T  -> inline editor
```

不需要根据当前值形态切换 UI，不需要 typed/path ref 补全、跳转和依赖分析。

Graph 视图只消费 `ref_by_host` / `RefEdge`。它不扫描 `CfdValue`，也不显示 `...&key` spread source 或 check 读依赖。

### 代码生成性能

生成代码不再为每个 object 字段生成 string-or-object 分支。loader 的分支由 schema 静态决定，运行时代码更短。

## 风险和处理

### 继承命名域会变紧

同一继承连通分量内 key 全局唯一会增加命名成本。处理方式：

- 建模指南明确继承表达“同一可引用实体族”。
- 共享字段优先用组合/inline object，不要滥用巨大 abstract base type。

### schema 继承重构会影响数据 key

把两个原本无关的 type 合并到同一父类下，可能产生 key collision。这是预期的破坏性变更，应通过 data model diagnostic 明确报告。

### `&Item?` 的语法结合需要明确

固定定义为 nullable reference，即 `(&Item)?`。schema inspect / formatter 应稳定打印为 `&Item?`。

### Singleton 字段是否允许 `&Singleton`

建议第一阶段仍禁止 singleton type 出现在字段类型中，包括 `&Singleton`。这样保持当前 singleton 作为全局配置入口的语义，避免 codegen 增加特殊路径。

### Whole-record spread 是否保留

保留 `...&key`，但必须有外层期望 object 类型，且 source 实际类型必须是期望类型本身或其子类。禁止父类、兄弟类型、无继承关系类型和 nested path spread。

### Mixed inline/ref 是否未来支持

本次不支持。若未来确实需要，应作为显式 union 类型重新设计，而不是恢复默认 object 字段双形态。

## 最终检查

完成实现后，从仓库根目录运行：

```powershell
cargo check --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

同时运行文档和示例相关检查，确保公开 reference 不再描述旧语义。
