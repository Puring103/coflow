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
✅ 已实现 Str/Int/Enum 类型 dict key 的内联编辑：
- Str key：单击 key 标签可进入编辑模式，Enter 提交，Escape 取消
- Int key：同上，非整数输入静默还原
- Enum key：显示 `<select>` 下拉（使用 `get_enum_variants`），直接选择即可，无需确认

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
- ~~**无离线写回缓冲**：所有写操作立即写盘，无 undo/redo~~ ✅ 已实现客户端 undo 栈（最多 50 步），Ctrl+Z 撤销最近字段写入（write_field）；记录创建/删除/重命名/文件操作不在 undo 范围内
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
- ~~**rename_record 不更新其他文件中的 &ref 引用**：重命名记录后，其他文件中已有的 `&old_key` 引用成为悬空引用，需手动修复~~ ✅ `rename_record_inner` 重写：(1) 收集源文件内所有 `CfdValue::Ref` 中匹配 `old_key` 的 key span + 记录 key span，一次性替换并写回；(2) 遍历所有其他已加载文件，对各文件做同样的跨文件 ref span 替换。新增 `collect_ref_key_spans` 和 `apply_span_replacements` 两个辅助函数，并添加了 `rename_record_updates_cross_file_refs` 集成测试。
- ~~**GlobalTableView 缺少右键菜单**：其他视图（TableView/RecordView）均有右键菜单，GlobalTableView 行无任何右键操作~~ ✅ 已添加 ContextMenu：跳转到记录视图、在文件表视图中打开、复制 Key、复制为 CFD 源码。
- ~~**GlobalTableView 缺少键盘导航**：TableView 支持 ↑↓ 键导航，GlobalTableView 没有~~ ✅ 已添加 ↑↓ 键移动焦点行（高亮 accent 左边框），Enter 打开记录，Ctrl+F 聚焦搜索框，filteredRows 改为 useMemo。
- ~~**grouped 语法检测误判**：`source_has_grouped_header` 使用 `contains("TypeName {")` 误将 standalone 记录（如 `key: TypeName {`）识别为 grouped 块头，导致 `create_record`/`move_record`/`copy_record` 在含 standalone 记录的目标文件中错误插入 grouped 格式~~ ✅ 改为检测 `TypeName {` 前一字符是否为换行符（即 header 必须位于行首），新增 `source_has_grouped_header()` 辅助函数替代三处 `contains` 调用，并添加回归测试。
- **源码编辑器缺少键盘快捷提交**：RecordView 的源码编辑 textarea 和 TableView 的粘贴导入 textarea 现在支持 Ctrl+Enter 提交（等效于点击"保存"/"导入"按钮）。
- ~~**GlobalTableView 缺少跨文件批量编辑**：GlobalTableView 只能查看，不能批量写入~~  ✅ 已添加 checkbox 多选列、Ctrl+A 全选、底部批量写入栏（字段名 datalist + 值输入 + Enter 提交）；另增加 删除/移动/复制 右键菜单项。
- ~~**TableView 缺少 Ctrl+A 全选**~~ ✅ 已添加 Ctrl+A 选中所有可见行（通过 filteredRowsRef 避免 stale closure）。
- ~~**"在资源管理器中显示"仅在 FileTree 可用**~~ ✅ 已在 TableView、RecordView 侧边栏、GlobalTableView 行右键菜单中都添加了"在资源管理器中显示"。
- ~~**无单文件重新加载**：只有全量 Reload Project，外部修改单个文件需要全量刷新~~ ✅ 已添加 `reload_file_from_disk` Tauri 命令；FileTree 右键菜单新增"从磁盘重新加载"选项；调用后触发 markDirty 重建视图。
- ~~**RecordView 侧边栏底部"新建记录"按钮行为不一致**：点击后跳转到 TableView 而非直接打开创建弹窗~~ ✅ 改为直接 `setCreateModal(...)`，与顶部 ＋ 按钮和 Ctrl+N 行为一致。
- ~~**dirty indicator 标题误导**：tooltip 显示"Reloading…"实际含义是"存在未保存/待刷新变更"~~ ✅ 改为"Unsaved changes — reloading data…"。
- ~~**命令面板没有记录预览提示**：大量相似 key（如 enemy_001、enemy_002）无法区分~~ ✅ `RecordBrief` 新增 `display_hint: Option<String>`（优先取 name 字段，否则取第一个非空标量值），CommandPalette 在 key 和类型之间渲染灰色预览文本。
- ~~**粘贴导入多条记录时无反馈**：成功导入 N > 1 条记录时弹窗静默关闭，用户不知道导入了什么~~ ✅ N > 1 时保留弹窗并显示成功摘要 + 可点击的记录链接列表；N = 0 时改为显示错误提示而非静默关闭。
- ~~**GlobalSearch 不支持 file: 过滤**~~ ✅ `search_records_inner` 新增 `file:filename` 语法，匹配文件名（含子目录路径），GlobalSearch 占位文本更新。
- ~~**RecordView 无 F2 重命名快捷键**~~ ✅ 在 RecordView keydown 中添加 F2 触发 `setEditingKey(true)`，与资源管理器体验一致。
- ~~**GlobalTableView 无诊断 badge**：TableView/RecordView 显示 error/warning 数字 badge，GlobalTableView 行无诊断提示~~ ✅ 已添加 `diagnostics?` prop + `rowDiagCounts` useMemo，key 列右侧渲染红色 error 数和黄色 warning 数 badge。
- ~~**GlobalTableView 无 spread 字段标识**：用户不知道哪些字段是继承来的（只读）~~ ✅ 对 `row.spread_fields` 中的字段列应用 `opacity: 0.55`，tooltip 说明该字段来自 spread。
- ~~**GlobalTableView 空状态无提示**：类型无记录时显示空表格，无帮助信息~~ ✅ 当 `rows.length === 0` 时显示"项目中没有 X 类型的记录"提示。
- ~~**Clipboard/RevealInExplorer 失败静默**：所有视图中 clipboard.writeText 和 revealInExplorer 失败后 `.catch(() => {})` 静默忽略~~ ✅ 所有视图（GlobalTableView/TableView/RecordView）的 clipboard 和 revealInExplorer 失败均通过 `onError?.()` 显示 toast。
- ~~**TableView 无 Ctrl+D 快捷键**：RecordView 有 Ctrl+D 复制记录，TableView 只有右键菜单~~ ✅ TableView 添加 Ctrl+D 快捷键，优先复制第一个已选中行（否则复制第一行），打开 duplicate modal。
- ~~**GlobalTableView 无批量删除**：可以多选行但无批量删除功能（只有单行右键删除）~~ ✅ 批量操作栏新增"批量删除"按钮，点击弹出确认对话框，确认后顺序删除并汇报失败条目。
- ~~**无一键全部文件排序**：sort_file_records 只能对单个文件操作，多文件项目需逐个排序~~ ✅ 新增 `sort_all_files_inner` Rust 命令 + Tauri handler + API；顶栏新增"⇅ Sort All"按钮，一键对所有已加载文件排序；新增集成测试。
- ~~**RecordView 字段列表无法用键盘导航**：所有字段 tabIndex=-1，用户只能点击字段，Tab 键无法在字段间移动~~ ✅ 所有非 spread 字段行改为 tabIndex=0，Tab 在字段间顺序移动，Enter/Space 触发编辑（通过 `[data-edit-trigger]` 查找），DataCard 所有 Object/Array/Dict 折叠头也添加了 tabIndex=0 + Enter/Space 支持。
- ~~**TableView 单元格编辑不支持 Tab 跳列**：编辑单元格后需要点击下一列才能继续编辑~~ ✅ CellEditor 新增 `onTabCommit` 回调，Tab 键提交当前值并移动编辑焦点到同行下一个可编辑的非 spread 标量字段（Bool 列跳过），`handleCellTabCommit` 实现跳转逻辑。
- ~~**RecordView 侧边栏无"查找引用"入口**：需要手动查看记录详情页才能看到 incoming refs~~ ✅ 侧边栏右键菜单新增"查找引用此记录的记录"，点击后跳转到该记录并自动展开 incoming refs 面板（若有引用）。
- ~~**RecordView Object/Array/Dict 无全局展开/折叠**：需要逐个点击才能展开或折叠所有嵌套结构~~ ✅ 字段列表上方新增"▼ 全部展开"和"▶ 全部折叠"按钮（仅在有 Object/Array/Dict 字段时显示），通过 CollapseForceContext React context 批量控制所有嵌套 ExpandedValue 节点的折叠状态。
- ~~**DiagnosticsPanel 无键盘快捷键**：需要点击 PROBLEMS 标题才能展开/折叠面板~~ ✅ 新增 Ctrl+Shift+M 快捷键，通过 toggleRef 机制从 App.tsx 控制面板开关。
- ~~**GlobalTableView 无新建记录入口**：查看全局类型表时无法创建新记录，必须先切换到具体文件的 TableView~~ ✅ 新增"＋ New"按钮和 Ctrl+N 快捷键；弹窗让用户输入 key 并（多文件项目时）选择目标文件；创建后跳转到新记录的 RecordView。
- Ctrl+Shift+C 快捷键（复制当前记录 key）已添加到 RecordView 键盘处理器。
- ~~**GlobalTableView 无行内单元格编辑**：TableView 支持点击标量字段直接编辑，GlobalTableView 只读~~ ✅ GlobalTableView 现在支持点击标量字段（非 spread、非 Bool）进入内联编辑；Bool 字段单击直接切换；Enum 字段弹出下拉选择；Ref 字段支持带 datalist 建议的文本输入；Tab 键提交当前单元格并移到同行下一个可编辑字段。
- ~~**RecordView 文件名不可点击**：头部文件名为静态文字，无法快速跳转到对应 TableView~~ ✅ 文件名现在为带虚线下划线的可点击链接（`cursor: pointer`），点击跳转到 `view: "table"` 文件表视图。
- ~~**GlobalTableView 单元格无右键菜单**：无法快速复制字段值或跳转到 Ref 引用~~ ✅ 字段列右键点击显示上下文菜单：标量字段显示"复制值"，Ref 字段额外显示"跳转到引用记录"。
- ~~**GlobalTableView 无列显示/隐藏控制**：所有字段列始终显示，字段多时横向滚动体验差~~ ✅ 新增 ⊞ 列 按钮，点击弹出列选择面板，支持全显/全隐一键操作；勾选状态持久化到 `localStorage`（按 typeName 分 key）；隐藏列时显示 ● 标记；CSV 导出也遵循当前可见列。
- ~~**GlobalTableView 无粘贴 CFD 导入入口**：只有 TableView 有"粘贴 CFD 源码"功能~~ ✅ 工具栏新增 ⎘ 粘贴 CFD 按钮；弹窗支持多文件时选择目标文件；导入多条时显示结果列表可点击跳转；调用 `handleGlobalImportRecord` 触发 markDirty + graphRefreshKey。
- ~~**GraphView 节点右键菜单无"复制为 CFD 源码"**~~ ✅ 已添加"复制为 CFD 源码"菜单项，调用 `api.getRecordSource`。
- ~~**TableView/GlobalTableView 无必填字段行过滤**：RecordView 有 ⚠ 必填 toggle，表视图没有~~ ✅ 两个表视图均新增 ⚠ 必填 按钮，过滤出含空必填字段的行；GlobalTableView 还对必填空单元格高亮橙色边框/文字。
- ~~**无 F8 跳转到下一个问题**~~ ✅ 新增 F8 / Shift+F8 快捷键，在诊断列表中循环跳转到下一个/上一个可导航诊断（仅 .cfd 文件相关诊断）。
- **窗口标题不显示项目名** ✅ `document.title` 现在同步为 `project.yaml● — CFD Editor`（`●` 表示有未保存变更）。
- ~~**TableView/GlobalTableView 搜索不支持 field:value 语法**~~ ✅ 两个表视图的搜索框现在支持 `fieldname:value` 语法，GlobalTableView 还支持 `key:x` 和 `file:x`；placeholder 已更新提示用户。
- ~~**incoming refs 面板不显示来源类型**~~ ✅ 每条 incoming ref 现在显示 `key [type] .field_path`，便于区分不同类型的引用记录。
- ~~**FileTree 无键盘导航**：只能点击文件，无法用 ↑↓ 键在文件间移动~~ ✅ FileTree 容器添加 tabIndex=0 + keyDown 处理器；ArrowUp/Down 按可见文件顺序（考虑目录展开状态）移动选中文件。
- ~~**RecordView 字段搜索栏仅在字段 > 6 时显示**：字段少时即使有 fieldSearch（如 F8 导航）也不显示搜索框，⚠ 必填 按钮也不可见~~ ✅ 条件改为 `字段>6 || fieldSearch非空 || 有必填空字段`，确保所有场景下搜索栏和必填按钮都可访问。
- ~~**F8 导航后不自动滚动到目标字段**：navigating 到含 fieldSearch 的记录时，字段列表不滚动到匹配字段~~ ✅ `useEffect([recordKey, initialFieldSearch])` 中增加 `requestAnimationFrame(() => firstMatchRef.scrollIntoView(...))` 调用。
- ~~**Ctrl+Tab 无法在视图间切换**：用户需要点击 Table/Record/Graph tab 按钮，无键盘快捷方式~~ ✅ 新增 Ctrl+Tab / Ctrl+Shift+Tab 循环切换 Table → Record → Graph（当 Record 视图不可用时跳过）。
- ~~**Stats 面板类型/文件行不可点击**：显示统计数字但无导航功能~~ ✅ 类型行点击打开对应 GlobalTableView，文件行点击打开对应 TableView。
- ~~**RecordView 切换记录不回到顶部**：导航到新记录时主内容区保持原来的滚动位置~~ ✅ 添加 mainContentRef + recordKey change 时调用 `scrollTo({ top: 0 })`。
- ~~**GraphView 无类型过滤**：文件含多种类型时无法按类型筛选节点显示~~ ✅ 工具栏新增类型 chip 按钮，点击切换隐藏/显示；隐藏类型节点淡出到 10% 透明度，chip 显示删除线；切换文件时重置。
- ~~**DiagnosticsPanel 无文本搜索**：只能按严重度过滤，无法搜索特定消息或代码~~ ✅ 展开时显示搜索输入框，实时过滤 message/code/record_key/field_path；Escape 清空；关闭面板时重置。
- ~~**无 Ctrl+Y redo**：Ctrl+Z 撤销但无法重做~~ ✅ 新增 redo 栈（undo 时 push，redo 时 pop 回 undo）；任何新写入清空 redo；Ctrl+Y 和 Ctrl+Shift+Z 均触发 redo；顶栏新增 ↪ Redo 按钮；shortcuts tooltip 更新。
- ~~**批量操作无成功反馈**：TableView/GlobalTableView 批量写入/删除成功后静默清空，用户不确认操作是否完成~~ ✅ 批量写入/删除成功后显示绿色"✓ 已写入 N 条记录的 field 字段"/"✓ 已删除 N 条记录"，3 秒后自动消失；有错误时错误优先显示。
- ~~**RecordView 字段行无 Ctrl+C 快捷键**：复制字段值只能右键上下文菜单~~ ✅ 字段行键盘焦点时 Ctrl+C 直接复制标量值（Null→"null"，Bool/Int/Float→字面量，Str→字符串，Enum→variant，Ref→target_key）到剪贴板；非标量类型忽略。
- ~~**GraphView 无邻居焦点模式**：无法临时只看某节点的直接连接关系~~ ✅ 右键菜单新增"仅显示相邻节点"，激活后只有该节点及其直接邻居（入边+出边）保持不透明；工具栏显示"⊙ key × 焦点"芯片，点击取消；切换文件时重置。
- ~~**RecordView 侧边栏无记录预览摘要**：大量相似 key 无法区分~~ ✅ 每个侧边栏记录项下方显示小灰色字段摘要（优先 name 字段，否则首个非空标量值），与 CommandPalette display_hint 逻辑一致。
- ~~**GlobalSearch 无历史记录**：每次打开都从空白开始，需要重新输入上次的搜索词~~ ✅ 空查询时显示最近 10 条搜索记录（localStorage 持久化），点击历史项自动填充并重新搜索；每次导航到记录时保存搜索词。
- ~~**TableView 列宽不持久**：列宽调整后刷新/切换类型会重置~~ ✅ 新增 `ColumnSizingState` + `onColumnSizingChange` 连接到 TanStack Table；列宽保存到 `cfd-col-size:{type}` localStorage，切换类型时读取。
- ~~**新建记录 key 需要手动输入**：需要自己想一个唯一 key，不知道已有哪些~~ ✅ "新建记录"弹窗新增"✦ 建议"按钮，自动生成 `type_001`、`type_002` 风格的 key（CamelCase→snake_case，跳过已存在的 key）；TableView 和 GlobalTableView 均支持。
- ~~**复制记录弹窗无智能 key 建议**：RecordView/GlobalTableView 复制弹窗默认 `{key}_copy` 但不检查冲突~~ ✅ 两个视图的复制弹窗新增"✦ 建议"按钮，生成 `{srcKey}_copy_001` 风格的 key，跳过已存在的 key。
- ~~**TableView/GlobalTableView 无 Ctrl+Shift+R 快捷键**：切换必填字段过滤只能点击按钮~~ ✅ 两个表视图添加 Ctrl+Shift+R 键盘快捷键，与 RecordView 的 Ctrl+Shift+R（跳转到下一个必填空字段）保持语义一致；shortcuts tooltip 更新。
- ~~**RecordView 新建记录弹窗无文件选择**：多文件项目中只能在当前文件创建新记录~~ ✅ 当项目有多个文件时，新建记录弹窗显示"目标文件"下拉选择器（与 GlobalTableView 一致）；`availableFiles` prop 从 App.tsx 传入。
- ~~**GraphView 文件图例不可点击**：多文件项目中无法按文件过滤节点~~ ✅ 文件图例 chip 改为可点击按钮，点击切换隐藏/显示该文件的所有节点（与类型 chip 行为一致）；切换文件/session 时重置。
- ~~**FileTree 目录无批量展开/折叠**：需逐个点击才能展开或折叠子目录树~~ ✅ 目录右键菜单新增"展开所有子目录"和"折叠所有子目录"选项，递归操作整个子树。
- **列头 tooltip 显示字段类型**：TableView 和 GlobalTableView 列头 hover 时现在显示 `fieldName: type`，方便用户了解字段类型而无需打开列选择器。
- ~~**RecordView 侧边栏无诊断过滤**：侧边栏无法快速定位有问题的记录~~ ✅ 侧边栏搜索框旁新增 ⚠ 按钮（仅在有诊断时显示），点击后只显示有 error/warning 的记录（当前记录始终显示）。
- **全局操作成功反馈**：新增 `showOpSuccess` 绿色 toast（3 秒自动消失）；Sort All 操作现在显示成功摘要（"已对 N 个文件排序"或"已是字母顺序"）。
- ~~**GlobalTableView 选中行无 CSV 复制**：只有全量导出 CSV，无法只复制选中行~~ ✅ 批量操作栏新增"⎘ CSV"按钮，将选中行（可见列）复制为 CSV 格式到剪贴板。
- **incoming refs 最多显示 50 条**：对被大量引用的记录，incoming refs 面板现在最多显示 50 条并注明"… and N more"，避免性能问题。
- ~~**GlobalTableView 排序状态不持久**：切换类型后排序重置~~ ✅ 排序状态保存到 `cfd-global-sort:TypeName` localStorage，切换类型时恢复。
- ~~**TableView 排序状态不持久**：切换类型后排序重置~~ ✅ 排序状态保存到 `cfd-sort:TypeName` localStorage，切换类型时恢复。
- ~~**RecordView 侧边栏搜索无清除按钮**：需手动选择清空~~ ✅ 当搜索框非空时显示 ✕ 清除按钮。
- **RecordView 字段搜索匹配计数**：搜索字段时显示 `N / total` 匹配数量提示。
- ~~**TableView 无 F2 快速进入单元格编辑**：需鼠标点击~~ ✅ 表格容器 F2 键进入焦点行的第一个可编辑标量字段的编辑状态。
- ~~**GraphView 无 Ctrl+F 快捷键**：需鼠标点击搜索框~~ ✅ Ctrl+F 聚焦搜索框；Escape 清空并失焦；placeholder 更新提示。
- ~~**GlobalTableView 行右键菜单缺少"复制为 JSON"**~~ ✅ 新增 `fieldValueToJson` 辅助函数；行右键菜单添加"复制为 JSON"选项。
- ~~**ContextMenu 无键盘导航**：无法用 ↑↓/Enter 选择菜单项~~ ✅ ContextMenu 新增 `activeIdx` 状态，ArrowUp/Down 移动高亮，Enter 触发选项，鼠标移入也更新高亮。
- ~~**incoming refs 面板无 display_hint**：无法区分相似 key 的记录~~ ✅ 每条 incoming ref 旁显示灰色 display_hint（来自 `recordDisplayHint` 计算）。
- ~~**GlobalTableView 无 Delete 键删除**：需右键菜单才能删除~~ ✅ Delete/Backspace 触发单条记录删除确认弹窗（`singleDeleteModal`）。
- ~~**TableView 无 Delete 键删除**：需右键菜单才能删除~~ ✅ Delete/Backspace 键打开已有的删除确认弹窗（`setDeleteModal`）。
