# 编辑器插件设计

## 1. 目标

编辑器插件用于扩展 `cfd-editor` 的界面和交互。插件可以为明确的数据类型提供定制界面，
增加视图、页面和侧栏，并通过编辑器提供的 API 查询或修改项目数据。

插件系统只扩展编辑器，不扩展 CFT/CFD 语言、项目构建流水线、CLI、代码生成器或目标语言
Runtime。

设计目标如下：

- 插件通过稳定、受控的接口扩展编辑器，不依赖编辑器内部 React 组件或 Tauri command。
- 插件只能替换或扩展明确的类型、视图和页面，不提供匹配全部内容的注册方式。
- 插件查询的数据与当前项目 revision 对应，修改仍由编辑器核心验证和写入。
- 插件界面失效或运行异常时，编辑器自动恢复对应的内置界面。
- 插件贡献的页面、侧栏、视图和类型界面共享同一套数据与事件合同。

## 2. 整体结构

插件公开能力由三组 API 组成：

```text
Plugin Host
├── Registration API
├── Event API
└── Data API
```

- Registration API 注册界面、命令和快捷键。
- Event API 通知插件项目、数据、选择和活动界面发生变化。
- Data API 提供 schema、记录和字段查询，并为可编辑界面提供结构化修改入口。

插件加载、容器生命周期、错误隔离、默认项解析、请求取消和故障回退由编辑器宿主管理，
不形成独立的插件 API。

## 3. 界面层级

编辑器向插件开放四个明确的界面层级：

```text
侧栏      活动栏入口对应的左侧栏内容
页面      中央文档标签对应的完整内容
视图      当前文件和记录类型下的一个视图标签
类型界面  视图内部特定记录类型或字段类型的显示与编辑界面
```

各层级相互独立。类型界面不能替换视图，视图不能替换页面，页面不能替换编辑器外壳。

### 3.1 页面

插件页面作为不绑定文件的文档标签，显示在中央文档标签区域：

```text
[items.cfd / Item] [quests.cfd / Quest] [数值分析] [×]
```

打开插件页面后，编辑器保留顶部栏、活动栏、左侧栏、文档标签栏和宿主管理的底部区域。
页面占用中央内容区域，不显示文件记录视图使用的表格、记录和图等视图标签。

页面有两种注册方式：

- 增加一个独立插件页面，不替换任何内置页面。
- 替换一个具有明确 `pageId` 的内置页面。

独立插件页面可以从文档标签栏的页面添加入口、插件侧栏、注册命令或快捷键打开。

### 3.2 视图

插件视图属于当前文件和记录类型，通过视图标签栏的加号选择。加号列出当前上下文中
匹配的插件视图，不匹配当前目标的插件视图不显示。

视图替换必须指定明确的 `viewId`。插件不能注册匹配全部视图的替换项。

### 3.3 侧栏

插件可以注册活动栏入口及其对应的左侧栏内容。侧栏适合提供数据树、分析结果目录、
收藏记录、快速筛选和页面导航。大型表格、关系图和完整分析结果应通过插件页面显示。

插件侧栏的切换不改变中央区域当前打开的文档或插件页面。

### 3.4 类型界面

类型界面可以针对以下明确目标注册：

- 记录类型，例如 `Quest` 或 `DialogNode`。
- 字段类型，例如 `Color`、`AssetRef` 或具体集合类型。

类型名称必须明确给出。空类型列表和 `*` 不表示全部类型，也不允许作为注册目标。

## 4. 类型界面 Slot

类型界面通过 slot 区分同一个类型在不同编辑器位置中的表现。首批提供三个 slot。

### 4.1 `cell`

`cell` 只在表格单元格中生效，并且只适用于记录的顶层字段。

```ts
interface CellSlotContext {
  record: RecordHandle
  fieldPath: [string]
  declaredType: TypeRef
  value: CfdValue
  readOnly: boolean
}
```

`cell` 界面必须适应单元格的固定布局。复杂编辑可以请求宿主打开受控浮层或对话框，
最终修改仍通过 Data API 提交。

### 4.2 `inspector`

`inspector` 在记录视图的字段区域和右侧 Inspector 中生效，支持顶层字段和任意嵌套字段。

```ts
interface InspectorSlotContext {
  surface: 'record' | 'inspector'
  record: RecordHandle
  fieldPath: FieldPath
  declaredType: TypeRef
  value: CfdValue
  depth: number
  readOnly: boolean
}
```

插件可以同时支持两个 surface，也可以只支持其中一个。列表元素、字典键值、Option 内部值和
嵌套对象字段均通过 `inspector` 表达，不为这些结构建立单独 slot。

### 4.3 `summary`

`summary` 用于折叠容器标题中的文字显示，包括字段值和嵌套对象的摘要。

```ts
interface SummaryResult {
  primary: string
  secondary?: string
  tone?: 'default' | 'muted' | 'warning' | 'error'
}
```

`summary` 返回受控文本和有限的语义样式，不挂载任意界面组件，确保折叠容器的尺寸、排版和
交互保持稳定。

## 5. Registration API

Registration API 统一注册页面、视图、侧栏、类型界面、命令和快捷键：

```ts
interface RegistrationApi {
  registerPage(definition: PageDefinition): Disposable
  registerView(definition: ViewDefinition): Disposable
  registerSidebar(definition: SidebarDefinition): Disposable
  registerTypePresentation(definition: TypePresentationDefinition): Disposable
  registerCommand(definition: CommandDefinition): Disposable
  registerKeybinding(definition: KeybindingDefinition): Disposable
}
```

所有注册方法返回 `Disposable`。插件停用、卸载或重新加载时，宿主释放插件的全部注册项、
事件订阅、界面容器和后台请求。

### 5.1 精确注册目标

替换项必须指向明确目标：

```ts
type ReplacementTarget =
  | { kind: 'record-type'; typeName: string }
  | { kind: 'field-type'; typeName: string }
  | { kind: 'view'; viewId: string }
  | { kind: 'page'; pageId: string }
```

不提供全局目标、通配符目标或省略目标后匹配全部内容的行为。一个插件需要支持多个目标时，
应分别注册这些目标。

### 5.2 类型界面注册

同一个类型插件可以同时提供多个 slot：

```ts
host.registration.registerTypePresentation({
  id: 'example.color',
  target: {
    kind: 'field-type',
    typeName: 'Color'
  },
  slots: {
    cell: createColorCell,
    inspector: createColorInspector,
    summary: summarizeColor
  }
})
```

插件没有提供的 slot 继续使用内置界面。某个 slot 运行失败时，只回退该 slot 的内置实现，
不影响插件的其他界面贡献。

### 5.3 命令与快捷键

快捷键绑定到注册命令，不直接绑定插件函数：

```ts
const command = host.registration.registerCommand({
  id: 'example.balance.open',
  title: '打开数值分析',
  execute: () => balancePage.open()
})

host.registration.registerKeybinding({
  command: 'example.balance.open',
  key: 'Ctrl+Shift+B',
  when: 'projectOpen'
})
```

同一个命令可以被侧栏、页面、工具栏、菜单和快捷键调用。插件不能通过全局键盘事件绕过
宿主的按键解析。

快捷键支持编辑器提供的上下文条件，例如：

```text
projectOpen
editorFocus
sidebarFocus
activePage == example.balance
activeView == example.quest-graph
recordType == Quest
fieldType == Color
readOnly
```

宿主负责保留快捷键、插件快捷键冲突、输入控件焦点和插件停用后的快捷键清理。项目配置可以
覆盖插件注册的默认按键。

## 6. Event API

Event API 提供影响插件界面有效性的状态变化：

```ts
interface EventApi {
  onProjectChanged(listener: ProjectChangedListener): Disposable
  onDataChanged(listener: DataChangedListener): Disposable
  onSelectionChanged(listener: SelectionChangedListener): Disposable
  onActiveSurfaceChanged(listener: ActiveSurfaceChangedListener): Disposable
}
```

数据事件用于通知插件已有结果失效，不携带完整记录数据：

```ts
interface DataChangedEvent {
  revision: number
  affectedTypes: string[]
  affectedRecords: RecordHandle[]
  reason: 'mutation' | 'reload' | 'external-change'
}
```

插件收到事件后通过 Data API 按需重新查询。项目关闭、插件停用或界面销毁时，宿主终止对应的
`AbortSignal` 并释放订阅。

## 7. Data API

Data API 是插件访问 schema、记录和字段的唯一入口：

```ts
interface DataApi {
  readonly revision: number

  getSchema(): Promise<SchemaSnapshot>
  queryRecords(query: RecordQuery): Promise<RecordPage>
  getRecord(handle: RecordHandle): Promise<RecordDetail>
  getReferences(handle: RecordHandle): Promise<ReferenceSet>
  apply(request: MutationRequest): Promise<MutationResult>
}
```

插件不能直接读取或写入项目文件，也不能直接调用 Tauri command。

### 7.1 数据投影

是否包含字段值由每次数据查询决定，不固定在视图注册信息中：

```ts
api.data.queryRecords({
  types: ['Quest'],
  projection: 'summary'
})

api.data.queryRecords({
  types: ['Quest'],
  projection: {
    fields: ['name', 'reward.amount']
  }
})
```

`summary` 投影只返回记录类型和记录 key，适合只显示两列的记录视图：

```ts
interface RecordSummary {
  type: string
  key: string
}
```

完整字段和值按需获取。记录查询支持分页和字段投影，避免插件页面一次复制整个项目模型。

### 7.2 Revision 与记录句柄

所有查询结果对应一个明确的项目 revision。记录使用稳定句柄，不使用记录数组索引：

```ts
interface RecordHandle {
  source: string
  type: string
  key: string
}
```

项目重载后，旧句柄必须重新解析。句柄已经失效时返回明确的 `not_found`，不能指向另一条记录。

### 7.3 修改

可编辑插件通过 Data API 提交结构化修改，并携带读取数据时的 revision：

```ts
await host.data.apply({
  revision,
  operations: [
    {
      kind: 'setField',
      record,
      path: ['reward', 'amount'],
      value: 100
    }
  ]
})
```

编辑器核心负责 revision 冲突检查、类型与引用验证、候选模型构造、文件写入以及撤销和重做历史。
插件不能直接提交文件写入。

只读界面没有修改能力。宿主根据注册信息授予只读或可编辑的 Data API 能力，不能依赖插件自行
隐藏编辑入口来保证只读。

## 8. 选择、默认项与回退

插件视图是视图标签栏加号中的可选项。插件页面是文档标签区域中的独立内容，可以从页面添加
入口、侧栏、命令或快捷键打开。

项目可以为一个明确目标指定默认插件界面，项目配置覆盖插件注册时声明的默认设置。配置键同时
包含目标种类和目标标识，不能使用通配符：

```json
{
  "defaultPluginViews": {
    "record-type:Quest": "example.quest-table",
    "field-type:Color": "example.color-editor",
    "view:records.graph": "example.quest-graph",
    "page:project.data": "example.data-page"
  }
}
```

当同一个明确目标存在多个匹配项且没有项目指定项时，暂时使用稳定注册顺序中的第一个匹配项。
宿主不能使用异步脚本实际完成加载的先后顺序作为注册顺序。

插件界面加载失败、创建失败或发生未捕获运行错误时，宿主执行以下回退：

- 释放失败界面的容器、订阅和请求。
- 恢复同一目标对应的内置界面。
- 保留当前文件、类型、记录、字段和选择状态。
- 显示一次非阻塞错误信息。
- 本次会话不再自动激活该失败界面，用户可以手动重试。

回退不会跨目标寻找另一个插件，也不会影响同一插件注册的其他正常 slot、视图、页面或侧栏。

## 9. 正确性约束

- 注册目标必须明确，禁止匹配全部类型、全部视图或全部页面。
- 插件数据绑定项目 revision，过期修改不能在新 revision 上直接执行。
- Event API 只通知数据失效，当前事实始终通过 Data API 查询。
- 插件修改必须经过编辑器核心现有的结构化 mutation、验证和写入流程。
- 只读能力由宿主限制，不能只依赖插件界面约定。
- 插件界面不能直接访问编辑器 DOM、内部 React 状态或 Tauri command。
- 插件异常只能影响对应的插件界面，不能破坏编辑器 session 和项目数据。
- 页面、视图、侧栏和类型 slot 都具有独立生命周期和独立故障回退边界。
