# CFD Editor — 待确认问题

## 实现过程中的设计选择

### 1. Int 类型改为 f64 传输
计划中 `FieldValue::Int { v: i64 }`，但 Tauri 2 IPC 走 `JSON.stringify`，`bigint` 不支持序列化。
已将 Rust 改为 `f64`，TS 改为 `number`。
对游戏数据的整数范围（通常远小于 2^53）没有影响。
**如果需要完整 64 位整数支持，需考虑字符串传输方案。**

### 2. refreshSnapshot 会创建新 session
`create_file` 后调用 `refreshSnapshot()` 重新 `load_project`，这会分配一个新的 `session_id`。
旧 session 留在 `SessionStore` 内存中，不会被清理（只有进程重启才清）。
**是否需要 `close_session` 命令来释放内存？**

### 3. 编辑 nested Object 整块写回
当用户编辑 `stats.hp`，`RecordView` 收到的是整个 `stats` 对象的新值，
span patch 会替换整个 `stats { ... }` 块（包括其他子字段）。
这意味着编辑子字段时，同一块的其他子字段也会被重新序列化（注释会丢失）。
**可以接受？还是需要更精细的路径传递（把嵌套路径传到 write_field）？**

### 4. 记录视图侧边栏只显示当前文件的记录
如果从图视图跳转到另一个文件的记录视图，侧边栏会显示那个文件的所有记录。
这是正确行为。但如果那个文件有很多记录，侧边栏会很长。
**是否需要按类型过滤侧边栏？**

### 5. Spread 语法在 patch 中不支持编辑
`CfdBlockEntry::Spread` 目前 patch 无法处理 spread 展开后的字段。
`locate_span_in_value` 只处理 `CfdValue::Block` 和 `CfdValue::Array`。
如果某个字段来自 spread，编辑会失败。
**spread 字段需要支持编辑吗？**

### 6. create_file 总是在项目目录下创建，不限于 sources 目录
目前 `create_file_inner` 允许在项目目录任意位置创建文件（通过相对路径）。
`in_sources` 标志依赖 `source_dirs` 检测。
**是否应该限制只能在 sources 目录下创建？**

## 已知限制（可接受）

- **整数精度**：大于 2^53 的 i64 值通过 f64 传输会丢失精度
- **无离线写回缓冲**：所有写操作立即写盘，无 undo/redo
- **图视图无法展开折叠节点**：`is_collapsed: true` 的节点点击无法展开（需要新命令）
- **自动保存策略**：写盘是即时的，"dirty" 只是 UI 等待 reload 的状态，不是真正未保存
