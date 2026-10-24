# 编辑器插件

插件可以为 CFD 编辑器添加专用视图、字段呈现、独立页面、侧栏和命令。插件与项目一起保存，团队成员可以使用同一套展示方式。

## 安装与项目配置

在编辑器插件管理中选择“添加项目插件”，选择插件的 `plugin.json`。插件列表支持启用、禁用和移除项目插件，也可以卸载已安装的全局插件。插件在编辑器中执行，请使用可信来源。

项目插件配置保存在 `editor-setting/plugins.json`：

```json
{
  "version": 1,
  "plugins": [
    { "id": "item-tools", "manifest": "plugins/item-tools/plugin.json", "enabled": true }
  ],
  "defaults": { "views": {}, "presentations": {} }
}
```

`manifest` 相对于项目根目录解析。共享项目时一并提交配置、清单和入口文件。`defaults.views` 按类型选择视图，`defaults.presentations` 按类型和 `cell`、`inspector`、`summary` 槽位选择呈现；配置值使用 `插件 ID/贡献项 ID`，例如 `"views": {"Item": "item-tools/item-list"}`；也可由贡献项的 `default` 指定默认使用。

## 清单与入口

`plugins/item-tools/plugin.json`：

```json
{
  "id": "item-tools",
  "name": "物品工具",
  "description": "物品配置的专用视图",
  "version": "1.0.0",
  "entry": "plugin.js"
}
```

`id` 使用非空 ASCII 字母、数字、下划线或连字符；`name` 与 `entry` 不得为空。入口为插件目录内的相对 `.js` 路径，不能通过 `..` 越出目录。将依赖打包到一个 ES 模块文件中，入口不使用相对模块导入。

入口必须默认导出 `activate(host)`，可以异步执行，返回空值或带 `dispose()` 的对象：

```js
export default function activate(host) {
  const unregister = host.register.view({
    id: 'item-list',
    title: '物品列表',
    types: ['Item'],
    readOnly: true,
    mount(context, outlet) {
      const element = document.createElement('pre')
      const update = next => {
        element.textContent = next.records.map(record => record.coordinate.id).join('\n')
      }
      update(context)
      outlet.replace(element)
      return { update }
    }
  })
  return { dispose() { unregister() } }
}
```

示例要求项目定义 `Item` 类型。切换到该类型的数据时可以选择“物品列表”视图。

## 扩展点

所有注册方法返回取消注册函数。贡献项 `id` 在插件内唯一；视图与呈现的 `types` 必须列出明确类型名。

| 方法 | 配置与行为 |
| --- | --- |
| `host.register.view` | `id`、`title`、`types`、`mount`；可设 `includeFieldValues`、`readOnly`、`default` |
| `host.register.presentation` | `id`、`types`、`slot`、可选 `default`；`cell` 和 `inspector` 使用 `mount`，`summary` 使用 `render(context): string` |
| `host.register.page` | `id`、`title`、`mount`；使用 `host.register.openPage(id)` 打开 |
| `host.register.sidebar` | `id`、`title`、`mount`，可选图标 `extensions` 或 `search`；使用 `host.register.openSidebar(id)` 打开 |
| `host.register.command` | `id`、`title`、`run(context)`；支持异步执行 |
| `host.register.keybinding` | `command`、`key`、可选 `when(context)`；绑定插件命令 |

视图上下文包含 `identity`、`filePath`、`typeName`、`records`、`readOnly` 和 `openPage`。每条记录包含 `filePath`、`coordinate`；需要字段值时开启 `includeFieldValues`。呈现上下文包含 `identity`、`filePath`、`coordinate`、`actualType`、`fieldPath`、`declaredType` 和可选的 `value`；根记录的 `fieldPath` 为 `null`。

页面上下文提供 `pageId`、`identity`、`openPage`、`closePage`；侧栏提供 `sidebarId`、`identity`、`openPage`、`openRecord`、`closeSidebar`。命令与快捷键条件可以读取当前 `identity`、`filePath`、`typeName`、`selection` 和 `surface`，命令还可调用 `openPage`。没有项目时 `identity` 可以为 `null`。

## 挂载与清理

`mount(context, outlet)` 使用 `outlet.element` 容器或 `outlet.replace(Node|string)` 显示内容。返回值可以为空、清理函数，或 `{ update(context), dispose() }` 对象。上下文变化时通过 `update` 刷新显示；卸载时释放定时器、事件与资源。异步任务检查 `outlet.signal.aborted`，避免卸载后写入界面。显示用户文本时使用 `textContent`。

`host.events.on(event, listener)` 返回取消订阅函数。事件包括 `project`（项目身份可为空）、`data`（新数据身份）、`selection`（当前选择）与 `surface`（当前文档、视图或侧栏）。停用插件时主机会清理注册项；插件自行创建的资源仍应在激活返回对象的 `dispose` 中释放。

## 查询与修改数据

`host.data.current()` 返回当前 `{ sessionId, revision }` 或 `null`。以下异步查询返回 `{ sessionId, revision, data }`，保留身份以判断结果是否仍属于当前项目与修订：

| 方法 | 用途 |
| --- | --- |
| `getSchema()` | 读取类型定义 |
| `getRecordsByType(typeName, { includeFieldValues })` | 按类型查询记录 |
| `getRecord(filePath, coordinate)` | 查询单条记录，找不到时数据为 `null` |
| `getField(filePath, coordinate, fieldPath)` | 查询字段，找不到时数据为 `null` |
| `searchRecords(query, { mode, limit })` | `mode` 为 `id` 或 `full_text`；数据包含 `hits` 与 `truncated` |

记录坐标为 `{ actual_type: 'Item', id: 'sword' }`。字段路径使用 `{ kind: 'field', value: 'name' }`、`{ kind: 'index', value: 0 }` 或 `{ kind: 'dict_key', value: '"key"' }`；字典路径的 `value` 是 CFD 键文本。插件路径采用编辑器对象格式，与 [CLI 变更集](./cli.md#批量修改与请求结构)分别使用各自的结构。

例如，响应用户操作修改物品名称：

```js
await host.data.mutate({
  kind: 'write_field',
  filePath: context.filePath,
  coordinate: { actual_type: 'Item', id: 'sword' },
  fieldPath: [{ kind: 'field', value: 'name' }],
  value: { kind: 'string', value: '长剑' }
})
```

写入前遵守视图的 `context.readOnly`，并确认目标属于当前项目。`mutate` 返回发布后的身份，失败时拒绝 Promise。通过该接口提交修改可保留编辑器的撤销、重做和数据更新流程；查询快照用于读取。

支持的修改种类：

| `kind` | 参数（除 `kind` 外） |
| --- | --- |
| `write_field` | `filePath, coordinate, fieldPath, value` |
| `write_fields` | `filePath, coordinates, fieldPath, value` |
| `write_field_batch` | `filePath, writes`，每项包含 `coordinate, field_path, new_value` |
| `edit_collection` | `filePath, coordinate, fieldPath, edit` |
| `rename_record` | `filePath, coordinate, newId` |
| `insert_record` | `filePath, recordId, actualType, fields` |
| `delete_record` | `filePath, coordinate` |
| `swap_records` | `filePath, first, second`，后两者为记录坐标 |
| `move_record` | `filePath, coordinate, targetIndex` |
| `transfer_record` | `sourceFile, destinationFile, coordinate, targetIndex` |

`edit_collection.edit` 支持 `array_append`（可选 `value`）、`array_remove`（`index`）、`array_move`（`from, to`）、`dict_insert`（`key`、可选 `value`）、`dict_remove`（`key`）。字典键对象为 `{ kind: 'string', value: 'key' }`、整数键或枚举键，沿用查询结果的键结构。

字段值使用带 `kind` 的对象，包括 `option_none`、`option_some`、`bool`、`int`、`float`、`string`、`formatted_string`、`enum`、`object`、`ref`、`array`、`dict`；`int.value` 为 JavaScript `bigint`。从查询结果保留复合值的结构，修改所需部分后通过写入接口提交。

可参考[化学式插件入口](https://puring103.github.io/coflow/examples/chemistry/plugin.js)为自定义数据类型添加单元格和检查器呈现。
