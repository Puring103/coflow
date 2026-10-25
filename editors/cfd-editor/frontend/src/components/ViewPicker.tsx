import { useRef } from 'react'
import type { ViewTab } from '../state/views'
import { Icon } from './Icon'

/** 所有视图平铺显示；指针拖动直接调整标签顺序。 */
export function ViewPicker({ tabs, activeId, plugins, activePluginKey, onSelect, onSelectPlugin, onCreate, onDelete, onReorder, onPreview }: {
  tabs: ViewTab[]
  activeId?: string
  plugins: readonly { key: string; title: string }[]
  activePluginKey?: string
  onPreview?: (tab: ViewTab) => void
  onSelect: (tab: ViewTab) => void
  onSelectPlugin: (key: string) => void
  onCreate?: () => void
  onDelete?: () => void
  onReorder: (ids: string[]) => void
}) {
  const drag = useRef<{ id: string; x: number; y: number; moved: boolean } | null>(null)
  const suppressClick = useRef(false)
  return <div className="view-picker" role="tablist" aria-label="视图">
    {tabs.map(tab => <button key={tab.id} className="btn view-picker-tab" role="tab"
      onPointerEnter={() => onPreview?.(tab)} onFocus={() => onPreview?.(tab)}
      data-view-id={tab.id} aria-selected={!activePluginKey && tab.id === activeId}
      onPointerDown={event => {
        if (event.button !== 0) return
        drag.current = { id: tab.id, x: event.clientX, y: event.clientY, moved: false }
        event.currentTarget.setPointerCapture(event.pointerId)
      }}
      onPointerMove={event => {
        const state = drag.current
        if (!state) return
        if (Math.hypot(event.clientX - state.x, event.clientY - state.y) < 4 && !state.moved) return
        state.moved = true
        const target = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>('[data-view-id]')
        if (!target || !event.currentTarget.parentElement?.contains(target) || target.dataset.viewId === state.id) return
        const ids = tabs.map(item => item.id).filter(id => id !== state.id)
        const index = ids.indexOf(target.dataset.viewId!)
        if (index < 0) return
        ids.splice(index, 0, state.id)
        onReorder(ids)
      }}
      onPointerUp={event => {
        suppressClick.current = drag.current?.moved ?? false
        drag.current = null
        event.currentTarget.releasePointerCapture(event.pointerId)
      }}
      onPointerCancel={() => { drag.current = null }}
      onClick={() => {
        if (suppressClick.current) { suppressClick.current = false; return }
        onSelect(tab)
      }}
      onKeyDown={event => {
        if (!event.altKey || !['ArrowLeft', 'ArrowRight'].includes(event.key)) return
        event.preventDefault()
        const ids = tabs.map(item => item.id)
        const index = ids.indexOf(tab.id)
        const next = index + (event.key === 'ArrowLeft' ? -1 : 1)
        if (next < 0 || next >= ids.length) return
        ;[ids[index], ids[next]] = [ids[next], ids[index]]
        onReorder(ids)
      }}
      title={`${tab.name}（拖动排序）`}>
      <Icon name={tab.kind === 'source' ? 'code' : tab.kind} size={14} /><span>{tab.name}</span>
    </button>)}
    {plugins.map(item => <button key={item.key} className="btn view-picker-tab" role="tab" aria-selected={item.key === activePluginKey} onClick={() => onSelectPlugin(item.key)}><Icon name="extensions" size={14} /><span>{item.title}</span></button>)}
    {onCreate && <button className="btn btn-icon" title="新建视图" aria-label="新建视图" onClick={onCreate}><Icon name="plus" size={14} /></button>}
    {onDelete && <button className="btn btn-icon" title="删除当前视图" aria-label="删除当前视图" onClick={onDelete}><Icon name="close" size={14} /></button>}
  </div>
}
