# CFD 编辑器插件契约

## 宿主与加载

插件作为可信前端模块运行，在编辑器提供的页面、数据视图、侧栏和类型展示位置扩展界面。窗口、活动栏、文档标签及内置视图由编辑器管理。

插件的 `activate(host)` 接收 `EditorPluginHost`，包含三组接口：`register`、`events`、`data`。准确的 TypeScript 契约位于 [plugins/types.ts](../../editors/cfd-editor/frontend/src/plugins/types.ts)，宿主实现位于 [plugins/index.tsx](../../editors/cfd-editor/frontend/src/plugins/index.tsx)。

每个插件具有唯一 ID，贡献具有插件内 ID。宿主组合两者形成全局贡献身份。内置插件、全局插件与项目插件使用同一宿主；内置插件随编辑器提供，始终启用。项目插件配置保存清单相对路径、启用状态和默认贡献；全局安装由 Tauri 宿主管理。

插件按照确定的加载与注册顺序提供贡献。禁用、卸载或重新加载会清理贡献、事件订阅与已挂载实例。项目搜索是内置插件，通过查询、侧栏、命令与导航接口提供跨文件搜索。

## 注册与匹配

| 注册接口 | 所在位置与契约 |
| --- | --- |
| `register.page` | 中央文档区的独立页面；上下文提供页面打开与关闭 |
| `register.view` | 当前文件与明确记录类型的数据视图；`types` 指定目标，`includeFieldValues` 控制字段查询，`readOnly` 表达只读展示 |
| `register.sidebar` | 活动栏入口与左侧侧栏；上下文支持打开记录、定位字段、打开页面和关闭侧栏 |
| `register.presentation` | 按明确类型和 `cell`、`inspector`、`summary` slot 提供展示 |
| `register.command` | 具有标题和执行回调的命令 |
| `register.keybinding` | 指向同插件命令的按键及可选 `when` 条件 |

注册方法返回注销函数。`register.openPage` 和 `register.openSidebar` 打开当前插件的贡献。视图和类型展示要求非空的明确类型集合，不接受 `*`。

`cell` 用于表格顶层字段；`inspector` 支持记录根、顶层及嵌套字段；`summary` 返回折叠摘要文字。展示上下文携带当前修订、文件、记录坐标、实际类型、声明类型及结构化字段路径，记录根的路径为 `null`。

匹配的插件视图保留为用户可选项。类型展示按项目默认配置、贡献默认标记和稳定注册顺序选择有效项。按键执行前检查上下文，并避开普通文字输入和组合输入。

## DOM 出口与生命周期

页面、视图、侧栏以及 `cell`、`inspector` 展示通过 `mount(context, outlet)` 挂载。出口契约为：

```typescript
interface PluginOutlet {
  element: HTMLElement
  signal: AbortSignal
  replace(content: Node | string): void
}
```

插件通过 DOM 构造界面，并调用 `replace` 替换出口子内容。挂载可以返回清理函数，也可以返回带 `update(context)` 和 `dispose()` 的实例。宿主在上下文变化时调用 `update`，卸载时触发 `signal`、调用 `dispose` 并清空出口。插件负责将自身异步任务与事件监听绑定到这个生命周期。

`summary` 的 `render(context)` 是返回字符串的专用回调。其余贡献使用上述 DOM 出口。

## 数据快照与写入

`data.current()` 返回当前 `sessionId` 与 `revision`。查询包括 `getSchema`、`getRecordsByType`、`getRecord`、`getField` 和 `searchRecords`；结果为携带相同身份的 `PluginSnapshot<T>`，数据从宿主状态复制。异步查询完成时校验身份，过期结果被拒绝。

记录坐标由实际类型与记录 id 构成，文件路径作为来源和编辑位置另行传递。字段路径由字段名、数组索引和字典键片段组成。搜索支持 `id` 与 `full_text`，返回记录坐标、字段位置、摘要和截断标记。

`data.mutate` 接收结构化请求，支持单字段、多记录同字段、批量字段、集合编辑、记录重命名、新增、删除、交换、移动及跨文件转移。请求在数据 API 边界串行，并交给编辑器 mutation history；每个成功请求独立进入撤销历史，失败请求不创建历史项。返回值是发布后的会话与修订身份。

只读视图根据 `readOnly` 上下文提供只读交互。插件对项目数据的修改通过 `data.mutate` 提交，由宿主执行校验、写入、重载、诊断与发布。

## 事件与故障隔离

`events.on` 提供订阅与注销，事件包括项目身份变化、数据修订发布、选择变化，以及活动文档、视图和侧栏变化。撤销、重做和外部文件重载均通过数据修订事件反映。

宿主隔离清单加载、激活、挂载、更新、事件、命令和释放异常。贡献失败时记录错误并采用对应的内置展示；页面保留外壳并显示失败状态。同一插件的其他有效贡献可以继续工作。插件重新加载会重新建立贡献与界面实例。
