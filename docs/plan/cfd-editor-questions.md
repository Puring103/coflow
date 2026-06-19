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

### 11. 重复 key 检测仅依赖 model（已修复）
✅ `create_record_inner`、`rename_record_inner`、`duplicate_record_inner` 的重复 key 检测
改用 `file_record_keys`（AST 索引），而不是 `model.records()`（仅含 model build 成功的记录）。
这确保在 model build 失败时（如有记录缺少必填字段），也能正确检测到跨文件重复 key。

### 12. GraphView 布局 race condition（已修复）
✅ `layoutGraph` 是异步的（ELK layout）；若 graphData 快速变化（如写入后刷新），旧的
layout promise 回来会覆盖新的。已在 useEffect cleanup 中添加 `cancelled` 标志。

### 13. 图形边去重 sort 缺失（已修复）
✅ `get_graph_inner` 使用 `labels.dedup()` 去重，但 dedup 只删除**相邻**重复项。
`labels.sort()` 现在在 `dedup()` 前执行，确保非相邻重复也被删除。

## 已知限制（可接受）

- **整数精度**：大于 2^53 的 i64 值通过 f64 传输会丢失精度
- **无离线写回缓冲**：所有写操作立即写盘，无 undo/redo
- **嵌套 Object/Array 写回是粗粒度的**：编辑子字段时整个父块会被重新序列化，注释丢失
- **Object 写回缩进**：`serialize_value` 使用固定 2 空格缩进，写回时可能不匹配原始文件缩进格式（CFD 解析不敏感缩进，功能正确但格式有时不美观）
- **自动保存策略**：写盘是即时的；`markDirty` 立即重新加载 fileRecords（消除 stale 显示），诊断检查（run_checks）防抖 1 秒后执行；`Ctrl+S` 立即刷新诊断
- **Spread 字段不可编辑**：来自 spread 的字段在 RecordView 和 TableView 中显示为只读（↗ 标记），应去源记录编辑。↗ 标记已可点击跳转到源记录（单一来源时直接跳转；RecordView 顶部也显示所有 spread 来源的可点击链接）。
- **外部文件只读**：file_tree 显示 sources 外的文件但不可点击打开（禁用点击，50% opacity 提示）
- ~~**Enum 字段无下拉**：Table/RecordView 编辑 Enum 时需手动输入 variant 名，无 schema 驱动的下拉选择~~ ✅ 已通过 `get_enum_variants` + `EnumEditor` + `CellEditor` 实现下拉选择
- ~~**无跨文件记录搜索**：只能在单个文件 TableView 内搜索~~ ✅ 已通过 Ctrl+P 命令面板实现全项目跳转
- ~~**Int dict key 无效输入无提示**：输入非整数字符串时 `parseInt` 返回 NaN，静默回退到 0~~ ✅ 已在 `DictEntry.commitKey` 中增加整数验证，非法输入静默还原（revert to original key）
- ~~**Array/Dict 新增条目默认值缺失**：`Array.defaultItem()` 和 `Dict.defaultVal` 对 `Array`/`Dict`/`Object` 嵌套类型返回 `Null` 而非正确的空容器~~ ✅ 已补充 `case "Array"`, `case "Dict"`, `case "Object"` 分支
- ~~**compact 模式 Array 内联渲染不显示 Ref**：Ref 数组项显示为 `…` 而非 `→key`~~ ✅ 已在 renderCompact 内联 parts 映射中补充 `case "Ref": return \`→\${item.target_key}\``
- ~~**TableView 中 Ref 字段点击只能跳转，不能编辑**~~ ✅ 已添加 Ref CellEditor（带 datalist 建议），点击打开编辑器；右键菜单保留"跳转到引用记录"
- ~~**空 target_key Ref 提交**：DataCard RefEditor 和 TableView Ref CellEditor 允许提交空 key，生成无效 `&` 语法~~ ✅ 前端已在 commit 前检查 `trimmed` 非空，否则 onCancel
- **React key 稳定性**：Array items 改用 `\`\${idx}:\${item.kind}\`` 作为 key；Dict entries 改用 `\`\${dictKeyStr(entry.key)}:\${idx}\`` 替代纯 idx，减少因删除中间项导致的状态复用问题
- ~~**可空 Object 字段无 UI 创建入口**：若 schema 中某字段类型为 `Stats?`（可空 Object），而当前值为 `null`，RecordView 中点击该字段会打开内联文本编辑器，但用户无法通过 UI 创建一个新的 Stats Object~~ ✅ 已通过 `get_field_schemas` 命令 + RecordView 获取字段 schema + DataCard 的 `nullableObjectType` prop 实现：当字段值为 null 且 schema 为 `T?`（T 为 Object 类型）时，RecordView 中显示"＋ T"按钮，点击写入空 Object（`{ kind: "Object", actual_type: T, fields: [] }`）。
- ~~**create_record / duplicate_record 写入格式与文件不一致**：当文件使用 grouped 语法（`TypeName { key { } }`）时，新建/复制记录错误地使用 standalone 语法（`key: TypeName { }`）追加，导致同一文件混用两种格式~~ ✅ 已检测 `type_span.start < key_span.start` 判断 grouped 格式，两个函数都将新记录插入到现有 group block 的 `}` 前；空文件回退到 standalone 语法
- ~~**空文件创建记录产生前导换行**：`create_record_inner` 的 `format!("{existing}\n...")` 在 `existing` 为空时产生开头多余的 `\n`~~ ✅ 已改为检测 `existing.ends_with('\n') || existing.is_empty()` 决定是否添加分隔换行
- ~~**TableView 列头只使用第一条记录的字段**：当不同记录的字段集合不同时（如 schema 演化后部分记录有新字段），TableView 只显示第一条记录的列集合~~  ✅ 已改为对当前 activeType 所有记录做字段名并集，按第一条记录顺序为主，额外字段追加在后
- ~~**Spread 字段 ↗ 标记不可点击**：RecordView 中 spread 字段标注 ↗ 但无交互，用户不知道去哪里编辑源记录~~ ✅ 已在 `RecordRow` 中新增 `spread_sources: Vec<SpreadSource>` 字段（`{key, file}` 结构，file 通过 `file_record_keys` 解析）；RecordView 头部显示来源列表（可点击跳转，使用正确 file path），单一来源时字段级 ↗ 也可直接点击跳转；跨文件 spread 导航已正确路由
- ~~**TableView Enum 编辑器加载失败卡死**：`getEnumVariants` 失败时 CellEditor 永远显示 "Loading…"~~ ✅ 改为 catch 时设置 `enumVariants = []`，空列表回退到文本输入框
- ~~**空/部分 inline Object 在 AST fallback 下不显示 schema 字段**：`convert_value` 和 `ast_value_to_field_value` 对 inline Object 使用 `filter_map`，导致 `Stats {}` 空块在 UI 中显示 0 个字段，用户无法编辑子字段~~ ✅ 改为 `map + unwrap_or(Null)` 与 `convert_record_row_with_ast` 一致，所有 schema 定义的字段都会显示（缺失的显示为 Null）；`ast_value_to_field_value` 也接受 `schema` 参数并做同样展开
- ~~**AST fallback 记录（model build 失败）在 UI 中无任何视觉区分**：用户不知道哪些记录是"不完整的"~~ ✅ `RecordRow` 和 `RecordBrief` 新增 `is_fallback: bool` 字段；RecordView 侧边栏、RecordView 头部（"⚠ incomplete" badge）、TableView key 列、CommandPalette 都对 fallback 记录显示 ⚠ 橙色警告
- **graph view 不显示 AST fallback 记录**：model build 失败的记录不出现在关系图中（这些记录没有解析好的 Ref，图中不会有意义的边）。可接受：图视图专注于已解析的数据关系。
- **RecordView 中必填字段高亮**：`has_default: false` 且当前值为 `Null` 的字段，字段名显示橙色并加 `*` 标记，提示用户填写。已通过 `fieldSchemas` + `isRequiredNull` 实现。
- ~~**嵌套 Object/Array 中的可空 Object 无 UI 创建入口**：`nullableObjectType` 仅传递到 RecordView 顶层字段，Object 子字段和 Array 元素中的 Null 项没有"＋ T"按钮~~ ✅ `DataCard.tsx` 新增 `useFieldSchemas` hook，Object 值展开时自动为每个子字段查找 `nullable_object_type`（来自该 Object 类型的 schema），并传入递归的 `ExpandedValue`；Array 中的 Null 项则通过推断（取现有 Object 元素的 `actual_type`）传入 `arrayElemObjectType`，使"＋ T"按钮在嵌套场景中也可用。
