# CFD Editor — 待确认问题

## 实现过程中的设计选择

### 1. Int 类型改为 f64 传输
计划中 `FieldValue::Int { v: i64 }`，但 Tauri 2 IPC 走 `JSON.stringify`，`bigint` 不支持序列化。
已将 Rust 改为 `f64`，TS 改为 `number`。
对游戏数据的整数范围（通常远小于 2^53）没有影响。
**如果需要完整 64 位整数支持，需考虑字符串传输方案。**

### 2. refreshSnapshot 会创建新 session（已修复）
✅ 已实现 `close_session` 命令，`loadProject` 和 `refreshSnapshot` 都会在创建新 session 前关闭旧 session。

### 3. 嵌套 Object 整块写回
当用户编辑 `stats.hp`，`RecordView` 收到的是整个 `stats` 对象的新值，
span patch 会替换整个 `stats { ... }` 块（包括其他子字段）。
这意味着编辑子字段时，同一块的其他子字段也会被重新序列化（注释会丢失）。
**可以接受（已决定）：写回路径是顶层字段，整个 Object 值被序列化为 CFD 语法，注释会丢失。**

### 4. 记录视图侧边栏只显示当前文件的记录
✅ 已实现按类型过滤侧边栏：当文件有多种类型时，显示类型过滤 tabs；当前查看的记录始终保持可见（即使类型被过滤）。

### 5. Spread 语法在 patch 中不支持编辑
`CfdBlockEntry::Spread` 目前 patch 无法处理 spread 展开后的字段。
`locate_span_in_value` 只处理 `CfdValue::Block` 和 `CfdValue::Array`。
如果某个字段来自 spread，编辑会失败（会显示错误 toast）。
**可以接受：spread 字段是从其他记录继承的，编辑应该发生在源记录上。错误 toast 提示用户导航到源。**

### 6. create_file 路径限制（已修复）
✅ 已添加路径遍历保护：`create_file_inner` 会 canonicalize 目标路径并检查是否在项目目录内。

### 7. Dict key 编辑
✅ 已实现 Str 类型 dict key 的内联编辑：单击 key 标签可进入编辑模式，Enter 提交，Escape 取消。Int/Enum 类型 key 暂不支持编辑（游戏数据中这类 key 通常来自 schema 定义，不应手动修改）。

### 8. Ref 序列化格式（已修复）
✅ 之前错误地将所有 Ref 序列化为 `@Type.key`（当 target_file 非 null 时）。
实际上，CFD 中所有记录 key 在全项目内唯一，`&key` 语法可以跨文件引用。
现在始终序列化为 `&key`，与 CFD 解析器兼容。

### 9. Float 序列化精度（已修复）
✅ Rust `f64::to_string()` 对整数值不包含小数点（1.0 → "1"）。
CFD 解析器用小数点区分 int 和 float，所以 "1" 被解析为 int。
现在对所有 float 强制追加 `.0`（如果没有小数点或指数符号）。

### 10. 外部文件修改检测
当前没有文件 watcher 机制。如果外部工具（如 git checkout、其他编辑器）修改了 .cfd 文件，
编辑器不会自动重新加载。
**当前方案**：顶栏 "↺ Reload" 按钮可手动重新加载整个项目（关闭旧 session，重新解析所有文件）。
**结论**：手动刷新已实现；自动 watcher 暂不计划实现（复杂度高、收益有限）。

## 已知限制（可接受）

- **整数精度**：大于 2^53 的 i64 值通过 f64 传输会丢失精度
- **无离线写回缓冲**：所有写操作立即写盘，无 undo/redo
- **嵌套 Object/Array 写回是粗粒度的**：编辑子字段时整个父块会被重新序列化，注释丢失
- **Object 写回缩进**：`serialize_value` 使用固定 2 空格缩进，写回时可能不匹配原始文件缩进格式（CFD 解析不敏感缩进，功能正确但格式有时不美观）
- **自动保存策略**：写盘是即时的，"dirty" 只是 UI 等待 reload 的状态（1 秒防抖）
- **Spread 字段不可编辑**：来自 spread 的字段在 RecordView 和 TableView 中显示为只读（↗ 标记），应去源记录编辑
- **外部文件只读**：file_tree 显示 sources 外的文件但不可点击打开（禁用点击，50% opacity 提示）
- ~~**Enum 字段无下拉**：Table/RecordView 编辑 Enum 时需手动输入 variant 名，无 schema 驱动的下拉选择~~ ✅ 已通过 `get_enum_variants` + `EnumEditor` + `CellEditor` 实现下拉选择
