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
**可以接受？还是需要更精细的路径传递（把嵌套路径传到 write_field）？**

### 4. 记录视图侧边栏只显示当前文件的记录
✅ 已实现按类型过滤侧边栏：当文件有多种类型时，显示类型过滤 tabs；当前查看的记录始终保持可见（即使类型被过滤）。

### 5. Spread 语法在 patch 中不支持编辑
`CfdBlockEntry::Spread` 目前 patch 无法处理 spread 展开后的字段。
`locate_span_in_value` 只处理 `CfdValue::Block` 和 `CfdValue::Array`。
如果某个字段来自 spread，编辑会失败（会显示错误 toast）。
**spread 字段需要支持编辑吗？**

### 6. create_file 路径限制（已修复）
✅ 已添加路径遍历保护：`create_file_inner` 会 canonicalize 目标路径并检查是否在项目目录内。

### 7. Dict key 编辑
✅ 已实现 Str 类型 dict key 的内联编辑：单击 key 标签可进入编辑模式，Enter 提交，Escape 取消。Int/Enum 类型 key 暂不支持编辑（游戏数据中这类 key 通常来自 schema 定义，不应手动修改）。

## 已知限制（可接受）

- **整数精度**：大于 2^53 的 i64 值通过 f64 传输会丢失精度
- **无离线写回缓冲**：所有写操作立即写盘，无 undo/redo
- **嵌套 Object/Array 写回是粗粒度的**：编辑子字段时整个父块会被重新序列化，注释丢失
- **自动保存策略**：写盘是即时的，"dirty" 只是 UI 等待 reload 的状态，不是真正未保存
- **Spread 字段不可编辑**：来自 spread 的字段编辑会失败并显示错误 toast
