# 维度与变体重构规格

**依赖文档**：[01-cft.md](01-cft.md)、[02-data-model.md](02-data-model.md)、[02-schema-api.md](02-schema-api.md)、[03-cell-value.md](03-cell-value.md)、[07-project-pipeline.md](07-project-pipeline.md)、[13-localization.md](13-localization.md)

本文档定义把现有 `@localized` 单一特性升级为通用 **维度/变体** 抽象的重构方案。重构后，本地化只是"语言维度"这一具体场景；schema、运行时、生成端、编辑器都按维度通用逻辑实现。本规格 **取代** 当前 `13-localization.md` 中所有运行时实现细节（注解语法面向用户的部分仍保留）。

本次重构是 **破坏性变更**：不保留旧 `LocalizationOverrides`、`localization` 配置块、CSV `id, default, lang*` 横向布局。Coflow 仍处早期版本，不提供兼容层。

---

## 目录

1. [设计目标与术语](#1-设计目标与术语)
2. [核心概念](#2-核心概念)
3. [项目配置](#3-项目配置)
4. [Schema 注解与字段定义](#4-schema-注解与字段定义)
5. [合成 Type](#5-合成-type)
6. [磁盘格式](#6-磁盘格式)
7. [运行时执行](#7-运行时执行)
8. [生成端](#8-生成端)
9. [编辑器](#9-编辑器)
10. [删除的旧基础设施](#10-删除的旧基础设施)
11. [实施 Phase 与验证标准](#11-实施-phase-与验证标准)
12. [未来扩展](#12-未来扩展)

---

## 1. 设计目标与术语

### 1.1 目标

- 把"本地化"提升为"维度/变体"的通用抽象，作为项目的核心特性之一
- 维度字段（如本地化字段）通过运行时合成的 schema type 表达"默认值 + 各变体值"，与普通 type 在编辑、加载、检查链路上一视同仁
- 删除所有 String-only 假设、覆盖式翻译表加载、特殊 `LocalizationOverrides` 类型，统一为"普通 source + 普通 record"模型
- 当前只暴露 `Localized`（语言）维度；架构层面预留多维度扩展点（如未来 `Platform`、`Difficulty`）

### 1.2 术语

| 术语 | 含义 |
|------|------|
| 维度（dimension） | 一个变体轴。当前内建 `Localized`（语言维度），预留 `Custom(String)` |
| 变体（variant） | 维度内的具体取值。`zh`/`en`/`ja` 是 `language` 维度下的变体 |
| 维度字段（dimensional field） | 被某维度切分的字段，通过 `@localized` 等注解声明 |
| 合成 type | 引擎运行时为每个维度字段构造的 schema type，描述 "default + 每个变体的取值"，存在于内存中、不写入用户 schema 文本 |
| 隐式 source | 引擎按维度配置的 `out_dir` 自动注册为 source 的文件，用户无需在项目 yaml 中手写 |

---

## 2. 核心概念

### 2.1 维度与变体的关系

```
dimension: Localized
  ├── variant: zh
  ├── variant: en
  └── variant: ja
```

变体是维度内部的具体取值。一个维度可以有任意多个变体，由项目配置声明。

### 2.2 字段如何与维度关联

源 schema 中某字段上挂注解（当前仅 `@localized`），表示该字段 **属于** 某个维度。引擎据此为该字段生成合成 type，并在运行时按变体读取对应取值。

```cft
type Item {
    name: string
    description: string @localized   // language 维度
}
```

含义：`Item.description` 是 language 维度字段，运行时按语言变体取值。

### 2.3 合成 type 的形态

每个 `(源 type, 源字段)` 对生成一个合成 type，结构固定：

```
type <SourceType>_<SourceField>Variants {
    default: T?
    <variant_1>: T?
    <variant_2>: T?
    ...
}
```

`T` = 源字段的类型；所有字段 nullable，null 时 evaluator 跳过该字段对应的检查。

合成 type 命名格式：`<Type>_<Field>Variants`，其中 `<Type>` 与 `<Field>` 直接使用源 type/字段名。

### 2.4 与磁盘的对应

合成 type 的 record 存储在 `dimensions["language"].out_dir` 下的文件中，按源 type 是否 singleton 选择两种格式：

- 普通 type → `<out_dir>/<type>_<field>.csv`（每行一个 record，record key = 原源 record 的 key）
- singleton type → `<out_dir>/<type>.cfd`（每个 field 一个 record，record key = field name，type = 该字段对应的合成 type）

这些文件通过引擎注册为 **隐式 sources**，加载链路与普通 source 完全一致。

---

## 3. 项目配置

### 3.1 配置形式

`coflow.yaml`：

```yaml
schema:
  - schema/main.cft

sources:
  - path: data/items.cfd

dimensions:
  language:
    variants: [zh, en, ja]
    out_dir: data/dimensions/language
```

### 3.2 字段规约

| 字段 | 类型 | 说明 |
|------|------|------|
| `dimensions` | `Map<String, DimensionConfig>` | 维度名 → 维度配置。当前内建键 `language` |
| `dimensions.<name>.variants` | `Vec<String>` | 变体名列表，须为合法 CFT 标识符，不重复，且不为 `default` |
| `dimensions.<name>.out_dir` | `PathBuf` | 该维度合成文件的输出目录，**无默认值**，必须显式配置；可为绝对路径或相对项目根目录 |

### 3.3 校验

- `dimensions` 顶层 key 缺省时引擎不构造任何合成 type；schema 中任意 `@localized` 字段在缺省 `dimensions.language` 时报错 `DIM-CONFIG-001`
- `language` 维度的 `variants` 沿用现有 `localization.languages` 全部约束：非空、合法标识符、不重复、不为 `default`，违反时报错 `DIM-CONFIG-002`
- 未知维度名（非 `language`）暂时静默接受但不生效；将来支持自定义维度后此行为变为正常配置
- `out_dir` 字段缺省报错 `DIM-CONFIG-003`

### 3.4 顶层 `localization` key 的处理

旧版 `coflow.yaml` 的 `localization:` 顶层 key 已被移除。检测到该键时反序列化必须 fail-fast：

```
PROJECT-CONFIG: `localization` has been removed; use `dimensions.language` instead.
```

不提供任何兼容读取层。

---

## 4. Schema 注解与字段定义

### 4.1 用户面注解

当前唯一支持的维度注解为 `@localized`，语法与现有完全一致：

```cft
type Item {
    description: string @localized
    icon: string @localized(bucket = "ui")
}
```

注解的语义升级为"该字段属于 `language` 维度"，bucket 含义不变（默认与 type 同名）。

### 4.2 内部表达

`coflow-cft` schema 内部表达调整：

```rust
pub enum Dimension {
    Localized,
    Custom(String),    // 预留，目前没有注解构造它
}

pub struct DimensionSpec {
    pub kind: Dimension,
    pub bucket: Option<String>,
}

pub struct CftSchemaField {
    // ...其他字段不变...
    pub dimension: Option<DimensionSpec>,
}
```

删除：
- `CftSchemaField.is_localized: bool`
- `CftSchemaField.localization_bucket: Option<String>`

编译器把 `@localized` 编译为 `dimension = Some(DimensionSpec { kind: Localized, bucket })`。

### 4.3 调用点替换规则

| 场景 | 旧写法 | 新写法 |
|------|--------|--------|
| 判断字段是否"维度字段" | `field.is_localized` | `field.dimension.is_some()` |
| 判断字段是否"语言维度" | `field.is_localized` | `matches!(field.dimension, Some(DimensionSpec { kind: Dimension::Localized, .. }))` |
| 读取 bucket | `field.localization_bucket.as_ref()` | `field.dimension.as_ref().and_then(\|d\| d.bucket.as_ref())` |

evaluator、checker 等运行时只能处理 `Localized` 维度，使用第二种显式匹配；codegen、editor、schema 检查等只关心"是否维度字段"的场景使用第一种。

### 4.4 程序化构造合成 type

`coflow-cft` 新增 API，允许引擎在不解析 cft 文本的前提下程序化注册一个 `CftSchemaType`：

```rust
impl CftContainer {
    /// Register a runtime-built type. Used by the engine to inject
    /// per-(type, field) variant container types for dimensional fields.
    ///
    /// # Errors
    /// Returns an error when a type with the same name is already present.
    pub fn register_runtime_type(&mut self, ty: CftSchemaType) -> Result<(), CftError>;
}
```

合成 type 通过该 API 注入容器。

---

## 5. 合成 Type

### 5.1 字段结构

对每个被识别为维度字段的 `(SourceType, source_field)`，引擎构造：

```
type <SourceType>_<source_field>Variants {
    default: <T>?
    <variant_1>: <T>?
    ...
}
```

其中：
- `<T>` 为源字段的 `ty_ref`（保持原类型，包括 `Nullable` 内部的实际类型）
- 字段顺序固定：`default` 第一，之后按 `dimensions["language"].variants` 数组顺序
- 所有字段类型为 `Nullable<T>`（null 在 evaluator 里有明确语义）

### 5.2 是否进入 schema 检查

合成 type 与用户声明的 type 完全等价，参与所有 schema 一致性检查、引用解析、检查器表达式求值。

### 5.3 合成 type 的 record key 约定

- 普通 type 的合成文件中，每行 record key = 源 record key（一对一映射）
- singleton type 的合成文件中，每个 record key = 字段名（每个 @localized 字段一条 record）

这两条约定保证 evaluator 能从源字段位置直接定位到对应合成 record。

### 5.4 合成 type record 的 `actual_type`

- 普通 type 合成文件中 record 的 `actual_type` = 该合成 type 名 `<SourceType>_<source_field>Variants`
- singleton type 的 cfd 文件中，每行 record 的 `actual_type` 同样是对应合成 type 名

---

## 6. 磁盘格式

### 6.1 普通 type：CSV

文件路径：`<out_dir>/<source_type>_<source_field>.csv`

CSV 字段：合成 type 的字段顺序，即 `default, <variant_1>, ...`。**额外的 `id` 列由 table-loader 当作 key 列处理**，跟普通 CSV source 完全相同。

示例（`language.variants = [zh, en]`）：

```csv
id,default,zh,en
sword,A sharp sword.,锋利的剑,A sharp sword
shield,A sturdy shield.,坚固的盾,A sturdy shield
```

每个单元格使用 `coflow-loader-table-core::cell_value` 现有 cell-value 语法（已支持 array/dict/enum/ref 等复杂值）；不引入新格式。

### 6.2 singleton type：CFD

文件路径：`<out_dir>/<source_type>.cfd`

每个 `@localized` 字段一条 record。例：源 schema 是

```cft
type @singleton UiText {
    welcome: string @localized
    farewell: string @localized
}
```

合成文件 `UiText.cfd`：

```cfd
welcome @UiText_welcomeVariants {
    default = "Welcome"
    zh = "欢迎"
    en = "Welcome"
}

farewell @UiText_farewellVariants {
    default = "Goodbye"
    zh = "再见"
    en = "Goodbye"
}
```

### 6.3 隐式 source 注册

引擎在 `build_project_session` 早期把 `out_dir` 下符合命名约定的文件作为隐式 sources 注入：

- 扫描 `out_dir`，按扩展名分发：`.csv` → csv loader；`.cfd` → cfd loader
- 不要求用户在 `sources:` 中显式声明这些文件
- 文件位置与 `sources:` 中的 path 同等地位，参与 `ProjectSession.files` 索引

### 6.4 默认 `actual_type` 推断

CSV loader 需要知道每行 record 的 `actual_type`。两种来源（按优先级）：

1. 项目 yaml 的 source options 提供 `type` 配置
2. 引擎注册隐式 source 时，options 自动填入 `type = <SourceType>_<source_field>Variants`（与文件名一致）

选 2，注册时直接写入，无需用户配置。

---

## 7. 运行时执行

### 7.1 evaluator 改造

evaluator 当前在求值字段时检查 `is_localized`，并按 String-only 替换。改造后：

- 字段值求值时，若 `field.dimension.is_some()`，进入"维度查找"分支
- 当前只支持 `Dimension::Localized`，其他 `Dimension::Custom(_)` 在该分支报错 `DIM-EVAL-001`（暂未实现）
- 在维度查找分支中：
  1. 当前迭代正在某个 variant 下（由 runner 设置）
  2. 从 model 查找对应合成 type record（按 6.3 / 5.3 的 key 约定）
  3. 取 record 的 `<variant>` 字段值
  4. null → 跳过该字段在本轮的相关 check（不报错、不替换）
  5. 非 null → 用该值替换源字段值参与求值

无须再调用 `CheckValue::String(_)` 模式匹配；typed 值直接进入表达式。

### 7.2 多轮 check

`coflow-checker` 删除 `run_checks_for_languages(overrides)`，新增：

```rust
pub fn run_checks_for_dimensions(
    schema: &CftContainer,
    model: &CfdDataModel,
    dimensions: &DimensionConfig,
) -> DiagnosticsStore;
```

行为：
- 默认值轮（无 variant）：对所有维度字段使用源字段值跑一遍
- 对 `dimensions["language"].variants` 中每个 variant 单独跑一轮，evaluator 在该轮把维度字段替换为对应 variant 的合成 record 值
- 各轮 diagnostics 合并入同一 store，diagnostic 上附 `variant` 上下文（消息体中标注 `[language=zh]` 等）

### 7.3 删除的 API

- `LocalizationOverrides` 类型
- `load_overrides_for_languages` 函数
- `run_checks_for_languages`
- evaluator 中 `if matches!(located.value, CheckValue::String(_))` 替换分支

---

## 8. 生成端

### 8.1 触发时机

`build_project_session` 在 schema 编译并构造合成 type 后，**调用一次生成端** 更新磁盘文件，再加载所有 sources（含隐式 source）。这保证：

- 用户修改源 record 的 default 值后，磁盘 default 列自动同步
- 用户在编辑器编辑某 variant 列后，下次构建仍保留其编辑

### 8.2 算法

对每个维度字段 `(SourceType, source_field)`：

1. 读取磁盘对应文件（普通 type csv / singleton cfd）
2. 解析为合成 type record 列表（按合成 type schema）
3. 用源 model 当前的 default 值刷新所有 record 的 `default` 字段；保留所有 variant 字段
4. 写回磁盘

如果文件不存在，按"全 null variant + 来自源 model 的 default"创建新文件。

### 8.3 子模块改造

`coflow-engine/src/localization/` 整体重命名为 `coflow-engine/src/dimensions/`，文件结构：

```
dimensions/
  mod.rs          // 入口、build_project_session 调用点
  synthesize.rs   // 构造合成 type、注入容器、注册隐式 source
  regenerate.rs   // 生成端：读 → 刷 default → 写
```

### 8.4 命名一致性

`generate_localization_tables` → `regenerate_dimension_sources`
`LocalizationConfig` → `DimensionConfig`
所有 `localization` 字眼在内部代码中替换为 `dimension`，用户面（注解、错误消息）保留 `localized` / `本地化`。

---

## 9. 编辑器

### 9.1 撤销当前临时实现

下列在前几轮提交里加入的临时代码被撤销：

- `__localization__` 虚拟路径前缀（包括 `LOCALIZATION_ROOT` 常量、`is_localization_path`）
- `coflow-loader-csv` 之外的所有 csv 单元格写回特例路径
- `SessionStore::get_localization_records` / `write_localization_field`
- `editor/session/localization.rs` 模块
- `FieldCell.read_only` wire 字段
- `SourceCapabilities::localization()` 工厂

合成 type 是普通 type，对应文件是普通 source；标准 `get_file_records` / `write_field` 路径足够。

### 9.2 文件树虚拟分组

引擎暴露 `out_dir` 列表（每个维度一个 `out_dir`）。编辑器在构造 file tree 时：

- 普通 sources 在主树正常显示
- 每个维度的 `out_dir` 提到顶层作为虚拟分组节点，节点名按维度对应中文展示（`Localized` → "本地化"）
- 主树 walkdir 跳过这些 `out_dir`，避免重复显示
- 节点 `path` 字段仍是真实路径（不是虚拟前缀），点击进入走标准 source pipeline

当 `dimensions` 中包含多个维度时，每个 `out_dir` 各占一个顶层组；当前仅 `language` 一个。

### 9.3 字段级只读

合成 type 的 `default` 字段在编辑器层硬编码只读：

- `TableView` / `RecordView` 在渲染单元格前检查 `record.actual_type` 是否以 `Variants` 后缀结尾 **且** 字段名为 `default`
- 满足时该单元格禁止编辑，并显示"由源记录决定，不可编辑"提示

不引入新的 wire 字段，纯前端规则。

---

## 10. 删除的旧基础设施

清单（按 crate 列出，便于追踪）：

**coflow-cft**
- `CftSchemaField.is_localized`
- `CftSchemaField.localization_bucket`

**coflow-project**
- `LocalizationConfig` 类型
- `ProjectConfig.localization` 字段
- 顶层 `localization:` yaml key 的反序列化路径

**coflow-checker**
- `LocalizationOverrides`
- `run_checks_for_languages`
- evaluator 的 `if matches!(.., CheckValue::String(_))` 分支
- `schema_view` 中所有按 `is_localized` 分流的代码

**coflow-engine**
- `localization/` 旧模块（重命名为 `dimensions/`）
- `load_overrides_for_languages`
- `merge_with_existing`
- `BucketKey` 等内部类型

**coflow-codegen-csharp**
- 所有 `is_localized` 调用点替换为 `dimension.is_some()`（语义不变，只换 API）
- 不在本次重构内做"按语言生成多套数据"，由 task #8 跟进

**editor**
- 上一节 9.1 列出的全部临时代码

---

## 11. 实施 Phase 与验证标准

每个 Phase 独立可提交，按顺序串行推进。

### Phase 0 — schema 字段重构

**改动**
- `coflow-cft/src/schema.rs`：新增 `Dimension` / `DimensionSpec`；改 `CftSchemaField`
- `coflow-cft/src/schema/compiler.rs`：把 `@localized` 编译为 `DimensionSpec`
- 全代码库 `is_localized` / `localization_bucket` 调用点替换（约 15 文件）
- 当前编辑器的 `is_localized` 用法同步更新

**验证**
- `cargo check --workspace` 通过
- `cargo test -p coflow-cft` 通过
- 现有 `tests/localization.rs` 暂时仍跑（行为不变，只是底层字段名换）

### Phase 1 — 项目配置改造

**改动**
- `coflow-project/src/lib.rs`：删除 `LocalizationConfig`、`ProjectConfig.localization`；新增 `DimensionConfig`、`ProjectConfig.dimensions`
- 校验逻辑迁移：`language` 维度沿用原 `localization.languages` 全部约束，新增 `out_dir` 必填校验
- 检测旧 `localization:` 顶层 key → fail-fast 报 `PROJECT-CONFIG-LOCALIZATION-REMOVED`
- 所有读 `project.config.localization` 的位置切换为 `project.config.dimensions.get("language")`

**验证**
- `cargo check --workspace` 通过
- 新增项目 yaml 测试覆盖：合法 `dimensions.language`、旧 `localization` 顶层 key、缺 `out_dir`、`variants` 不合法

### Phase 2 — 合成 type 注入 + 隐式 source

**改动**
- `coflow-cft`：暴露 `CftContainer::register_runtime_type`
- `coflow-engine/src/dimensions/synthesize.rs`：扫维度字段 → 构造 `<Type>_<Field>Variants` → 注入容器
- `coflow-engine`：注册 `out_dir` 下文件为隐式 sources（loader 按扩展名分发），options 自动填 `type`
- `build_project_session` 调整调用顺序：编译 schema → 合成 type → 注册隐式 source → 加载所有 sources

**验证**
- `cargo check --workspace` 通过
- 引擎集成测试：项目带 `@localized` 字段时，session.schema 含合成 type；session.files 含隐式 source

### Phase 3 — evaluator 改造 + 删旧基础设施

**改动**
- `coflow-checker/src/check/evaluator.rs`：维度字段求值切换为"查合成 type record + 取 variant 字段"
- `coflow-checker/src/check/runner.rs`：实现 `run_checks_for_dimensions`，按 variant 跑多轮
- 删除：`LocalizationOverrides`、`load_overrides_for_languages`、`merge_with_existing`、`run_checks_for_languages`、evaluator String-only 分支

**验证**
- `cargo test --workspace` 通过
- `tests/multi_language.rs` 等翻译相关测试改写为基于合成 type 的新模型
- 删除代码无引用残留

### Phase 4 — 生成端新格式

**改动**
- `coflow-engine/src/dimensions/regenerate.rs`：实现 `regenerate_dimension_sources`，生成 csv/cfd 新格式
- 不做迁移，旧 `<type>.csv` / `<type>_<field>.csv`（旧布局）不再读取
- `build_project_session` 调用：合成 type 注入后调用一次，再加载 sources

**验证**
- `cargo test --workspace` 通过
- 集成测试：首次跑生成出符合新格式的文件；编辑某 variant 列后再跑保留该列、刷新 default 列

### Phase 5 — 编辑器收尾

**改动**
- 撤销 9.1 列出的全部临时代码
- 实现 9.2 file tree 虚拟分组
- 实现 9.3 `default` 字段硬编码只读

**验证**
- 前端 `tsc --noEmit` 通过
- `cargo build` 通过
- 手动验证：本地化文件出现在顶层"本地化"分组；点击进入显示正常 TableView；编辑 variant 列写回成功；default 单元格不可编辑

### Phase 6 — codegen（task #8 跟进）

不在本次重构 scope 内。

---

## 12. 未来扩展

### 12.1 自定义维度

要支持新维度（如 `platform`），需要：

1. 在 `coflow-cft` 编译器添加新注解，例如 `@variant(platform)`，产出 `DimensionSpec { kind: Dimension::Custom("platform"), .. }`
2. 在 `coflow-project` 接受 `dimensions.platform` 配置
3. 在 `coflow-engine` 合成 type 注入逻辑无需改动（已通用）
4. 在 `coflow-checker` evaluator 解锁 `Dimension::Custom` 分支
5. 编辑器自动按 `out_dir` 注册新虚拟分组

### 12.2 多维度交叉

当前明确 **不支持** 一个字段标多个维度。未来若需要"语言 × 平台"交叉，方案是让用户通过嵌套对象组合表达，不在维度系统本身做笛卡尔积。

### 12.3 codegen 按 variant 生成多套数据

由 task #8 单独跟进。预想方向：exporter 端按 variant 生成多份产物（如 `items.zh.json`、`items.en.json`），运行时按当前语言加载对应文件。

---

## 13. 错误码

| 错误码 | 阶段 | 含义 |
|--------|------|------|
| `DIM-CONFIG-001` | project | schema 中存在 `@localized` 字段但 `dimensions.language` 未配置 |
| `DIM-CONFIG-002` | project | `dimensions.<name>.variants` 不合法（空、非标识符、重复、保留字 `default`） |
| `DIM-CONFIG-003` | project | `dimensions.<name>.out_dir` 缺失 |
| `PROJECT-CONFIG-LOCALIZATION-REMOVED` | project | 旧 `localization:` 顶层 key 已移除 |
| `DIM-EVAL-001` | check | evaluator 遇到 `Dimension::Custom(_)` 但当前未实现支持 |
| `DIM-SOURCE-001` | engine | 隐式 source 注册失败（`out_dir` 不存在且无法创建、扩展名未识别等） |

旧错误码 `LOC-IO-001`、`LOC-IO-003` 删除（不再有独立 IO 路径）。
