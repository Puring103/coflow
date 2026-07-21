# Editor 视图功能设计方案

## 一、存储

### 1.1 设置文件（按关注点拆分）
- 目录：`editor-setting/`（文件夹和文件都不带点前缀）
- 单一 `settings.json` 拆分为多个文件，各自独立读写：
  - `views.json` —— 自定义视图配置（含每视图列宽）
  - `record-groups.json` —— 记录分组
- **旧配置不兼容**：不读取、不迁移 `.coflow/editor.json`；直接采用新格式。用户既有的列宽 / 分组 / `graph_enabled_fields` 全部丢弃，从空配置开始
- 废弃 `graph_enabled_fields`：新语义"自定义图视图的字段"由 ViewConfig 承载，无对应迁移

### 1.2 数据结构
- `views`（`views.json`）：keyed by (filePath, actualType)，值为自定义视图列表
- `record_groups`（`record-groups.json`）：keyed by (filePath, actualType)，现状保留
- **列宽并入 ViewConfig**：不再有独立的 `table_column_widths`；每个表格视图（含默认表格视图）的列宽存在各自 ViewConfig 内

每个 ViewConfig 包含：
- id、name、kind（Table 或 Graph）
- 共通：group_filter（可选分组 id）—— 见 §4
- Table 专属：columns（有序字段列表）、column_widths（columnName → 宽度）
- Graph 专属：relations（关系列表）、fields（要显示的字段列表，选择逻辑同表格列，不限标量）

### 1.2.1 默认表格视图的列宽
- 默认表格视图不在 `views` 数组中，但仍需持久化列宽
- 用保留 id `"__default_table"` 在 `views.json` 里单独存一条"仅含 column_widths 的隐式 ViewConfig 存根"，或在结构上单列一个 `default_table_column_widths`（keyed by filePath, actualType）
- 实现取后者：结构更清晰，避免默认视图混入自定义 `views` 列表（见 §10 数据类型）

### 1.3 视图配置的 key
- 沿用 (filePath, actualType) 双层 key，与 workspace tab id 一致
- 同一 type 名出现在多个 .cfd 文件时视图独立

### 1.4 路由模型（Route / viewId）
现状 `Route` 只有 `view: 'table' | 'record' | 'graph'` 三种固定值，无法表达"同一 table 视图下的多个自定义实例"。改造为携带 `viewId`：

```
Route = { file, typeFilter, viewId }
```

- `viewId` 取值：`"__default_record"` / `"__default_table"` / 自定义视图的 uuid
- 视图的 `kind`（Record / Table / Graph）不再存在 Route 里，而是由 `viewId` 查 ViewConfig 得到（默认视图用保留 id 推断）
- back/forward 历史栈、tab 高亮均以 `viewId` 为准
- 影响面：`useRouter`、`App.tsx` 中所有 `router.push({ view: ... })` 调用点，以及 `preferredView` 状态（由 `'table' | 'record' | 'graph'` 改为按 viewId 记忆）

### 1.5 列宽归属
- 列宽是视图的一部分，不再是独立的顶层字典
- 自定义表格视图：`column_widths` 存在其 ViewConfig 内
- 默认表格视图：`default_table_column_widths` 按 (filePath, actualType) 存（见 §1.2.1）
- 删除自定义视图时其列宽随 ViewConfig 一并消失，无需额外清理（对比旧方案 §8.2）

---

## 二、视图类型

### 2.1 默认视图（隐式，始终存在，不可删除，不可配置）
- 每个 type 默认有一个记录视图和一个表格视图
- 默认视图显示完整信息（全部列、全部记录）
- 单例 type 只有记录视图，且隐藏左侧记录列表
- 默认视图不在 views 数组中存储
- 保留 id：默认记录视图 `"__default_record"`，默认表格视图 `"__default_table"`

### 2.2 自定义视图（用户创建，可命名，可编辑，可删除）
- 可创建类型：表格视图 和 图视图
- 记录视图不可创建
- 单例 type 不可创建任何自定义视图，不显示"新建视图"按钮

### 2.3 视图 tab 展示
- 格式：[图标] 视图名（图标区分 record / table / graph）
- 默认视图名固定（"记录" / "表格"），不可改
- 自定义视图名由用户命名
- 视图过多时复用现有 overflow 下拉机制

---

## 三、视图创建与编辑

### 3.1 创建流程
- 点击"新建视图"按钮，弹出对话框
- 对话框为单一界面（不分 tab），从上到下：
  - 视图名称、视图类型（表格 / 图）
  - 图视图：先勾选关系（作为图中的连线）
  - 显示字段 / 显示列：勾选并**直接拖动排序**（不使用上下箭头）
    - 图视图中已勾选为关系的字段不再出现在字段选择里
  - 分组过滤（可选，见 §4）
- 确认创建

### 3.2 编辑流程
- 在视图 tab 上右键，弹出菜单，选择"编辑"打开同一对话框
- 可修改字段选择 / 关系选择 / 分组过滤 / 视图名称
- 右键菜单同时提供"删除"等操作

---

## 四、各视图详细行为

### 4.0 分组过滤（两种自定义视图共通）
- 表格视图和图视图都可配置 `group_filter`
- 表格视图：仅显示某分组的记录（整体过滤，不是折叠/展开）
- 图视图：仅以某分组的记录作为根节点筛选（只筛根节点，关联出的其他节点不受限）
- 无分组时分组过滤选项不可用

### 4.1 自定义表格视图
- 仅显示所选列，按选中顺序排列
- 每视图独立列宽
- 可设置分组过滤（见 §4.0）
- Inspector：默认只显示该视图的可见列；面板右上角有按钮切换显示全部字段

### 4.2 自定义图视图
- 仅显示选中关系的边
- 卡片只显示所选字段（选择逻辑同表格列，不限标量；不显示完整字段）
- 可设置分组过滤，仅筛选根节点（见 §4.0）
- Inspector：选中节点后默认只显示该视图的可见字段；面板右上角有按钮切换显示全部字段

### 4.3 默认表格视图
- 显示全部列，默认顺序
- 显示全部记录
- Inspector 显示完整信息，右上角无切换按钮

### 4.4 默认记录视图
- 显示完整记录内容
- 单例 type 隐藏左侧记录列表
- Inspector 显示完整信息，右上角无切换按钮

### 4.5 默认图视图
- 无默认图视图：图视图只能作为自定义视图创建（表格/记录才有默认视图）
- 图内不再提供右上角的关系筛选浮层，也不保存该筛选；关系可见性完全由 ViewConfig.relations 决定

---

## 五、Inspector 面板

- 自定义视图下：默认只显示该视图的可见字段
- 默认视图下：显示完整信息
- 切换按钮在面板右上角标题区，每视图独立
- 切换后显示全部字段（含被隐藏的）

| 视图 | 可见字段 | 切换按钮 |
|---|---|---|
| 默认记录视图 | 全部字段 | 不显示 |
| 默认表格视图 | 全部字段 | 不显示 |
| 自定义表格视图 | 所选列 | 显示 |
| 默认图视图 | 全部字段 | 不显示 |
| 自定义图视图 | 所选字段 | 显示 |

---

## 六、单例类型

- 前端需要拿到 is_singleton 信息
- 单例 type 只显示默认记录视图
- 无左侧记录列表
- 不显示"新建视图"按钮
- 不显示表格视图 / 图视图 tab

---

## 七、UI 布局

视图 tab 位置变更：
- 从：topbar 中部
- 移到：文档 tab 下方、全局搜索框上方

新布局从上到下：
1. topbar（左侧项目名 → 构建按钮 → 后退/前进、撤销/重做）
2. 文档 tab 行
3. 视图 tab 行（含新建视图按钮）
4. 全局搜索框
5. 视图容器（TableView / RecordView / GraphView）
6. 诊断面板

构建按钮位置变更：
- 从：topbar 中部、视图 tab 之后（`App.tsx` btn-build）
- 移到：topbar 左侧、项目名之后（视图 tab 已从 topbar 移走，构建按钮不再跟随视图 tab）

视图 tab 行内容：
- 左边：默认记录视图 tab [图标] "记录"
- 左边：默认表格视图 tab [图标] "表格"
- 自定义视图 tab [图标] 视图名（按创建顺序排列）
- 右边："+" 新建按钮（仅非单例时显示）

---

## 八、开放问题决议

### 8.1 单例信息透出
- `FileTypeOption` 增加 `is_singleton: bool`，由后端 `Schema::singleton_types()` 填充
- 前端据此判断：只显示默认记录视图、隐藏左侧记录列表、隐藏"新建视图"按钮、不显示表格/图视图 tab
- 属前置依赖，最先实现

### 8.2 视图删除时的清理
- 列宽已内聚在 ViewConfig 内，删视图即删列宽，无需单独清理（见 §1.5）
- 若当前路由正停在被删视图，回退到默认表格视图

### 8.3 group_filter 引用失效
- 表格视图的 `group_filter` 指向的分组被删除后，视图回退为"显示全部记录"，不显示空表
- sanitize 阶段校验 `group_filter` 引用有效性（参照 `sanitized_record_groups` 的做法）

### 8.4 Inspector "显示全部字段" 开关
- 仅作为 session 内存态，不持久化到 settings
- 不进入 ViewConfig，避免额外字段与 sanitize 负担
- 每视图独立，切换视图时重置

### 8.5 保留 id 保护
- 用户创建的视图 id 禁止使用 `__` 前缀
- sanitize 阶段拦截 `__` 前缀 id，避免与默认视图保留 id（`__default_record` / `__default_table`）冲突

---

## 九、实施顺序

每步可独立提交：

1. 后端：`FileTypeOption` 加 `is_singleton`，透出 + regen binding（纯增量）
2. 后端：settings 数据结构改造（拆分 `views.json` / `record-groups.json`、`views` + `default_table_column_widths`、drop 旧格式与 `graph_enabled_fields`）+ sanitize + 测试
3. 后端：视图 CRUD Tauri 命令（增删改查 + 列宽写入）
4. 前端：`Route` 加 `viewId`，改 `useRouter` 和所有 push 点，默认视图跑通
5. 前端：视图 tab 行 UI（布局下移）+ 构建按钮移位 + 单例隐藏逻辑
6. 前端：新建/编辑视图对话框（tab 分步）+ 右键菜单
7. 前端：自定义表格/图视图渲染（字段/关系/分组过滤）+ Inspector 字段过滤与切换开关

---

## 十、详尽实现计划

> 落到具体文件、数据类型和函数签名。行号为撰写时快照，实现前以实际代码为准。

### 10.0 涉及文件总览

后端（`editors/cfd-editor/src-tauri/src/`）：
- `editor/types.rs` —— 数据类型（`FileTypeOption`、`EditorProjectSettings`、新增 `ViewConfig` 等）
- `editor/settings.rs` —— 读写 + sanitize（拆分文件、去迁移）
- `editor/session/mod.rs` —— session 方法（`snapshot_file_types` 加 singleton、视图 CRUD、设置读写）
- `lib.rs` —— Tauri 命令注册
- `bindings/*.ts` —— ts-rs 生成（`cargo test --features ts-export` 或既有 regen 流程）

前端（`editors/cfd-editor/frontend/src/`）：
- `wire.ts` —— `Route` 加 `viewId`
- `hooks/useRouter.ts` —— 无需大改（Route 是不透明载荷），但比较逻辑改看 viewId
- `api.ts` —— 视图 CRUD 调用，删除 `setGraphEnabledFields`、改造 `setTableColumnWidths`
- `App.tsx` —— 视图状态、tab 渲染、switchView、布局、单例分支
- `state/views.ts`（新增）—— 视图解析/默认视图/可见字段投影的纯函数 + 单测
- `components/ViewEditorDialog.tsx`（新增）—— 创建/编辑对话框
- `components/ViewTabContextMenu.tsx`（新增，或复用既有 context menu）
- `components/TableView.tsx`、`components/GraphView.tsx`、`components/InspectorPanel.tsx` —— 消费视图配置

---

### 10.1 后端数据类型（`editor/types.rs`）

```rust
// 保留 id 常量（settings.rs 或 types.rs 顶部）
pub const DEFAULT_RECORD_VIEW_ID: &str = "__default_record";
pub const DEFAULT_TABLE_VIEW_ID: &str = "__default_table";

// FileTypeOption 增量：透出单例
pub struct FileTypeOption {
    pub name: String,
    pub display_name: String,
    pub record_count: usize,
    pub is_singleton: bool,   // 新增
}

// 视图种类
#[derive(…, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind { Table, Graph }

// 单个自定义视图
#[derive(…, Serialize, Deserialize, TS)]
pub struct ViewConfig {
    pub id: String,                 // uuid，禁止 __ 前缀
    pub name: String,
    pub kind: ViewKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_filter: Option<String>,   // 分组 id，两种视图共通
    // Table 专属
    #[serde(default)]
    pub columns: Vec<String>,           // 有序字段
    #[serde(default)]
    pub column_widths: BTreeMap<String, f64>,
    // Graph 专属
    #[serde(default)]
    pub relations: Vec<String>,         // 关系字段路径
    #[serde(default)]
    pub fields: Vec<String>,            // 卡片显示字段（同表格列选择逻辑）
}

// 顶层设置（拆分后仍在同一结构里，但落到两个文件）
pub struct EditorProjectSettings {
    #[serde(default)]
    pub views: BTreeMap<String, BTreeMap<String, Vec<ViewConfig>>>,
    #[serde(default)]
    pub default_table_column_widths:
        BTreeMap<String, BTreeMap<String, BTreeMap<String, f64>>>, // file→type→col→w
    #[serde(default)]
    pub record_groups: BTreeMap<String, BTreeMap<String, Vec<EditorRecordGroup>>>,
    // 删除：table_column_widths、graph_enabled_fields
}
```

> 备注：`views.json` 持久化 `{ views, default_table_column_widths }`，`record-groups.json` 持久化 `{ record_groups }`。运行时仍合并成一个 `EditorProjectSettings` 返回给前端（前端一份 state），只是磁盘上分两文件。

### 10.2 后端读写与 sanitize（`editor/settings.rs`）

```rust
const VIEWS_PATH: &str = "editor-setting/views.json";
const RECORD_GROUPS_PATH: &str = "editor-setting/record-groups.json";
const MIN_COLUMN_WIDTH: f64 = 48.0;

// 读：两文件各自读，缺失即默认，合并返回。不再读 .coflow/editor.json
pub(super) fn read_project_settings(project_root: &Path)
    -> Result<EditorProjectSettings, EditorError>;

// 写：按字段拆两文件原子写（沿用 AtomicFile）
pub(super) fn write_project_settings(project_root: &Path, settings: &EditorProjectSettings)
    -> Result<(), EditorError>;

// sanitize 单个视图列表：
// - 去空 id / 去重 id / 拦截 __ 前缀 id
// - name trim + 截断
// - column_widths 走 sanitized_column_widths（复用）
// - group_filter 校验（引用无效则置 None）—— 需传入该 (file,type) 的合法分组 id 集合
// - columns/relations/fields trim + 去空 + 去重
pub(super) fn sanitized_views(
    views: Vec<ViewConfig>,
    valid_group_ids: &BTreeSet<String>,
) -> Vec<ViewConfig>;

// 复用现有：sanitized_column_widths、sanitized_record_groups
```

sanitize 关键规则：
- `id.starts_with("__")` → 丢弃该视图（保留 id 保护，§8.5）
- `group_filter` 不在 `valid_group_ids` → 置 `None`（§8.3）
- `kind == Table` 时忽略 relations/fields；`kind == Graph` 时忽略 columns/column_widths（或保留但不用，倾向清空以免脏数据）

### 10.3 session 方法（`editor/session/mod.rs`）

```rust
// snapshot_file_types：填 is_singleton
// 判定：session.queries().schema() 里该 type 是否 singleton
// 现有 ProjectQueries 无直接 is_singleton，需要加一个查询：
//   crates/coflow-runtime/src/query.rs:
//     pub fn type_is_singleton(self, type_name: &str) -> bool {
//         self.session.schema().resolve_type(type_name)
//             .is_some_and(|meta| meta.is_singleton)
//     }
//   （schema declarations 已有 is_singleton: bool，见 declarations.rs:33）
fn snapshot_file_types(session: &EditorSession)
    -> BTreeMap<String, Vec<FileTypeOption>>;   // map 时 is_singleton: queries.type_is_singleton(&name)

// 视图 CRUD（替代原 set_table_column_widths / set_graph_enabled_fields）
impl Sessions {
    // 覆盖式写入某 (file,type) 的自定义视图全集（前端持整份列表提交，最简单）
    pub fn set_views(&self, id: u32, file_path: String, actual_type: String,
                     views: Vec<ViewConfig>) -> Result<EditorProjectSettings, EditorError>;

    // 默认表格视图列宽
    pub fn set_default_table_column_widths(&self, id: u32, file_path: String,
        actual_type: String, widths: BTreeMap<String, f64>)
        -> Result<EditorProjectSettings, EditorError>;

    // 自定义表格视图列宽（就地更新某 view 的 column_widths）
    pub fn set_view_column_widths(&self, id: u32, file_path: String, actual_type: String,
        view_id: String, widths: BTreeMap<String, f64>)
        -> Result<EditorProjectSettings, EditorError>;

    // record_groups 保持现状（set_record_groups 不变，路径改到 record-groups.json）
}
```

> set_views 覆盖写：CRUD（增/删/改名/改配置）都由前端在内存里改整份 `Vec<ViewConfig>` 后整体提交，后端只负责 sanitize + 持久化。避免后端维护逐项增删接口。列宽因高频（拖拽）单独走 set_view_column_widths / set_default_table_column_widths，避免每次拖动提交整份视图列表。

### 10.4 Tauri 命令（`lib.rs`）

```rust
#[tauri::command] async fn set_views(session_id, file_path, actual_type, views, host) -> …;
#[tauri::command] async fn set_default_table_column_widths(session_id, file_path, actual_type, widths, host) -> …;
#[tauri::command] async fn set_view_column_widths(session_id, file_path, actual_type, view_id, widths, host) -> …;
// 删除：set_graph_enabled_fields；改造：set_table_column_widths → set_default_table_column_widths
// get_project_settings 不变（返回合并后的 EditorProjectSettings）
// 记得在 invoke_handler! 里更新注册列表
```

### 10.5 前端 wire / 路由（`wire.ts`、`useRouter.ts`）

```ts
// Route 统一携带 viewId；kind 通过 viewId 查视图配置得到
export type Route =
  | { view: 'record'; file: string; viewId: string; coordinate: RecordCoordinate }
  | { view: 'table';  file: string; viewId: string; typeFilter?: string }
  | { view: 'graph';  file: string; viewId: string; typeFilter?: string }

export const DEFAULT_RECORD_VIEW_ID = '__default_record'
export const DEFAULT_TABLE_VIEW_ID  = '__default_table'
```

- `useRouter` 内部不解释 Route，无需改；但 App 里所有 `router.push/replace({view:...})` 都要补 `viewId`（默认视图用保留 id，自定义视图用其 uuid）
- back/forward 天然按整条 Route 记忆，已含 viewId

### 10.6 前端视图纯函数（新增 `state/views.ts` + `views.test.ts`）

```ts
export interface DefaultViewMeta { id: string; name: string; kind: 'record' | 'table' | 'graph' }

// 给定 (file,type,isSingleton)，返回该 type 应展示的 tab 列表（默认 + 自定义，按顺序）
export function viewTabsFor(
  settings: EditorProjectSettings | null,
  file: string, type: string, isSingleton: boolean,
  graphSupported: boolean,
): ViewTab[]   // ViewTab = { id, name, kind, isDefault }

// 由 viewId 解析出视图配置（默认视图返回合成配置）
export function resolveView(
  settings: EditorProjectSettings | null,
  file: string, type: string, viewId: string,
): ResolvedView   // { kind, columns?, columnWidths?, relations?, fields?, groupFilter?, isDefault }

// 自定义视图下 Inspector / 卡片的可见字段集合（默认视图返回 undefined = 全部）
export function visibleFieldsFor(view: ResolvedView): Set<string> | undefined

// group_filter → 记录坐标过滤谓词（默认视图或无 filter 返回恒真）
export function groupFilterPredicate(
  view: ResolvedView, groups: EditorRecordGroup[],
): (coordinate: RecordCoordinate) => boolean

// 生成新视图 id（uuid，保证非 __ 前缀）
export function newViewId(): string
```

单测覆盖：默认视图合成、保留 id 不与自定义冲突、group_filter 失效回退、单例只出记录视图。

### 10.7 App.tsx 改动点

- `preferredView: 'table'|'record'|'graph'` → 记忆"上次选的 viewId"（或保留 kind 记忆 + viewId 二级），`switchView(view)` 改为 `switchView(viewId)`，从 viewId 反查 kind 决定 record/table/graph 分支
- `tableColumnWidths` memo：改为 `resolveView(...).columnWidths`（默认视图取 `default_table_column_widths`，自定义取 view 内）
- `tableOnColumnWidthsChange`：按当前 viewId 决定调 `set_default_table_column_widths` 还是 `set_view_column_widths`
- 删除 `graph_enabled_fields` 相关：`saveGraphFields`、`graphFieldsSaveSequence`、`settingsWithGraphFields`；GraphView 的 `enabledFieldsOverride` 改为来自 `resolveView(...).fields`
- 单例分支：`activeTypeOption.is_singleton` 为真时——仅渲染默认记录视图 tab、隐藏记录列表、隐藏"新建视图"按钮
- 视图 tab 行：从 topbar 中部（`App.tsx:1960` 一带）移到文档 tab 下方、搜索框上方；构建按钮（`App.tsx:1989`）移到 topbar 左侧项目名之后
- 视图删除后当前路由回退默认表格视图（§8.2）

### 10.8 对话框与右键菜单

- `ViewEditorDialog`：props `{ mode: 'create'|'edit', initial?: ViewConfig, availableFields, availableRelations, groups, onSubmit(view) }`
  - 内部 tab：类型 / 字段（拖拽排序，复用现有列排序交互如有）/ 关系（仅 graph）/ 分组
  - 提交时前端把新/改后的 ViewConfig 合入整份列表 → `api.setViews(...)`
- 右键菜单：视图 tab 上右键 → 编辑 / 删除（默认视图 tab 不弹或仅置灰）

### 10.9 组件消费

- `TableView`：新增/沿用 `visibleColumns?: string[]`（自定义视图传所选 columns，默认视图 undefined=全部）；`columnWidths` 来自 resolveView
- `GraphView`：`enabledFieldsOverride` 语义改为视图 `fields`；新增关系过滤 `visibleRelations?: string[]`；根节点按 `groupFilterPredicate` 过滤
- `InspectorPanel`：新增 `visibleFields?: Set<string>`（自定义视图）+ `showAll` 开关（session 态，§8.4），默认视图不传/不显示开关

### 10.10 测试

- 后端：`sanitized_views`（__ 前缀拦截、group_filter 失效、列宽下限）、读写两文件 round-trip、`type_is_singleton`
- 前端：`views.test.ts`（10.6 列出的用例）、`switchView` viewId 分支、单例隐藏

### 10.11 风险与顺序提醒

- Route 加 viewId 是最大扩散点，先在 §9 step 4 单独跑通默认视图（viewId=保留 id），再叠自定义视图，避免一次改爆
- ts-rs binding 每次改后端类型都要 regen，注意 `ViewKind` / `ViewConfig` 新文件纳入 `bindings/`
- 旧配置不兼容 → 首次运行老项目会"丢"列宽/分组，属预期；如需可在 §一补一行用户提示（可选）