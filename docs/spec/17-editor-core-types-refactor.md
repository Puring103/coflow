# 编辑器接入核心类型与索引重构

**依赖文档**：[02-data-model.md](02-data-model.md)、[02-schema-api.md](02-schema-api.md)、[07-project-pipeline.md](07-project-pipeline.md)、[10-diagnostics.md](10-diagnostics.md)、[16-dimensions-refactor.md](16-dimensions-refactor.md)

本文档定义把 `cfd-editor` 后端与前端**全面接入核心库权威类型**的重构。重构后：

- 编辑器后端不再持有"按 key 全局查"的索引，记录的对外身份统一为 `(actual_type, key)` 稳定坐标
- 前端 `bindings/index.ts` 由核心库类型自动生成，不再手写
- 通用的索引/视图/写入能力下沉到 `coflow-engine`，编辑器只承担 UI 专属逻辑
- `CfdValue` 成为纯数据类型（不再内含 id 缓存），wire 可直接复用，无需孪生
- 解决目前因"源记录与合成记录共享 key"导致的本地化表显示错乱 bug

本次重构是**破坏性变更**（wire 协议、Tauri command 入参、`coflow-engine` 公共 API、`CfdValue::Ref` 内部表示都会调整）。Coflow 仍处早期版本，不提供兼容层。

---

## 目录

1. [背景与问题](#1-背景与问题)
2. [设计目标](#2-设计目标)
3. [身份与缓存的分离](#3-身份与缓存的分离)
4. [总体方案](#4-总体方案)
5. [分层与职责](#5-分层与职责)
6. [核心库改动](#6-核心库改动)
7. [编辑器后端改动](#7-编辑器后端改动)
8. [前端改动](#8-前端改动)
9. [TS 自动绑定](#9-ts-自动绑定)
10. [一致性保障](#10-一致性保障)
11. [实施 Phase 与验证标准](#11-实施-phase-与验证标准)
12. [兼容性与风险](#12-兼容性与风险)
13. [未来扩展](#13-未来扩展)
14. [错误码](#14-错误码)

---

## 1. 背景与问题

### 1.1 本地化表显示错乱（核心 bug）

打开 `data/dimensions/language/Item_name.csv`（合成 type `Item_nameVariants` 的 record 文件）时，编辑器渲染的列与值与源 `Item` 表完全一致，不是预期的 `default / zh / en`。

根因链：

1. 维度引入合成 record 后，源记录 `Item.potion` 与合成记录 `Item_nameVariants.potion` **共享 record key `potion`**
2. `coflow-engine::RecordIndex` 用 `keys: BTreeMap<String, RecordRef>` 单 key 索引，第二次 `add(potion)` 直接**覆盖**前一次，`get / file_for_key` 不再可靠
3. 编辑器 `editor/session/mod.rs::lookup_record_by_key` 用 `model.records().find(|r| r.key == key)` 线性扫描，命中的是源 `Item` 记录而非合成 `Item_nameVariants` 记录
4. 渲染管线据此把"本地化表文件"渲染为"Item 表"

`CfdDataModel` 内部本身**是** `(type, key)` 双键索引（`tables[type].primary_index`），合成与源记录从不冲突。bug 在 engine 与编辑器两层"自建的、丢掉 type 维度的、按 key 单键索引"。

### 1.2 wire 类型漂移

`editors/cfd-editor/frontend/src/bindings/index.ts` 是**手写**的 TS 接口，要和 `src-tauri/src/editor/types.rs` 双向保持一致。任一侧改字段、改 enum tag 都可能不同步，且无自动校验。

### 1.3 wire DTO 复刻

编辑器在 `editor/types.rs` + `editor/convert.rs` 重新发明了一套 wire DTO：`FieldValue` ≈ `CfdValue`、`FieldPathSegment` ≈ `CfdPathSegment`、`DictKey` ≈ `CfdDictKey`、`SourceCapabilities` ≈ `WriterCapabilities`、`DiagnosticItem` ≈ `Diagnostic`。每加一个变体或字段，都要在两处（model + wire）改并写转换。

### 1.4 跨宿主复用受阻

`build_file_tree / build_dimension_subtree / dimension_out_dirs / dimension_group_name / write_field_to_source / insert_record_in_source / delete_record_in_source / sheet_for_file_type / refresh_session_after_write` 都住在编辑器 crate 里，但本质和 UI 无关。CLI、未来的 LSP 或 Web 宿主想用同样能力时无法复用。

### 1.5 `CfdValue::Ref` 内含 id 缓存

`CfdValue::Ref { key, target: CfdRecordId }` 把"解析后的目标 id"塞进了值本身。这把"值"和"内部索引缓存"耦合在一起：

- 序列化时 id 字段如果跟着输出，会把不稳定的内部下标泄漏到 wire 上
- 反序列化时拿不到合法 id，需要在某处补一个哨兵或重新解析
- 阻碍 wire 直接复用 `CfdValue`，否则就要写一层孪生

---

## 2. 设计目标

1. **修复本地化 key 撞车 bug**：record 对外身份统一为 `(actual_type, key)`，源/合成天然分离
2. **核心库定义权威 wire 类型**：`CfdValue / CfdRecord / CfdPath* / Diagnostic / WriterCapabilities` 等直接作为 Tauri 返回值
3. **`CfdValue` 成为纯数据类型**：把 `Ref` 的 id 缓存剥离到 `CfdDataModel` 内部的 `ref_index`，`CfdValue` 不再含任何 id 字段，可以无障碍 Serialize/Deserialize
4. **轻度侵入**：核心库加 `Serialize/Deserialize` + `feature = "ts-export"` 控制的可选 `ts-rs::TS`；默认编译路径不引入 ts-rs，CLI / 其他宿主无感
5. **通用能力下沉**：file tree、维度元数据、写入事务、索引视图全部下沉到 `coflow-engine`
6. **编辑器后端瘦身**：从约 800 行收缩到约 300 行，只保留 SessionStore、Tauri command 路由、graph BFS、wire 装饰
7. **前端 wire 自动生成**：删手写 `bindings/index.ts`，生成产物纳入 git，CI 校验无漂移

非目标：

- 不重写 `Cargo` 工作区结构（不引入新 crate `coflow-views`）
- 不改写 schema 编译、loader、writer 的对外 API（除非字段需要加 derive）

---

## 3. 身份与缓存的分离

本次重构最核心的一条设计原则：**值不带缓存，身份不暴露内部 id**。

### 3.1 三种"指代记录"的方式

| 用途 | 表达 | 寿命 | 暴露范围 |
|---|---|---|---|
| **对外稳定坐标**（wire / 路由 / 持久化引用） | `(actual_type: String, key: String)` | 永久（schema 不重命名前提下） | 前端、CLI、未来宿主 |
| **内部高效索引**（hot path、ref 解析、evaluator） | `CfdRecordId(usize)` | 单次 `model.build()` 内 | model / engine 内部 |
| **跨记录引用的值**（在 `CfdValue` 中） | `target_type + target_key` 字符串 | 永久 | 同 wire |

`CfdRecordId` 不消失，但**不出 engine 边界**：

- model 内部所有索引、ref 解析结果、evaluator hot path 继续用 id
- wire 边界只暴露 `(actual_type, key)`
- engine 提供 `id_for_coordinate(type, key) -> Option<CfdRecordId>` 在边界处一次性解析

### 3.2 为什么不直接把 `CfdRecordId` 改稳定

- 改为 `(Arc<str>, Arc<str>)`：失去 O(1) 下标访问，hot path 大幅变慢
- 改为 GUID 写盘：引入持久化 id 的合并冲突、迁移、删除追踪一连串问题，超出本次 scope
- 改为"按 (type, key) 字典序分配下标"：仍然不稳——加/删 record 会让 id 整体偏移；只解决可复现性，不解决前端缓存

`CfdRecordId` 的本质就是"`Vec<CfdRecord>` 的下标"，这个优化在 model 内部完全合理且必要。需要做的是把它**关在 engine 边界内**，wire 用稳定坐标。

### 3.3 把 `CfdValue::Ref` 的 id 缓存剥离

当前：

```rust
pub enum CfdValue {
    Ref { key: String, target: CfdRecordId },  // ← id 嵌在值里
}
```

重构后：

```rust
pub enum CfdValue {
    Ref { target_type: String, target_key: String },  // ← 纯数据
}

pub struct CfdDataModel {
    // ...
    ref_index: BTreeMap<RefSite, CfdRecordId>,   // 由 model.build() 填充
}

pub struct RefSite {
    pub host: CfdRecordId,
    pub path: CfdPath,
}

impl CfdDataModel {
    pub fn resolve_ref(&self, site: RefSite) -> Option<CfdRecordId>;
}
```

收益：

- `CfdValue` 是 100% 纯数据类型，可以无障碍 `Serialize + Deserialize`
- wire 直接复用 `CfdValue`，零孪生、零 `#[serde(skip)]`、零哨兵
- 反序列化拿回的 `CfdValue::Ref` 携带完整信息（`target_type + target_key`），可以无歧义重新解析
- model 边界更清晰："值"是数据，"缓存"是 model 的关切

代价：

- evaluator / checker 取 ref 目标时多一次 `BTreeMap` 查找（O(log n)），相对 model 自身已有开销可忽略
- `model.build()` 多一次扫描填充 `ref_index`，O(N × 平均 ref 数)

### 3.4 写入与 rebuild 的身份保持

- 前端写入请求带 `(actual_type, key)` 坐标
- engine 入口处 `id_for_coordinate` 解析为 id，内部走 id 快路径
- 写入完成后 engine in-place rebuild model；rebuild 后 `(actual_type, key)` 仍指向同一条逻辑记录（除非 key 字段本身被改），坐标稳定
- `WriteOutcome` 返回操作后的新坐标，前端用返回值刷新缓存

---

## 4. 总体方案

### 4.1 wire 直接复用 model / api 类型

凡是核心库已有的权威类型，wire 不再复刻：

| 旧 wire 类型 | 替换为 |
|---|---|
| `FieldValue` | `coflow_data_model::CfdValue`（剥离 id 后） |
| `FieldPathSegment` | `coflow_data_model::CfdPathSegment` |
| `DictKey / DictEntry` | `coflow_data_model::CfdDictKey` + `CfdValue` |
| `SourceCapabilities` | `coflow_api::WriterCapabilities`（加 `provider_id` 字段） |
| `DiagnosticItem` | `coflow_api::Diagnostic` 扁平视图 |

编辑器**保留**的 wire 类型：`EditorError / ProjectSnapshot / FileRecords / RecordRow / FieldCell / FieldAnnotation / SpreadInfo / WriteFieldOutcome / InsertRecordOutcome / DeleteRecordOutcome / GraphData / GraphNode / GraphEdge / RecordCoordinate / RefTarget`，它们是"组合视图"，不是"model 镜像"。

### 4.2 编辑器附加注解 `FieldAnnotation`

对于 model 不直接表达、但编辑器渲染需要的派生信息（spread 来源、ref 目标文件、enum 整数值），统一收口为 `FieldAnnotation`：

```rust
pub struct FieldCell {
    pub name: String,
    pub value: CfdValue,                  // ← 直接用 model 类型
    pub annotation: Option<FieldAnnotation>,
}

pub struct FieldAnnotation {
    pub spread_info: Option<SpreadInfo>,
    pub ref_target_file: Option<String>,
    pub enum_int_value: Option<i64>,
}
```

注意：`FieldAnnotation` 中**不再**含 `ref_target_id`（前 spec 草稿曾有），因为 wire 不暴露 id；前端要导航 ref 直接用 `(target_type, target_key)` 调 `get_file_records` 或路由跳转。

### 4.3 通用能力下沉到 `coflow-engine`

新增 / 上移到 engine 的 API：

```rust
impl ProjectSession {
    // 坐标解析（wire 边界 → 内部 id）
    pub fn id_for_coordinate(&self, actual_type: &str, key: &str) -> Option<CfdRecordId>;
    pub fn coordinate_of(&self, id: CfdRecordId) -> Option<RecordCoordinate>;

    // 索引视图（按坐标取，内部解析为 id）
    pub fn record_view(&self, actual_type: &str, key: &str) -> Option<RecordView<'_>>;
    pub fn record_views_in_file(&self, file: &str) -> impl Iterator<Item = RecordView<'_>>;
    pub fn coordinates_in_file(&self, file: &str) -> impl Iterator<Item = RecordCoordinate>;
    pub fn file_for_record(&self, actual_type: &str, key: &str) -> Option<&str>;

    // 文件树
    pub fn file_tree(&self) -> Vec<FileTreeNode>;
    pub fn file_tree_with(&self, options: FileTreeOptions) -> Vec<FileTreeNode>;

    // 维度元数据
    pub fn dimensions(&self) -> impl Iterator<Item = &DimensionInfo>;
    pub fn dimension(&self, name: &str) -> Option<&DimensionInfo>;

    // 写入事务（含 preflight、writer 调度、写后 rebuild）
    pub fn write_field(&mut self, actual_type: &str, key: &str,
                       path: &[CfdPathSegment], value: &CfdValue)
        -> Result<WriteOutcome, DiagnosticSet>;
    pub fn insert_record(&mut self, file: &str, key: &str, actual_type: &str,
                          fields: BTreeMap<String, CfdValue>)
        -> Result<WriteOutcome, DiagnosticSet>;
    pub fn delete_record(&mut self, actual_type: &str, key: &str)
        -> Result<WriteOutcome, DiagnosticSet>;

    // 模型辅助查询
    pub fn enum_int_value(&self, enum_name: &str, variant: &str) -> Option<i64>;
}
```

`RecordCoordinate` 是 wire 上的稳定坐标值类型：

```rust
pub struct RecordCoordinate {
    pub actual_type: String,
    pub key: String,
}
```

`RecordView<'a>` 是 engine 内部 + 宿主获取记录的统一视图：

```rust
pub struct RecordView<'a> {
    pub coordinate: RecordCoordinate,
    pub display_path: &'a str,
    pub record: &'a CfdRecord,
    pub origin: &'a RecordOrigin,
    pub source_id: SourceId,
    pub provider_id: &'a str,
}
```

`WriteOutcome` 描述写后状态：

```rust
pub struct WriteOutcome {
    pub touched: Vec<RecordCoordinate>,
    pub inserted: Option<RecordCoordinate>,
    pub deleted: Option<RecordCoordinate>,
    /// `Some` 当本次写入触发了 key 字段变化（旧坐标 → 新坐标）。
    pub renamed: Option<(RecordCoordinate, RecordCoordinate)>,
    pub diagnostics: DiagnosticSet,
}
```

---

## 5. 分层与职责

```
coflow-data-model    权威数据类型（CfdValue 纯化、CfdRecord、CfdPath*、CfdDictKey）
coflow-cft           权威 schema 类型（按需导出）
coflow-api           跨 crate 接口（Diagnostic / WriterCapabilities / RecordOrigin / ResolvedSource）
coflow-project       项目配置（含 DimensionConfig，加 display_name）
coflow-engine        ref_index 持有方、记录索引、文件树、维度元数据、写入事务、坐标↔id 转换
─────── 以上：所有宿主共享，加 Serialize/Deserialize/可选 TS ───────
cfd-editor/src-tauri SessionStore、Tauri command 路由、EditorError、wire 装饰、graph BFS
frontend             100% 生成类型 + UI
```

| crate | 是否依赖 `ts-rs` | 备注 |
|---|---|---|
| `coflow-data-model` | feature `ts-export`，默认关 | derive cfg_attr |
| `coflow-cft` | feature `ts-export`，默认关 | 暂仅预留 |
| `coflow-api` | feature `ts-export`，默认关 | derive cfg_attr |
| `coflow-project` | feature `ts-export`，默认关 | derive cfg_attr |
| `coflow-engine` | feature `ts-export`，默认关 | derive cfg_attr |
| `cfd-editor/src-tauri` | 直接依赖（无 feature） | 启用上游 `ts-export` feature 触发导出 |
| 其他宿主（CLI 等） | 不依赖 | 默认 feature 路径完全不引入 ts-rs |

---

## 6. 核心库改动

### 6.1 `coflow-data-model`

**Cargo.toml**：

```toml
[features]
default = []
ts-export = ["dep:ts-rs"]

[dependencies]
serde = { version = "1", features = ["derive"] }
ts-rs = { version = "10", optional = true }
```

**`CfdValue` 纯化**：

```rust
// 重构前
pub enum CfdValue {
    // ...
    Ref { key: String, target: CfdRecordId },
}

// 重构后
pub enum CfdValue {
    // ...
    Ref { target_type: String, target_key: String },
}
```

无 `#[serde(skip)]`、无哨兵、无 id 字段。

**`CfdDataModel` 新增 `ref_index`**：

```rust
pub struct CfdDataModel {
    pub(crate) tables: BTreeMap<String, CfdTable>,
    pub(crate) inheritance_index: BTreeMap<String, CfdPolymorphicIndex>,
    pub(crate) records: Vec<CfdRecord>,
    pub(crate) ref_index: BTreeMap<RefSite, CfdRecordId>,   // 新增
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RefSite {
    pub host: CfdRecordId,
    pub path: CfdPath,
}

impl CfdDataModel {
    /// 在 `host` 记录的 `path` 位置查 `CfdValue::Ref` 已解析的目标 id。
    /// 返回 None 表示要么不是 Ref，要么 target_type/key 当前无法解析（缺失目标）。
    pub fn resolve_ref(&self, site: &RefSite) -> Option<CfdRecordId>;
}
```

`ModelCompiler` build 阶段填充 `ref_index`：遍历所有记录的字段，遇到 `CfdValue::Ref` 就解析 `(target_type, target_key) → CfdRecordId` 写入。原本 evaluator/checker 通过 `record.fields[...]::Ref { target, .. }` 拿到的 id，改为通过 `model.resolve_ref(&RefSite { host, path })` 拿。

**加 `Serialize + Deserialize + cfg_attr TS` 的类型**：

- `CfdValue`（已纯化）
- `CfdDictKey`
- `CfdRecord`（含 `key / actual_type / fields / origin / spread_field_sources`）
- `CfdPathSegment / CfdPath`
- `RecordOrigin`（若已在 api 中，则只导一处）

注意：`CfdRecord.spread_field_sources: BTreeMap<String, CfdRecordId>` 也含 id。两种处理：

- 标 `#[serde(skip)]`，反序列化为空——前端不需要这个字段，只需要 `SpreadInfo` 派生信息
- 或拆出 `spread_index` 到 `CfdDataModel` 内部，对应处理同 ref（更彻底但工作量大）

本次取**前者**（`#[serde(skip)]`）。`CfdRecord` 本身不会从 wire 反序列化回 model 实例（只在前端展示），不存在哨兵被误用风险。`spread_field_sources` 由 engine 内部维护，反序列化重建 model 时需通过 model rebuild 重新填充。

**`CfdRecordId` 不导出 TS**：保持 `pub`（model / engine 内部互通），但**不**加 `cfg_attr(TS)`。任何企图把 `CfdRecordId` 序列化到 wire 的代码都因缺少 derive 而编译失败。

**新增**：测试样本生成器

```rust
#[cfg(any(test, feature = "test-fixtures"))]
impl CfdValue {
    pub fn all_sample_variants() -> Vec<CfdValue> { /* 覆盖每个 variant */ }
}
```

### 6.2 `coflow-cft`

预留 feature `ts-export`，本次不强制导出 schema 类型。若前端后续需要展示 schema 树则按需追加。

### 6.3 `coflow-api`

**Cargo.toml**：同样 feature gate。

**加 `Serialize + cfg_attr TS` 的类型**：

- `Diagnostic`（含 `SourceLocation`、`Label`、`Severity`）
- `DiagnosticSet`
- `WriterCapabilities`，新增字段 `pub provider_id: String`（让 wire 端"id + 能力"合并表达，删除编辑器 `SourceCapabilities` 包装）
- `RecordOrigin`
- `ResolvedSource`（若有 wire 需求）

**新增**：`Diagnostic::flat_view() -> FlatDiagnostic { severity, code, stage, message, file_path, record_key, field_path }`，前端继续以扁平结构消费，但住在核心库。

### 6.4 `coflow-project`

`DimensionConfig` 加字段：

```rust
pub struct DimensionConfig {
    pub variants: Vec<String>,
    pub out_dir: Option<PathBuf>,
    pub display_name: Option<String>,   // 新增；缺省时编辑器走"内建映射"或退化为 name
}
```

加 `Serialize + cfg_attr TS`。

### 6.5 `coflow-engine`

**`RecordIndex` 重做**：

```rust
pub struct RecordIndex {
    by_id: BTreeMap<CfdRecordId, RecordRef>,                  // 内部主索引
    files: BTreeMap<String, Vec<CfdRecordId>>,                // file → ids
    by_coordinate: BTreeMap<(String, String), CfdRecordId>,   // (actual_type, key) → id（边界入口）
}

pub struct RecordRef {
    pub id: CfdRecordId,
    pub coordinate: RecordCoordinate,   // 持有 actual_type + key
    pub origin: RecordOrigin,
    pub source_id: SourceId,
    pub provider_id: String,
    pub display_path: String,
}

impl RecordIndex {
    pub fn get(&self, id: CfdRecordId) -> Option<&RecordRef>;
    pub fn get_by_coordinate(&self, actual_type: &str, key: &str) -> Option<&RecordRef>;
    pub fn ids_in_file(&self, file: &str) -> &[CfdRecordId];
    pub fn file_for_id(&self, id: CfdRecordId) -> Option<&str>;
    pub fn id_for_coordinate(&self, actual_type: &str, key: &str) -> Option<CfdRecordId>;
}
```

`push_loaded_records` 调整为两阶段：

1. 加载阶段：把 `(key, actual_type, origin, source_id, provider_id, display_path)` 暂存为 `PendingRecordRef`
2. `model.build()` 完成后：遍历 `model.records()` 拿到 `CfdRecordId`，按 `(actual_type, key)` 与 pending 项匹配，构造完整 `RecordRef` 写入 `by_id` 与 `by_coordinate`

合成 record 与源 record 共 key 但 type 不同，按 `(actual_type, key)` 联合键稳定匹配。

**新增子模块**：

```
coflow-engine/src/
├── coordinate.rs     // RecordCoordinate 类型（也可放 records.rs）
├── files.rs          // FileTreeNode / FileTreeOptions / build_file_tree（从 editor 上移）
├── records.rs        // RecordView / RecordIndex 重做
├── dimensions/
│   ├── mod.rs
│   ├── synthesize.rs (现存)
│   ├── regenerate.rs (现存)
│   └── info.rs       // DimensionInfo / display_name 解析
└── writes.rs         // write_field / insert_record / delete_record / refresh / sheet 推断
```

`FileTreeNode / FileTreeOptions` 字段与现编辑器版本一致：

```rust
pub struct FileTreeNode {
    pub name: String,
    pub path: String,         // 项目相对，/ 分隔
    pub is_dir: bool,
    pub in_sources: bool,
    pub children: Vec<FileTreeNode>,
}

pub struct FileTreeOptions {
    pub include_dimension_groups: bool,   // 默认 true
    pub extra_extensions: Vec<String>,    // 默认从 registered loaders
}
```

`build_file_tree / build_dimension_subtree` 整体迁入。`dimension_group_name` 由 `DimensionConfig.display_name` + 内建 fallback (`"language" → "本地化"`) 决定。

`DimensionInfo`：

```rust
pub struct DimensionInfo {
    pub name: String,
    pub display_name: String,
    pub variants: Vec<String>,
    pub out_dir: PathBuf,
    pub fields: Vec<DimensionField>,
}
```

**写入事务**：把以下函数从 `editor/session/mod.rs` 整体迁入 `engine/writes.rs`：

- `write_field_to_source`
- `insert_record_in_source`
- `delete_record_in_source`
- `refresh_session_after_write`
- `sheet_for_file_type`
- `resolved_source_for_file`
- `resolve_effective_origin`

`ProjectSession::write_field / insert_record / delete_record` 是单一入口，内部完成「坐标→id 解析」→「preflight」→「writer.write」→「rebuild session」→ 返回 `WriteOutcome`。

`ProjectSession` 变为可 `&mut` 调用（当前是 immutable build-once，引入写入后 SessionStore 需相应调整锁）。

**evaluator / checker 对 ref 的访问**：

```rust
// 旧（直接从 CfdValue::Ref 拿 id）
let CfdValue::Ref { target, .. } = value else { return ... };
let target_record = model.record(*target)?;

// 新（通过 ref_index 查）
let CfdValue::Ref { target_type, target_key } = value else { return ... };
let target_id = model.resolve_ref(&RefSite { host, path: current_path.clone() })?;
let target_record = model.record(target_id)?;
```

ref site 的 `path` 由 evaluator 在遍历时维护（已存在 path 跟踪机制）。

**TS 导出**：`FileTreeNode / FileTreeOptions / DimensionInfo / WriteOutcome / RecordCoordinate` 加 cfg_attr；`RecordView` 不导出（含借引用，难导）。

### 6.6 写入路径的 Origin 推断

`origin` 由 model 提供（`record.origin`），spread 字段重定向逻辑也归 engine 内部（已在 `editor::resolve_effective_origin`，搬过来）。

---

## 7. 编辑器后端改动

### 7.1 删除的代码

- `editor/session/mod.rs::lookup_record_by_key`
- `editor/session/mod.rs::record_file_map`
- `editor/session/mod.rs::write_field_to_source / insert_record_in_source / delete_record_in_source`
- `editor/session/mod.rs::sheet_for_file_type / resolved_source_for_file / resolve_effective_origin / refresh_session_after_write`
- `editor/session/mod.rs::snapshot_record_before_delete`（改为 engine 提供 record 视图 + 编辑器侧 wire 转换）
- `editor/session/file_tree.rs`（整体下沉 engine）
- `editor/session/build.rs::dimension_out_dirs / dimension_group_name / session_file_tree / static_provider_id`
- `editor/types.rs::FieldValue / FieldCell.value 的孪生 enum / FieldPathSegment / DictEntry / DictKey`（改用 model 类型）
- `editor/types.rs::SourceCapabilities`（用 `WriterCapabilities`）
- `editor/types.rs::DiagnosticItem`（用 `Diagnostic::flat_view`）
- `editor/session/diagnostics.rs::diagnostic_from_api`（核心库提供）

### 7.2 保留并改造

`editor/types.rs` 留下：

```rust
pub struct EditorError { kind, message, diagnostics: Vec<FlatDiagnostic> }
pub enum EditorErrorKind { Session, Project, Write, NotFound, Other }

pub struct ProjectSnapshot {
    pub session_id: u32,
    pub project_root: String,
    pub file_tree: Vec<FileTreeNode>,      // ← engine 类型
    pub diagnostics: Vec<FlatDiagnostic>,
}

pub struct FileRecords {
    pub file_path: String,
    pub type_names: Vec<String>,
    pub records: Vec<RecordRow>,
    pub capabilities: WriterCapabilities,  // ← api 类型
}

pub struct RecordRow {
    pub coordinate: RecordCoordinate,      // ← (actual_type, key)
    pub display_path: String,
    pub fields: Vec<FieldCell>,
}

pub struct FieldCell {
    pub name: String,
    pub value: CfdValue,                   // ← model 类型（已纯化）
    pub annotation: Option<FieldAnnotation>,
}

pub struct FieldAnnotation {
    pub spread_info: Option<SpreadInfo>,
    pub ref_target_file: Option<String>,
    pub enum_int_value: Option<i64>,
}

pub struct SpreadInfo {
    pub source: RecordCoordinate,                  // ← 用坐标
    pub source_record_file: Option<String>,
    pub source_field_path: Vec<String>,
}

pub struct WriteFieldOutcome { pub row: RecordRow, pub diagnostics: Vec<FlatDiagnostic> }
pub struct InsertRecordOutcome { pub file_records: FileRecords, pub diagnostics: Vec<FlatDiagnostic> }
pub struct DeleteRecordOutcome {
    pub file_records: FileRecords,
    pub diagnostics: Vec<FlatDiagnostic>,
    pub deleted_snapshot: Option<DeletedRecordSnapshot>,
}

pub struct DeletedRecordSnapshot {
    pub record: CfdRecord,                 // 直接用 model 类型
    pub display_path: String,
}

pub struct GraphData { pub nodes: Vec<GraphNode>, pub edges: Vec<GraphEdge> }
pub struct GraphNode {
    pub coordinate: RecordCoordinate,
    pub file_path: String,
    pub in_focus_file: bool,
    pub is_collapsed: bool,
    pub fields: Vec<FieldCell>,
}
pub struct GraphEdge {
    pub source: RecordCoordinate,
    pub target: RecordCoordinate,
    pub field_path: String,
}

pub struct RefTarget {
    pub coordinate: RecordCoordinate,
    pub file_path: String,
}
```

所有上面这些类型 derive `Serialize + ts_rs::TS`。`#[ts(export, export_to = "../../frontend/src/bindings/")]`。

### 7.3 Tauri command 签名

```rust
fn load_project(yaml_path: PathBuf) -> Result<ProjectSnapshot, EditorError>;
fn get_file_records(session: u32, file_path: String) -> Result<FileRecords, EditorError>;
fn make_default_object(session: u32, type_name: String) -> Result<CfdValue, EditorError>;
fn get_enum_variants(session: u32, enum_name: String) -> Result<Vec<String>, EditorError>;
fn get_ref_targets(session: u32, expected_type: String) -> Result<Vec<RefTarget>, EditorError>;
fn get_graph(session: u32, file_path: String) -> Result<GraphData, EditorError>;

fn write_field(session: u32, coordinate: RecordCoordinate,
               field_path: Vec<CfdPathSegment>, new_value: CfdValue)
    -> Result<WriteFieldOutcome, EditorError>;

fn insert_record(session: u32, file_path: String, key: String,
                 actual_type: String, fields: CfdValue /* Object */)
    -> Result<InsertRecordOutcome, EditorError>;

fn delete_record(session: u32, coordinate: RecordCoordinate)
    -> Result<DeleteRecordOutcome, EditorError>;
```

### 7.4 SessionStore 锁调整

`ProjectSession` 现在被写入事务 `&mut`。原 `state: RwLock<EditorSession>` + `write_mutex: Mutex<()>` 调整为：

- 写入：`state.write()` 拿到 `&mut EditorSession`，直接调 `session.engine.write_field(...)`，engine 内部完成 rebuild（in-place 替换 model/sources/files/records）
- 读：`state.read()` 不变

`write_mutex` 不再需要（`RwLock::write` 已经独占），删除。

### 7.5 `convert.rs` 收敛

只剩 `cfd_value_annotation(value: &CfdValue, session: &ProjectSession) -> Option<FieldAnnotation>`，负责：

- 若是 `CfdValue::Ref { target_type, target_key }`：查 `session.file_for_record(target_type, target_key)` 拿 file
- 若是 `CfdValue::Enum { enum_name, variant }`：查 `session.enum_int_value`
- 若字段经过 spread：从 `record.spread_field_sources` 构造 `SpreadInfo`（spread_field_sources 在 engine 内部不再 `#[serde(skip)]`-only，但编辑器侧通过 `RecordView` 拿到它做派生）

记录 → `RecordRow` 的转换：

```rust
fn record_to_row(view: RecordView<'_>, session: &ProjectSession) -> RecordRow {
    let fields = view.record.fields.iter().map(|(name, value)| FieldCell {
        name: name.clone(),
        value: value.clone(),
        annotation: cfd_value_annotation(value, session),
    }).collect();
    RecordRow {
        coordinate: view.coordinate,
        display_path: view.display_path.to_string(),
        fields,
    }
}
```

---

## 8. 前端改动

### 8.1 删除

- `editors/cfd-editor/frontend/src/bindings/index.ts`（整文件，由生成产物代替）

### 8.2 生成产物布局

```
frontend/src/bindings/
├── CfdValue.ts
├── CfdRecord.ts
├── CfdPathSegment.ts
├── CfdDictKey.ts
├── FileTreeNode.ts
├── DimensionInfo.ts
├── WriterCapabilities.ts
├── FlatDiagnostic.ts
├── RecordCoordinate.ts
├── EditorError.ts
├── ProjectSnapshot.ts
├── FileRecords.ts
├── RecordRow.ts
├── FieldCell.ts
├── FieldAnnotation.ts
├── SpreadInfo.ts
├── WriteFieldOutcome.ts
├── InsertRecordOutcome.ts
├── DeleteRecordOutcome.ts
├── DeletedRecordSnapshot.ts
├── GraphData.ts
├── GraphNode.ts
├── GraphEdge.ts
└── RefTarget.ts
```

文件头自动带 `// @generated by ts-rs`。

### 8.3 路由 / 状态 / 写回路径

- `Route` 变体 `{ recordKey: string }` → `{ coordinate: RecordCoordinate }`
- `fileDataCache` 继续按 file 缓存，行内取值改 `rows.find(r => r.coordinate.actual_type === t && r.coordinate.key === k)` 或建立 `Map<string, RecordRow>` 索引（key = `${type}::${key}`）
- 所有 `onWriteField / onInsertRecord / onDeleteRecord` 的 record 入参从 `key: string` 改为 `coordinate: RecordCoordinate`
- 撤销栈、跳转、context menu 全部按 coordinate
- 列头 / 详情头 / 标签等**展示**仍用 `coordinate.key`（人读字段）
- key 字段编辑场景：写后用 `WriteFieldOutcome.row.coordinate` 替换前端缓存中的旧坐标项

### 8.4 UI 行为不变

`isDimensionDefaultField`、本地化分组、`default` 单元格只读三处逻辑（spec 16 Phase 5）继续按 `record.coordinate.actual_type.endsWith('Variants') + field name === 'default'` 实现。本次重构仅替换底层类型，不改 UI 规则。

---

## 9. TS 自动绑定

### 9.1 工具选型

`ts-rs` v10+。理由：纯 derive、零运行时、生态稳定、对 serde 兼容好；通过 `#[ts(export)]` 注册函数，运行 `cargo test` 时触发文件输出。

### 9.2 注解风格

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export",
           ts(export, export_to = "../../editors/cfd-editor/frontend/src/bindings/"))]
pub enum CfdValue { ... }
```

`export_to` 用工作区相对路径，所有 crate 输出汇聚到同一目录。

### 9.3 触发

`editors/cfd-editor/src-tauri/tests/export_bindings.rs`：

```rust
//! Run with: cargo test --features ts-export -p cfd-editor export_bindings
#[test]
fn export_bindings() {
    // body 留空——ts-rs 注册的 export 函数在测试启动时由 ctor 触发
}
```

### 9.4 CI gate

在主 CI workflow 加：

```bash
cargo test --features ts-export -p cfd-editor export_bindings
git diff --exit-code editors/cfd-editor/frontend/src/bindings
```

未跑生成器或漏跑导致漂移的 PR 直接挂红。

### 9.5 默认编译路径

- `cargo build` / `cargo build --release`：不引入 ts-rs
- `cargo test`：默认不引入 ts-rs
- CLI、其他宿主：不引入 ts-rs

只有显式 `--features ts-export` 时才链接。

---

## 10. 一致性保障

由于 wire 类型**直接复用** model 类型（且 `CfdValue` 已纯化为不含 id 的纯数据类型），没有"孪生"层，一致性自动得到保证：

| 漂移源 | 防护 |
|---|---|
| 核心 enum 加 variant | ts-rs 重新生成 → 前端 `tsc` 报缺分支 |
| 核心 struct 加字段 | ts-rs 重新生成 → 前端类型变 → 调用点 tsc 报错 |
| serde tag/rename 改 | ts-rs 输出变 → CI diff 红 |
| 漏跑生成器 | CI `git diff --exit-code` 拦截 |
| `CfdValue` 加 variant | ts-rs 输出变 + 编辑器 `cfd_value_annotation` 穷尽 match 编译失败 |

`convert.rs` 中 `cfd_value_annotation` 用穷尽 match 处理 `CfdValue`，加 `#[deny(non_exhaustive_omitted_patterns)]`，核心加新 variant 时编辑器编译失败。

不需要 round-trip 测试、不需要 `serde-reflection`、不需要快照对比。

---

## 11. 实施 Phase 与验证标准

每个 Phase 独立可提交，按顺序串行推进。

### Phase 0 — `CfdValue` 纯化 + `ref_index` 内化

**改动**：

- `CfdValue::Ref { key, target: CfdRecordId }` → `CfdValue::Ref { target_type, target_key }`
- `CfdDataModel` 新增 `ref_index: BTreeMap<RefSite, CfdRecordId>` 与 `resolve_ref` 方法
- `ModelCompiler::build` 填充 `ref_index`
- evaluator / checker / 所有直接读 `CfdValue::Ref.target` 的代码改为查 `model.resolve_ref(&RefSite)`
- 跟改：`coflow-checker`、`coflow-codegen-csharp`、`coflow-exporter-*`、`coflow-loader-*`、`coflow-engine`、`cfd-editor` 中所有 `match CfdValue::Ref { target, .. }` 调用点

**验证**：

- `cargo test --workspace` 通过
- 性能基线：`tests/perf/large_project.rs`（如存在）的 check 时长不显著变差（允许 5% 内）
- 新增测试：`tests/cfd_value_pure_serde.rs` 跑 `CfdValue` round-trip，确认无 id 字段泄漏

### Phase 1 — 核心库加 serde + ts-export feature

**改动**：

- 五个核心 crate 加 feature `ts-export`，加 `ts-rs` optional dep
- `coflow-data-model`：`CfdValue / CfdRecord / CfdPath* / CfdDictKey / RecordOrigin` 加 `Serialize + Deserialize + cfg_attr(TS)`
- `CfdRecord.spread_field_sources` 标 `#[serde(skip)]`
- `CfdRecordId` 仅加 `Serialize + Deserialize`（供 engine 内部使用，不加 TS）
- `coflow-api`：`Diagnostic / DiagnosticSet / Severity / Label / SourceLocation / WriterCapabilities / RecordOrigin / ResolvedSource` 加 derive；`WriterCapabilities` 加 `provider_id`；新增 `Diagnostic::flat_view`
- `coflow-project`：`DimensionConfig` 加 `display_name`；加 derive
- `coflow-engine`：暂不动逻辑，预留 feature

**验证**：

- `cargo build --workspace`（默认 feature）通过，dep tree 中无 `ts-rs`
- `cargo build --workspace --features cfd-editor/ts-export` 通过
- 加 `tests/serde_roundtrip.rs`：对每个新加 Serialize 的类型跑 round-trip

### Phase 2 — `RecordIndex` 坐标化（修 bug 的核心提交）

**改动**：

- `RecordIndex` 内部主索引换 `by_id: BTreeMap<CfdRecordId, RecordRef>` + `by_coordinate: BTreeMap<(String, String), CfdRecordId>`
- `RecordRef` 加 `id / coordinate`
- 两阶段构造：loader 出 `PendingRecordRef`；`model.build()` 后回填
- `keys_for_file → ids_in_file / coordinates_in_file`；`file_for_key → file_for_id / file_for_coordinate`；新增 `id_for_coordinate`
- 编辑器 `lookup_record_by_key` 改用 `engine.record_view(type, key)`

**验证**：

- `cargo test --workspace` 通过
- 新增集成测试：源 `Item.potion` + 合成 `Item_nameVariants.potion` 共存时，两个 record 在 `RecordIndex` 中各占一项；按 file 查得到正确 coordinate
- 手动打开本地化表 → 显示 default/zh/en 列

### Phase 3 — engine API 扩面（视图 + 文件树 + 维度）

**改动**：

- 新增 `engine/files.rs`：`FileTreeNode / FileTreeOptions / build_file_tree`（从编辑器迁入）
- 新增 `engine/records.rs`：`RecordView / RecordCoordinate / record_view / record_views_in_file`
- `engine/dimensions/info.rs`：`DimensionInfo` + `ProjectSession::dimensions / dimension`
- `engine/lib.rs`：暴露 `enum_int_value / file_for_record / coordinates_in_file`
- 编辑器 `session/build.rs` 调用 engine `file_tree()` 替代自构造
- 删除 `editor/session/file_tree.rs`、`editor/session/build.rs::dimension_out_dirs / dimension_group_name / session_file_tree / static_provider_id`

**验证**：

- `cargo test -p coflow-engine` 通过
- `cargo build -p cfd-editor` 通过
- 手动验证：本地化分组、文件树展示与重构前一致

### Phase 4 — engine 写入事务下沉

**改动**：

- 新增 `engine/writes.rs`：`write_field / insert_record / delete_record / refresh_after_write / sheet_for_file_type / resolve_source_for_file`
- `ProjectSession::write_field / insert_record / delete_record` 公开 API，入参用 `(actual_type, key)` 坐标
- `ProjectSession` 变 `&mut` 写入；engine 内部 rebuild（in-place 替换字段）
- `WriteOutcome` 类型（含 `renamed` 字段）
- 编辑器 `session/mod.rs` 的三个写入函数删除，调用替换为 `session.engine.write_field(...)`
- SessionStore 删除 `write_mutex`

**验证**：

- `cargo test --workspace` 通过
- 写入相关集成测试：write/insert/delete 各跑一遍，验证 `WriteOutcome.touched / inserted / deleted / renamed` 正确
- 编辑 key 字段：验证 `renamed: Some((old, new))` 正确返回
- 手动验证：编辑器写入回路不变

### Phase 5 — wire 类型瘦身

**改动**：

- `editor/types.rs` 删除 `FieldValue / FieldPathSegment / DictEntry / DictKey / SourceCapabilities / DiagnosticItem`
- `RecordRow` 改为 `coordinate / display_path / fields`
- `FieldCell` 改 `value: CfdValue + annotation: Option<FieldAnnotation>`
- `SpreadInfo` 改为含 `source: RecordCoordinate`
- `GraphNode / GraphEdge` 字段改坐标
- `RefTarget` 新类型
- `convert.rs` 收敛为 `cfd_value_annotation` + `record_to_row`
- 所有 wire 类型 derive `Serialize + ts_rs::TS`
- Tauri command 签名按 §7.3 调整

**验证**：

- `cargo build -p cfd-editor` 通过
- `cargo test --features ts-export -p cfd-editor export_bindings`：生成 TS 文件
- 检视生成产物，字段齐全

### Phase 6 — 前端切换

**改动**：

- 删 `frontend/src/bindings/index.ts`
- 全代码库 import 切到生成产物
- Route / state / 写回 / undo / 跳转 / context menu 全部按 coordinate
- `App.tsx::fileDataCache` 调整为按 coordinate 索引
- `RecordView` / `TableView` / `DataCard` / `GraphView` 改造

**验证**：

- `npx tsc --noEmit` 通过
- Tauri dev 模式手动验证：
  - 打开本地化表显示 default/zh/en 列
  - 编辑 variant 列写回成功
  - default 列只读
  - 编辑 key 字段 → 前端缓存更新到新坐标
  - 撤销、跳转、context menu、graph view 正常

### Phase 7 — CI gate

**改动**：

- 主 CI workflow 加步骤：`cargo test --features ts-export -p cfd-editor export_bindings && git diff --exit-code frontend/src/bindings`
- PR 模板提醒：改了核心类型后跑生成器并提交 bindings

**验证**：

- 故意改一个核心字段不跑生成器 → CI 红
- 跑了生成器并提交 bindings → CI 绿

---

## 12. 兼容性与风险

### 12.1 破坏性变更范围

- `CfdValue::Ref` 内部表示（`target: CfdRecordId` → `target_type + target_key` 字符串），所有读 ref 的代码必须改
- Tauri command 入参签名（`record_key` → `coordinate`）
- wire 类型形状（`FieldValue` → `CfdValue` 等）
- `coflow-engine` 公共 API（`keys_for_file` → `coordinates_in_file` 等）
- 前端 `Route` 类型（`recordKey` → `coordinate`）

均不提供过渡兼容。同一 PR 内修完。

### 12.2 风险点

1. **`CfdValue::Ref` 改造影响面广**：evaluator / checker / codegen / exporter / loader / writer 都可能读 `target`。需要全代码库 grep `CfdValue::Ref` 并 `match { target, .. }` 逐一改造为 `model.resolve_ref(&RefSite)`。Phase 0 单独成提交，便于 review 与回退。

2. **`ref_index` 填充开销**：每次 `model.build()` 多扫一遍记录填 ref_index。实测影响应在 ms 级（与已有 schema 检查相比可忽略）。Phase 0 验证步骤包含性能基线。

3. **key 字段编辑后坐标失效**：编辑 key 字段时旧坐标立刻失效。`WriteOutcome.renamed` 字段返回 `(old, new)` 对，前端必须用返回值刷新缓存。所有持有旧坐标的长生命周期状态（undo 栈、路由历史）必须接受 renamed 通知更新。

4. **`ProjectSession` 变可写**：`SessionStore` 的 `RwLock` 中 `&mut` 写入若与读冲突，写入路径会阻塞读。当前 SessionStore 已隔离每 session 一把锁，影响可控。

5. **ts-rs 升级风险**：v10 是当前推荐版本；后续大版本变化可能影响生成产物格式（需在升级 PR 中重新审 bindings）。

6. **写入事务 + 维度 regenerate**：`engine.write_field` 内部需要 rebuild，rebuild 会触发 `regenerate_dimension_sources` 重新刷盘 default 列。这是预期行为（spec 16 §8.1），需在 `engine/writes.rs` 单元测试覆盖写入 variant 列后 default 不被错误刷写。

7. **`DimensionConfig.display_name` 默认值**：缺省时编辑器需 fallback；约定：`display_name = config.display_name.clone().or(builtin_name(name)).unwrap_or_else(|| name.clone())`，`builtin_name("language") = Some("本地化")`。

8. **`(actual_type, key)` 字符串脆弱**：任何归一化（大小写、trim）差异会导致解析失败。约定：wire 全程透传字符串，不做归一化；type 名永远 case-sensitive 比较。单元测试覆盖含空格/全角字符的 key。

9. **schema 重命名 type**：`actual_type` 字段变更等同于身份变更。前端应在收到 rebuild 后整体清缓存。文档中明确"schema 修改非 hot-reload 安全"。

### 12.3 测试覆盖增量

- `coflow-data-model`：`CfdValue` 纯化后 round-trip 测试；`ref_index` 填充覆盖（含跨 type 引用、循环引用）
- `coflow-engine`：`RecordIndex` 双键测试；写入事务三件套；file tree + dimension info；`WriteOutcome.renamed` 路径
- `cfd-editor/src-tauri`：bindings export 测试；wire round-trip 测试
- 前端：手动验证清单（本地化表 / 编辑 / 撤销 / 跳转 / graph / key 改名）

---

## 13. 未来扩展

- **LSP 宿主**：直接消费 `coflow-engine` 的索引和写入 API，无需重写
- **Web 编辑器**：bindings 可以编译期共享给 web 前端，wasm 化 engine 后 wire 协议不变
- **多 schema 维度交叉**：若引入 `Platform` 等新维度，`DimensionInfo / RecordIndex` 不需要调整（坐标化身份天然支持）
- **可序列化 session 快照**：当 wire 类型 = model 类型后，可以把整个 `ProjectSession` 序列化用于离线分析或多端同步（`CfdValue` 已纯化，无 id 障碍）
- **持久化 record id**：若未来确实需要稳定 GUID，可以在 `CfdRecord` 上加 `Option<RecordGuid>` 字段，作为更高优先级的身份；当前 `(actual_type, key)` 方案仍可作为 fallback

---

## 14. 错误码

本次重构不引入新的用户面错误码。所有现有错误码（`DIM-CONFIG-*`、`PROJECT-CONFIG-*`、`DIM-EVAL-*`、`DIM-SOURCE-*`、loader/writer 已有码）保持不变。

新增的内部错误（如 `EditorError::not_found("unknown record @Type.key")`）走 `EditorErrorKind::NotFound`，前端按 kind 路由。
