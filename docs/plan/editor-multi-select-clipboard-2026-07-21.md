# 表视图矩形选区与剪贴板实现计划

## 目标与边界

在 `cfd-editor` 表视图中实现类似 Excel 的矩形单元格选择，以及与系统剪贴板互操作的复制、粘贴、追加粘贴和剪切。

本次包含：

- 单击、拖动、Shift+单击和 Shift+方向键建立矩形选区。
- Ctrl+A 选择当前表格全部可见数据单元格。
- Ctrl+C 输出 TSV；整行选择仍输出一列 record ref。
- Ctrl+V 根据目标 schema 上下文解析，并通过一次批量 mutation 原子写入。
- Ctrl+Shift+V 向 array 目标追加值。
- Ctrl+X 在成功写入剪贴板后，原子清空可空的源单元格。
- 每次粘贴或剪切各形成一个撤销步骤。
- 记录选择、分组拖放和记录排序只从 Key 行头单元格启动。

本次不包含：

- Ctrl+点击形成离散单元格选区。
- 新增记录、自动生成 Key 或跨现有表格边界扩展数据。
- 自定义 JSON clipboard MIME；只写入和读取 `text/plain` TSV。
- 剪切整条记录。

## 选择模型与交互

### 状态模型

扩展 `ValueSelection`，以稳定的 `RecordCoordinate` 和顶层字段路径保存矩形两端，不保存可见行号，也不缓存完整 cell 集合：

```ts
interface CellAnchor {
  coordinate: RecordCoordinate
  fieldPath: FieldPathSegment[]
}

interface ValueSelection {
  kind: 'value'
  filePath: string
  coordinate: RecordCoordinate       // 当前焦点/活动端
  fieldPath: FieldPathSegment[]       // 当前焦点列
  rangeAnchor: CellAnchor             // 固定锚点
}
```

新增纯函数，根据 `visibleCoordinates` 和 `visibleFields` 计算选区边界并按行主序枚举单元格。所有表格多选只允许顶层字段列，Key 列不属于 value 矩形。

- 普通单击：锚点和焦点都设为当前 cell，得到 1x1 选区。
- 左键拖动：pointer down 设置锚点，越过拖动阈值后按命中的 cell 更新焦点；pointer up 固定矩形。
- Shift+单击：保留原锚点，将焦点移动到点击 cell。
- Shift+方向键：保留原锚点，将焦点移动一格并扩展或收缩矩形。
- 无 Shift 的方向键：将选择折叠成移动后的 1x1 cell；从 Key 行头向右进入第一个数据列，向左回到行头。
- Ctrl+A：value 模式下锚定左上角、聚焦右下角，选择全部可见数据 cell；record 模式保持现有的全选可见记录行为。
- 排序、筛选和分组折叠改变可见集合时，用稳定坐标重新计算矩形；若一个端点消失，则折叠到仍可见端点，两端都消失则清除选择。
- 记录重命名和删除时，`rebindSelection` / `removeSelection` 同时维护锚点与焦点。

选中 cell 使用现有 accent 色体系绘制半透明背景；矩形外边缘使用 2px 边框，内部 cell 不重复绘制粗边框。焦点 cell 保留 `aria-selected`，表格容器通过状态文本报告选区尺寸。

### 指针职责分离

新增表格 cell-range pointer controller，统一处理 pointer capture、拖动阈值、命中 cell 和靠近滚动容器边缘时的自动滚动。原生输入控件、下拉框和编辑态 cell 只响应单击选择，不启动拖选。

`useRecordPointerDrag` 保持记录拖放算法，但 TableView 只在 Key 行头 cell 上暴露 drag source 标记；数据 cell 不再能启动记录移动。行仍保留 drop target 标记，保证拖到目标记录和插入边缘的行为不变。RecordView 的现有拖动行为不受影响。

## 剪贴板规则与粘贴规划

### TSV

新增无 React 依赖的 `state/clipboard.ts`：

- `serializeCellMatrix` 按矩形行列顺序调用 `renderCellText`，以 tab 分列、LF 分行。
- 字段包含 tab、CR、LF 或双引号时用双引号包围，内部双引号写成 `""`。
- `parseTsv` 支持 LF/CRLF、引号字段、嵌入换行、尾部空 cell，并拒绝未闭合引号或引号后的非法字符。
- `serializeRecordsToRefColumn` 将可见顺序中的已选记录输出为每行一个 `&key`。

复制 value 选区时始终输出完整矩形；复制 record 选区时输出 ref 列。复制失败只显示错误，不改变选区或数据。

### 矩阵映射

解析后的源矩阵为 MxN，目标矩形为 RxC：

- 源为 1x1 时，广播到目标矩形全部 cell。
- 目标为 1x1 且目标是普通 scalar/ref/enum 等非复合字段时，以该 cell 为左上角，将源矩阵扩展到现有可见行列范围。
- 其他情况从目标矩形左上角开始按二维位置覆盖。
- 源小于目标时，未覆盖 cell 保持原值，不循环平铺。
- 源大于可用目标时截断，不新增字段或记录。
- 任一目标只读、不可写或解析失败时，整个操作失败且不写入。

映射结果统一转换为异构 `BatchWriteFieldInput[]`，先完成所有解析和校验，再提交一次批量 mutation。

### 上下文解析

普通目标 cell 直接调用现有 `parse_cell_text(coordinate, fieldPath, text)`，因此 string、number、bool、enum、ref、nullable、object、array 和 dict 都沿用 runtime 的 schema 规则。

当只选择一个复合 cell 且源矩阵不应展开为普通表格目标时，应用以下组装规则：

- array + 1x1：先按完整 array 字段解析；失败后在路径末尾追加虚拟 `index(0)`，按 item 类型解析并包装为单元素数组。
- array + 1xN 或 Nx1：每个源 cell 按虚拟 `index(0)` 的 item 类型解析，按源顺序组成 array。
- object + 1x1：先按完整 object 字段解析；若失败且 object 只有一个直接字段，则按该子字段解析并组装 object。
- object + 1xN：N 必须等于 schema 中全部直接字段的数量，按字段声明顺序逐列映射。字段不限 scalar；nested object、array 或 dict 在单个 TSV cell 中使用各自 CFD 值语法。
- array<object> + MxN：N 必须等于 item object 的全部直接字段数；每行组装一个 object，再组成 array。
- 其他无法无歧义组装的形状返回明确错误。

字段顺序不能依赖 JavaScript object key 的偶然顺序。扩展 editor wire annotation，显式提供 object 直接字段的 schema 声明顺序和实际/声明对象类型；array 的 `item_annotation` 同样携带 item object 的有序字段信息。该信息在 editor backend 的 annotation 转换处生成。

多态 object 必须由完整 cell 文本显式给出 concrete type；位置组装只有在 annotation 能确定唯一 concrete object type 时才允许，否则报错。

### 追加、剪切与行选择

- Ctrl+Shift+V 仅允许目标矩形中的所有被映射 cell 都是 array；否则报错，不退化为普通粘贴。
- 对每个 array 目标，按上述 array 规则解析输入，再读取当前 array 并追加；当前值为 null 时按空数组处理，非 array 当前值报错。
- Ctrl+X 仅支持 value 矩形。先完成 TSV 渲染并成功写入系统剪贴板，再确认全部目标可编辑且 nullable，最后以一次批量 mutation 写入 null。
- record 选择下 Ctrl+C 输出 ref；Ctrl+V、Ctrl+Shift+V 和 Ctrl+X 均提示该模式不支持，不隐式选择第一列，也不删除记录。

## 代码结构与接口调整

### 前端状态和纯函数

- `state/editorSelection.ts`：加入稳定矩形锚点、范围计算、可见集合归一化，以及重命名/删除处理；相应扩展 `tableCellNavigation.ts`。
- `state/clipboard.ts`：实现 TSV codec、矩阵映射、复合值组装和错误聚合。该模块只产生 write plan，不直接访问 clipboard 或提交 mutation。
- 新增表格 cell pointer controller；复用 `useRecordPointerDrag` 的清理模式，但保持两类拖动状态互不共享。

### 写入接口

将 `EditorMutations.writeFields(filePath, coordinates, fieldPath, newValue)` 保留给现有批量编辑器，并新增公开的异构批量入口，例如：

```ts
writeFieldBatch(
  filePath: string,
  writes: readonly BatchWriteFieldInput[],
): Promise<void>
```

该入口复制输入值后进入现有 mutation queue，并调用已有 `writeFieldsInternal(..., { recordHistory: true })`。`App.tsx` 向 TableView 传递 `onWriteFieldBatch`；TableView 不循环调用 `onWriteField`。后端已有 `write_fields`、停止写入错误和批量撤销数据结构，不新增 Tauri 写命令。

后端仅扩展 `FieldAnnotation` wire DTO 及转换逻辑以提供有序 object 字段元数据；生成 TypeScript bindings 后由前端消费。

### TableView 集成

- 接入矩形选择的 mouse/pointer/keyboard 状态和高亮。
- 在表格容器处理 Ctrl+A/C/V/Shift+V/X；native editing target 继续保留浏览器默认行为。
- 粘贴期间锁定重复提交；结束后保持选区和焦点，错误通过现有 `cellNotice` 展示。
- 批量成功后的 generation 更新、数据刷新、引用拓扑判断和 undo/redo 全部复用 `EditorMutations`。
- 将 record drag source 限制到 Key 行头，但保持整行 drop target 和当前排序/筛选下的重排限制。

## 测试与验收

### 单元测试

- `editorSelection.test.ts`：1x1、拖动矩形、反向拖动、Shift 连续扩缩、Ctrl+A、可见集合变化、重命名和删除端点。
- `tableCellNavigation.test.ts`：普通方向键折叠选择、Shift 方向键固定锚点、Key/data 列边界。
- `clipboard.test.ts`：TSV 引号/CRLF/空 cell/非法输入；矩阵广播、覆盖、保留、截断和单 cell 有界扩展。
- clipboard paste planner：混合 scalar 类型、ref/enum、nullable、完整/单 item array、object 有序字段、nested 字段、array<object>、append、只读目标和多错误聚合。
- `editorMutations.test.ts`：异构 batch 只发起一次后端调用、只记录一个 `batch-field` history、失败不产生 history。
- editor backend/runtime：annotation 字段顺序及继承字段顺序稳定；虚拟 array index 路径可在空数组上解析 item。

### 交互验收

- 在排序、筛选、分组折叠和虚拟滚动状态下拖选，选区只覆盖当前可见数据 cell，靠近边缘可以连续自动滚动。
- 从数据 cell 拖动只改变矩形选区；从 Key 行头拖动仍可执行记录分组和排序。
- 与 Excel/常见表格应用双向复制含 tab、换行和引号的字符串，矩形形状保持一致。
- 粘贴中任一 cell 解析失败时数据完全不变；成功后一次 Undo/Redo 覆盖整次操作。
- 剪切仅在 clipboard 写入成功且所有源 cell 可清空时执行，失败时源数据不变。

### 验证命令

不启动或关闭 CFD editor。实现完成后从相应目录运行：

```powershell
cd editors/cfd-editor/frontend
npm test
npm run build

cd ../../..
cargo check --workspace
cargo test --workspace
```

## 已确定的默认行为

- 单元格选择永远是一个矩形，不支持离散多区选择。
- 行头即 Key 列；记录移动只能从该列启动。
- 所有坐标使用稳定 `RecordCoordinate`，可见索引只参与即时范围计算。
- 粘贴只覆盖现有可见行列，不新增记录，不实现原 auto-expand。
- object 按全部直接字段的 schema 声明顺序映射，字段可以是复合类型。
- 复合值优先按完整目标类型解析，再按明确的 item/child 上下文回退。
- 所有多 cell 写入必须通过一次异构 batch 完成，并形成一个撤销步骤。
