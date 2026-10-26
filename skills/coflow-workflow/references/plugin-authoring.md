# 编写和安装编辑器插件

适用于用户需要专用视图、类型呈现、页面、侧栏或命令。先确认目标类型与展示/编辑需求，再选一个扩展点。插件运行在编辑器中，CLI 没有插件管理命令。

## 文件与安装

在项目下创建 plugins/item-tools/plugin.json：

```json
{
  "id": "item-tools",
  "name": "物品工具",
  "description": "物品配置的专用视图",
  "version": "1.0.0",
  "entry": "plugin.js"
}
```

id 使用非空 ASCII 字母、数字、下划线或连字符，name 与 entry 非空。entry 为插件目录内相对 .js 路径，不得越出目录；依赖打包到单个 ES 模块，入口不使用相对模块导入。

在编辑器插件面板选择“添加项目插件”并选择清单，启用后在目标类型选择贡献视图。项目配置存于 editor-setting/plugins.json，manifest 相对项目根目录，团队共享时一并保存配置、清单和入口。不要为了交付代码擅自启动或重启用户的编辑器。

## 最小视图

以下插件要求项目存在 Item 类型：

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
        // 使用文本输出，避免配置内容被解释为 HTML。
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

activate 可异步，返回空值或含 dispose 的对象。mount 返回空值、清理函数或含 update/dispose 的对象。更新上下文后刷新展示，卸载后释放资源；异步回调先检查 outlet.signal.aborted。

## 按需选择扩展点

| register 方法 | 必要字段与行为 |
| --- | --- |
| view | id、title、types、mount；可设 includeFieldValues、readOnly、default |
| presentation | id、types、slot；cell/inspector 用 mount，summary 用 render(context): string |
| page / sidebar | id、title、mount；sidebar 可选图标 extensions/search |
| command | id、title、run(context)，可异步 |
| keybinding | command、key、可选 when(context) |

注册返回注销函数，贡献 id 在插件内唯一。通过 host.register.openPage/openSidebar 打开页面或侧栏。view/presentation 的 types 列出明确类型名。

view context 包含 identity、filePath、typeName、records、readOnly、openPage；记录包含 filePath、coordinate，includeFieldValues 才请求 fields。presentation context 包含 coordinate、actualType、fieldPath、declaredType、可选 value；根记录的 fieldPath 为 null。

page/sidebar/command 在没有项目时 identity 可为 null。命令可读取 filePath、typeName、selection、surface，并可 openPage。page context 提供 pageId、openPage、closePage；sidebar context 提供 sidebarId、openPage、openRecord(filePath, coordinate, fieldPath?)、closeSidebar。openRecord 的可选 fieldPath 是文本路径，数据写入路径则使用片段数组。

项目默认视图可在 editor-setting/plugins.json 的 defaults.views 按类型选择，例如 `"Item": "item-tools/item-list"`；默认呈现在 defaults.presentations 按类型及 cell/inspector/summary 槽位选择，值同样是“插件 ID/贡献 ID”。已有配置应保留其他插件项，贡献自身也可用 default 声明默认使用。

host.events.on 支持 project、data、selection、surface，返回取消订阅函数。卸载时清理自身定时器、事件和资源。

## 数据身份与修改

详细查询、写入和字段值格式读[插件数据接口](plugin-data.md)。它使用编辑器对象协议，CLI apply 的 model、path 结构不能直接传给 host.data。

写入前遵守 context.readOnly，确认查询快照的 sessionId/revision 仍属于当前项目。通过 host.data.mutate 提交，保留撤销、重做和编辑器更新流程；查询返回的快照仅用于读取。

## 验收

检查清单路径与单模块入口、实际类型匹配、更新和清理行为、只读视图以及写入失败处理。已有无界面测试环境时验证宿主调用；编辑器中加载、切换和撤销的人工验证单独报告完成情况。没有实际加载时明确说明“代码已完成，编辑器内运行尚未验证”。
