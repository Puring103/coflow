# CFD Editor — 实现计划

> 历史归档：本文记录早期独立 CFD Editor 原型设计，其中仍出现旧的 `@ref/@inline`
> 和 `field_modes` 元数据。当前实现以 schema 类型决定字段形态：`&Type` 表示 record
> reference，数据中使用 `&key`；普通 `Type` 表示 inline object，不再传输 field mode。

独立桌面编辑器，查看和编辑 Coflow 数据文件（CFD）。技术栈：Tauri 2 + React。一次性实现完整原型。

---

## 整体布局

```
┌─────────────────────────────────────────────────────────┐
│  CFD Editor    [Open Project…]    [←] [→]   ● unsaved  │
├──────────────┬──────────────────────────────────────────┤
│  文件树       │  [Table] [Record] [Graph]                │
│              │                                          │
│  📁 data/    │  （右侧内容随视图切换）                    │
│  ▶ item.cfd  │                                          │
│    npc.cfd   │                                          │
│    grey.cfd  │  ← 不在 sources 内，灰色                  │
│              │                                          │
│  [+ 新建]    │                                          │
└──────────────┴──────────────────────────────────────────┘
│  诊断面板（底部，可折叠）                                 │
└─────────────────────────────────────────────────────────┘
```

---

## 代码结构

```
editors/cfd-editor/
├── Cargo.toml
├── tauri.conf.json
├── build.rs
├── src/main.rs                  # Tauri command wrappers（#[tauri::command] 不能跨 crate）
└── frontend/
    ├── package.json
    └── src/
        ├── main.tsx
        ├── App.tsx              # 根布局，顶部栏，左右分栏
        ├── router.ts            # 内存路由（history stack）
        ├── bindings/            # ts-rs 自动生成，提交到 repo
        ├── components/
        │   ├── FileTree.tsx
        │   ├── TableView.tsx
        │   ├── RecordView.tsx
        │   ├── GraphView.tsx    # React Flow + ELK 自动布局
        │   ├── DataCard.tsx     # 通用嵌套卡片
        │   └── DiagnosticsPanel.tsx
        └── hooks/
            ├── useProject.ts
            └── useRouter.ts

crates/coflow-editor-core/
└── src/
    ├── lib.rs
    ├── types.rs                 # #[derive(Serialize, TS)] 序列化类型
    ├── commands.rs              # SessionStore + 数据查询逻辑
    └── patch.rs                 # span patch 写回（基于 coflow-cfd AST）
```

---

## 依赖现有 crate 的方式

编辑器**不重新实现**任何已有逻辑，直接复用：

| 功能 | 用哪个 crate | 说明 |
|------|-------------|------|
| 项目配置解析 | `coflow-project` | `Project::open(yaml_path)` 处理 yaml、schema 路径规范化 |
| Schema 编译 | `coflow-cft` | `CftContainer::compile()` |
| 数据加载 | `coflow-loader-cfd` | `parse_cfd_input_records` → `CfdDataModel::build()` |
| 数据查询 | `coflow-data-model` | `CfdDataModel::records_of_type`、`record(id)` 等 |
| 运行时检查 | `coflow-checker` | `model.run_checks(&schema)` |
| AST（写回用） | `coflow-cfd` | `parse_cfd(source)` → `CfdAst`，schema-free，保留注释 |

`coflow-editor-core` 只负责两件事：
1. Session state（`SessionStore`，持有每个文件的 `CfdAst` + 原始文本 + `CfdDataModel`）
2. `patch.rs`：按 field_path 在 `CfdAst` 中定位 span，做字符串替换写回

---

## 打开工程

用户点击 `Open Project…`，文件选择器过滤 `.yaml/.yml`，选中 `coflow.yaml`。

后端 `load_project(yaml_path)` 流程：
1. 手动解析 `coflow.yaml`（用 `serde_yaml`，不用 `Project::open` —— 后者验证失败会直接 Err，编辑器需要允许有错误的工程打开）
2. 加载并编译 CFT schema → `CftContainer`；schema 编译失败时记录诊断，继续（schema 为空）
3. 扫描 sources 目录下所有 `.cfd`，逐文件：
   - `parse_cfd(source)` → `CfdAst`（存入 session，用于写回）
   - `parse_cfd_input_records(&schema, source)` → `CfdInputRecord`，记录 `file_path` 来源
4. 全部文件的 `CfdInputRecord` 合并构建一个全局 `CfdDataModel`（保证跨文件引用解析正确）
5. session 维护 `HashMap<file_path, Vec<record_key>>` 用于按文件过滤查询
6. `model.run_checks(&schema)` 收集诊断（失败也继续）
7. 递归扫描项目目录所有 `.cfd`，标记 `in_sources: bool`
8. 存入 `SessionStore`，返回 `ProjectSnapshot`（所有阶段的诊断都收集，不因错误中止）

---

## Rust 序列化类型（`types.rs`）

全部 `#[derive(Serialize, Deserialize, TS)]` + `#[ts(export)]`。

```rust
ProjectSnapshot {
    session_id: u32,
    file_tree: Vec<FileTreeNode>,
    diagnostics: Vec<DiagnosticItem>,
}

FileTreeNode {
    name: String,
    path: String,           // 相对项目目录
    is_dir: bool,
    in_sources: bool,       // false → 灰色
    children: Vec<FileTreeNode>,
}

DiagnosticItem {
    severity: String,       // "error" | "warning" | "info"
    code: String,
    stage: String,
    message: String,
    file_path: Option<String>,
    record_key: Option<String>,
    field_path: Option<String>,
}

FileRecords {
    file_path: String,
    type_names: Vec<String>,    // 该文件中出现的类型，保持文件顺序
    records: Vec<RecordRow>,
}

RecordRow {
    key: String,
    actual_type: String,
    fields: Vec<FieldCell>,
}

FieldCell { name: String, value: FieldValue }
FieldAnnotation {
    // historical: CFT @ref/@inline 字段形态限制；当前由 &Type / Type 决定
    field_mode: Option<"ref" | "inline">
}
FileRecords.field_modes: { [typeName]: { [fieldName]: "ref" | "inline" } }
GraphData.field_modes: { [typeName]: { [fieldName]: "ref" | "inline" } }

// #[serde(tag = "kind")]
FieldValue:
  Null
  | Bool        { v: bool }
  | Int         { v: i64 }
  | Float       { v: f64 }
  | Str         { v: String }
  | Enum        { enum_name: String, variant: String, int_value: i64 }
  | Object      { actual_type: String, fields: Vec<FieldCell> }
  | Ref         { target_type: String, target_key: String, target_file: Option<String> }
  | Array       { items: Vec<FieldValue> }
  | Dict        { entries: Vec<DictEntry> }

DictEntry { key: DictKey, value: FieldValue }
DictKey: Str { v } | Int { v } | Enum { enum_name, variant, int_value }

GraphData {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

GraphNode {
    id: String,             // "{file_path}::{record_key}"
    key: String,
    actual_type: String,
    file_path: String,
    in_focus_file: bool,    // 是否属于当前选中文件
    is_collapsed: bool,     // 超出深度的占位符节点，可请求展开
    fields: Vec<FieldCell>, // 用于 node 模式 DataCard（collapsed 时为空）
}

FieldPathSegment:
  | Field { name: String }
  | Index { i: usize }

GraphEdge {
    source: String,         // node id
    target: String,
    field_path: String,     // 来自哪个字段
}
```

---

## Tauri Commands

```
load_project(yaml_path: String)
  → Result<ProjectSnapshot, String>

get_file_records(session_id: u32, file_path: String)
  → Result<FileRecords, String>
  // 返回该文件所有记录，按原文件顺序，按类型分组
  // session 维护 HashMap<file_path, Vec<record_key>>，不修改现有 crate

get_record(session_id: u32, file_path: String, record_key: String)
  → Result<RecordRow, String>

get_graph(session_id: u32, file_path: String)
  → Result<GraphData, String>
  // 以该文件为起点，递归展开引用和被引用（跨文件），默认深度 3 层
  // 超出深度的节点作为折叠占位符返回（is_collapsed: true），前端可请求展开

write_field(session_id: u32, file_path: String, record_key: String,
            field_path: Vec<FieldPathSegment>, new_value: FieldValue)
  → Result<(), String>
  // FieldPathSegment: { kind: "field", name: String } | { kind: "index", i: usize }

create_record(session_id: u32, file_path: String, key: String, type_name: String)
  → Result<RecordRow, String>
  // append 到文件末尾，返回初始化的空 RecordRow

delete_record(session_id: u32, file_path: String, record_key: String)
  → Result<(), String>

create_file(session_id: u32, rel_path: String)
  → Result<FileTreeNode, String>
  // 在第一个 source 目录下创建空 .cfd，返回新的 FileTreeNode
```

---

## 编辑模型

### 内存缓冲区 + 防抖写盘

- 所有改动写入内存 `DirtyBuffer`（per-file），UI 立即反映
- 编辑停止 **1 秒**后自动触发写盘 + 重新解析
- 顶部显示 `●` 未保存标记，`Ctrl+S` 立即保存
- 切换文件时若有未保存内容，自动保存（不弹确认框）

### Span Patch 写回（`patch.rs`）

基于 `coflow-cfd::CfdAst`（`parse_cfd(source)` 返回，schema-free，字节 span）。

AST span 覆盖：

| 元素 | span |
|------|------|
| `CfdRecord` | `span`、`key_span`、`type_span` |
| `CfdField` | `span`、`name_span` |
| `CfdValue` 每个 variant | `.span()` 方法统一获取 |

写回流程（`write_field`）：
- **字段已存在**：在 `CfdAst` 中沿 `record_key` → `field_path` 遍历，定位目标 `CfdValue.span()`，替换为新值文本片段
- **字段不存在**（新增）：定位 `CfdRecord` 结束 `}` 的 span.start，在其前插入新字段行 `  name: value,\n`
- 写盘后重新 `parse_cfd` + `parse_cfd_input_records` 更新该文件的 AST 和全局 model

注释保留：span patch 只替换字段值范围，行内/行间注释不在替换范围内，自动保留。

新增记录（`create_record`）：序列化整条记录 append 到文件末尾（前置空行），字段暂不写入（全 null），用户编辑时逐字段触发 `write_field` 新增。

删除记录（`delete_record`）：用 `CfdRecord.span` 替换为空，同时清理前置空行。

---

## DataCard 组件

通用嵌套卡片，三种模式：

| 模式 | 用途 |
|------|------|
| `compact` | 表格单元格，单行，不展开 |
| `expanded` | 记录视图字段，可折叠嵌套，可编辑 |
| `node` | 图视图节点内容，紧凑多行 |

### Compact 摘要规则

| kind | 显示 |
|------|------|
| null | `—`（灰色） |
| bool / int / float / str / enum | 字面值 |
| ref | `→ key`（同类型）/ `→ Type.key`（跨类型），中间截断 28 字符，tooltip 完整路径 |
| object | `ActualType` |
| array ≤6 标量且 ≤100 字符 | `[1, 2, 3]` 展开 |
| array 其他 | `[T × N]` |
| dict | `{K: V × N}` |

### Expanded 嵌套规则

- 每层缩进 **10px**（最深 5 层 = 50px 总缩进）
- Object / Array / Dict 默认折叠，点击 `▶` 展开
- 超过 **5 层**后强制 compact，不再递归展开
- 字段名 + 值同行；object/array/dict 的 `▶ TypeName` 作折叠句柄
- 可编辑：标量点击进入 inline edit，失焦或 Enter 触发 `write_field`
- 历史方案中 schema 字段带 `@ref` 时隐藏“内联”按钮，带 `@inline` 时隐藏“->Ref”按钮，
  并通过 `field_modes` 传输限制。当前语义下 UI 应直接由 schema type 决定：`&T` 渲染
  record picker，`T` 渲染 inline object editor，数组和字典递归使用元素/value 类型。

---

## 表视图（TableView）

- 范围：当前选中文件的所有记录
- 按类型分 tab（该文件中出现的类型，保持文件顺序）
- TanStack Table v8 + `@tanstack/react-virtual` 行虚拟滚动
- 列从该类型第一条记录的 fields 推导
- key 列：monospace bold
- 字段列：`<DataCard mode="compact" />`
- 行右键菜单：「跳转到记录视图」「删除记录」
- 字段右键菜单（Ref）：「跳转到引用记录」
- 底部「+ 新建记录」→ 弹出输入框（key + 类型选择）

---

## 记录视图（RecordView）

- 范围：当前选中文件，侧边列表可切换同文件其他记录
- 主区域：`<DataCard mode="expanded" />`
- 字段右键菜单（Ref）：「跳转到引用记录」
- 字段 inline edit → 触发 `write_field`
- 顶部显示 record key + actual_type

---

## 图视图（GraphView）

- 库：**React Flow (`@xyflow/react`) + ELK (`elkjs`)** 自动布局
- 以当前选中文件所有记录为起始节点，递归展开引用边（跨文件）
- 反向引用（被引用）也展示
- 不同文件节点用不同颜色区分，当前文件节点加边框高亮
- 节点内容：`<DataCard mode="node" />`
- 节点右键菜单：「跳转到记录视图」「在表中查看」
- 支持框选、搜索高亮（按 key 或类型过滤）

---

## 路由模型

前端内存路由（history stack），不依赖浏览器 URL。

路由状态：
```ts
type Route =
  | { view: 'table';  file: string; typeFilter?: string }
  | { view: 'record'; file: string; recordKey: string }
  | { view: 'graph';  file: string }
```

导航入口：
- 文件树点击 → `push({ view: 'table', file })`
- 视图 tab 切换 → `replace({ ...current, view })`
- 右键「跳转到记录视图」→ `push({ view: 'record', file, recordKey })`
- 右键「在表中查看」→ `push({ view: 'table', file, typeFilter })`
- 顶部 `←` / `→` → history back / forward

点击本身只做选中高亮，不触发路由跳转。

---

## 关键依赖（参考文件）

- `crates/coflow-cfd/src/ast.rs` — `CfdAst`、`CfdField`、`CfdValue`（span patch 基础）
- `crates/coflow-cfd/src/lib.rs` — `parse_cfd(source)` 入口
- `crates/coflow-project/src/lib.rs` — `Project::open`、`ProjectConfig`
- `crates/coflow-loader-cfd/src/lib.rs` — `parse_cfd_input_records`
- `crates/coflow-data-model/src/model.rs` — `CfdDataModel`、`CfdValue`、`CfdRecord`
- `crates/coflow-checker/src/lib.rs` — `CfdCheckExt::run_checks`
- `examples/cfd/coflow.yaml` — 示例工程
