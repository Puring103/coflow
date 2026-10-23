import type { ViewTab } from '../state/views'
import { Menu } from './Menu'
import { Icon } from './Icon'

/** 内置视图、命名视图与管理操作分组，所有入口共享关闭和键盘行为。 */
export function ViewPicker({ tabs, activeId, plugins, activePluginKey, onSelect, onSelectPlugin, onCreate, onDelete, onPin }: {
  tabs: ViewTab[]
  activeId?: string
  plugins: readonly { key: string; title: string }[]
  activePluginKey?: string
  onSelect: (tab: ViewTab) => void
  onSelectPlugin: (key: string) => void
  onCreate?: () => void
  onDelete?: () => void
  onPin: () => void
}) {
  const active = tabs.find(tab => tab.id === activeId)
  const plugin = plugins.find(item => item.key === activePluginKey)
  const icon = plugin ? 'extensions' : active?.kind === 'source' ? 'code' : active?.kind ?? 'table'
  const renderTab = (tab: ViewTab) => <button key={tab.id} className="btn" aria-current={!plugin && tab.id === activeId ? 'true' : undefined} onClick={() => onSelect(tab)}>
    <Icon name={tab.kind === 'source' ? 'code' : tab.kind} size={14} /><span>{tab.name}</span>{!plugin && tab.id === activeId && <Icon name="check" size={13} />}
  </button>
  return <Menu className="view-picker" title={`切换视图 · ${plugin?.title ?? active?.name ?? '视图'}`} label={<><Icon name={icon} size={16} />{(plugin || !active?.isDefault) && <span>{plugin?.title ?? active?.name}</span>}<Icon name="chevron-down" size={12} /></>}>
    <div className="view-picker-menu">
      <div className="view-picker-group">{tabs.filter(tab => tab.isDefault).map(renderTab)}</div>
      {tabs.some(tab => !tab.isDefault) && <div className="view-picker-group"><span className="view-picker-heading">自定义视图</span>{tabs.filter(tab => !tab.isDefault).map(renderTab)}</div>}
      {plugins.length > 0 && <div className="view-picker-group"><span className="view-picker-heading">插件视图</span>{plugins.map(item => <button key={item.key} className="btn" onClick={() => onSelectPlugin(item.key)} aria-current={item.key === activePluginKey ? 'true' : undefined}><Icon name="extensions" size={14} /><span>{item.title}</span>{item.key === activePluginKey && <Icon name="check" size={13} />}</button>)}</div>}
      <div className="view-picker-group">
        {onCreate && <button className="btn" onClick={onCreate}><Icon name="plus" size={14} />新建视图</button>}
        {onDelete && <button className="btn" onClick={onDelete}><Icon name="close" size={14} />删除当前视图</button>}
        {!plugin && <button className="btn" onClick={onPin}><Icon name="arrow-up" size={14} />当前视图置顶</button>}
      </div>
    </div>
  </Menu>
}
