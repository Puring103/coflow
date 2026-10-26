import { useSelectionSlide } from '../../../shared/motion/useSelectionSlide'
import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { ViewTab } from '../state/views'
import { Icon } from '../../../shared/ui/Icon'

/** 所有视图平铺显示；指针拖动直接调整标签顺序。 */
export function ViewPicker({ tabs, activeId, plugins, activePluginKey, onSelect, onSelectPlugin, onCreate, onDelete, onEdit, onReorder, onPreview }: {
  tabs: ViewTab[]
  activeId?: string
  plugins: readonly { key: string; title: string }[]
  activePluginKey?: string
  onPreview?: (tab: ViewTab) => void
  onSelect: (tab: ViewTab) => void
  onSelectPlugin: (key: string) => void
  onCreate?: () => void
  onDelete?: (id: string) => void
  onEdit?: (id: string) => void
  onReorder?: (ids: string[]) => void
}) {
  const rootRef = useRef<HTMLDivElement>(null)
  useSelectionSlide(rootRef, '[role="tab"][aria-selected="true"]')
  const drag = useRef<{ id: string; x: number; y: number; moved: boolean } | null>(null)
  const suppressClick = useRef(false)
  const [preview, setPreview] = useState<{ tab: ViewTab; x: number; y: number } | null>(null)
  const [order, setOrder] = useState<string[] | null>(null)
  const orderRef = useRef<string[] | null>(null)
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (drag.current || !order) return
    const ids = tabs.map(tab => tab.id)
    if (ids.join('|') === order.join('|') || ids.length !== order.length || ids.some(id => !order.includes(id))) setOrder(null)
  }, [tabs, order])
  useEffect(() => {
    if (!menu) return
    menuRef.current?.querySelector('button')?.focus({ preventScroll: true })
    const close = (event: Event) => { if (!menuRef.current?.contains(event.target as Node)) setMenu(null) }
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') setMenu(null) }
    window.addEventListener('pointerdown', close, true)
    window.addEventListener('keydown', escape)
    window.addEventListener('wheel', close, true)
    return () => { window.removeEventListener('pointerdown', close, true); window.removeEventListener('keydown', escape); window.removeEventListener('wheel', close, true) }
  }, [menu])
  const displayTabs = order ? [...tabs].sort((a, b) => order.indexOf(a.id) - order.indexOf(b.id)) : tabs
  return <div ref={rootRef} className="view-picker" role="tablist" aria-label="视图"
      onPointerMove={event => {
        const state = drag.current
        if (!state) return
        if (Math.hypot(event.clientX - state.x, event.clientY - state.y) < 4 && !state.moved) return
        if (!state.moved) event.currentTarget.setPointerCapture(event.pointerId)
        state.moved = true
        const bounds = event.currentTarget.getBoundingClientRect()
        if (event.clientX < bounds.left + 28) event.currentTarget.scrollLeft -= 24
        else if (event.clientX > bounds.right - 28) event.currentTarget.scrollLeft += 24
        // 拖动期间仅更新本地排序；悬浮副本跟随指针，松手后一次性保存。
        setPreview({ tab: tabs.find(item => item.id === state.id)!, x: event.clientX + 10, y: event.clientY - 18 })
        const siblings = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('[data-view-id]') ?? [])
        const target = siblings.find(element => { const rect = element.getBoundingClientRect(); return event.clientX >= rect.left && event.clientX <= rect.right })
        if (!target || target.dataset.viewId === state.id) return
        const ids = [...(orderRef.current ?? tabs.map(item => item.id))]
        const from = ids.indexOf(state.id)
        const to = ids.indexOf(target.dataset.viewId!)
        if (from < 0 || to < 0) return
        ids.splice(from, 1)
        ids.splice(to, 0, state.id)
        orderRef.current = ids
        setOrder(ids)
      }}
      onPointerUp={event => {
        suppressClick.current = drag.current?.moved ?? false
        drag.current = null
        if (orderRef.current) onReorder?.(orderRef.current)
        orderRef.current = null
        setPreview(null)
        if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId)
      }}
      onPointerCancel={() => { drag.current = null; orderRef.current = null; setOrder(null); setPreview(null) }}
>
    {displayTabs.map(tab => <button key={tab.id} className={`btn view-picker-tab${preview?.tab.id === tab.id ? " dragging" : ""}`} role="tab"
      onContextMenu={event => { event.preventDefault(); if ((!tab.isDefault && onDelete) || (onEdit && (tab.kind === 'table' || tab.kind === 'graph'))) setMenu({ id: tab.id, x: Math.min(event.clientX, window.innerWidth - 160), y: Math.min(event.clientY, window.innerHeight - 45) }) }}
      onPointerEnter={() => onPreview?.(tab)} onFocus={() => onPreview?.(tab)}
      data-view-id={tab.id} aria-selected={!activePluginKey && tab.id === activeId}
      onPointerDown={event => {
        if (event.button !== 0 || !onReorder) return
        suppressClick.current = false
        drag.current = { id: tab.id, x: event.clientX, y: event.clientY, moved: false }

      }}
      onClick={() => {
        if (suppressClick.current) { suppressClick.current = false; return }
        onSelect(tab)
      }}
      onKeyDown={event => {
        if (!onReorder || !event.altKey || !['ArrowLeft', 'ArrowRight'].includes(event.key)) return
        event.preventDefault()
        const ids = tabs.map(item => item.id)
        const index = ids.indexOf(tab.id)
        const next = index + (event.key === 'ArrowLeft' ? -1 : 1)
        if (next < 0 || next >= ids.length) return
        ;[ids[index], ids[next]] = [ids[next], ids[index]]
        onReorder(ids)
      }}
      title={onReorder ? `${tab.name}（拖动排序）` : tab.name}>
      <Icon name={tab.kind === 'source' ? 'code' : tab.kind} size={14} /><span>{tab.name}</span>
    </button>)}
    {plugins.map(item => <button key={item.key} className="btn view-picker-tab" role="tab" aria-selected={item.key === activePluginKey} onClick={() => onSelectPlugin(item.key)}><Icon name="extensions" size={14} /><span>{item.title}</span></button>)}
    {onCreate && <button className="btn btn-icon" title="新建视图" aria-label="新建视图" onClick={onCreate}><Icon name="plus" size={14} /></button>}
    {preview && createPortal(<div className="view-drag-preview" style={{ left: preview.x, top: preview.y }}><Icon name={preview.tab.kind === 'source' ? 'code' : preview.tab.kind} size={14} />{preview.tab.name}</div>, document.body)}
    {menu && createPortal(<div ref={menuRef} className="context-menu" role="menu" style={{ position: 'fixed', left: menu.x, top: menu.y, zIndex: 1000 }}>
      {onEdit && tabs.some(tab => tab.id === menu.id && (tab.kind === 'table' || tab.kind === 'graph')) && <button className="ctx-item" role="menuitem" onClick={() => { onEdit(menu.id); setMenu(null) }}><Icon name="settings" size={14} />视图设置</button>}
      {tabs.some(tab => tab.id === menu.id && !tab.isDefault) && <button className="ctx-item" role="menuitem" onClick={() => { onDelete?.(menu.id); setMenu(null) }}><Icon name="trash" size={14} />删除视图</button>}
    </div>, document.body)}
  </div>
}

/** 固定业务视图使用同一标签组件，只省略自定义视图管理与排序入口。 */
export function BuiltinViewPicker<T extends 'table' | 'record' | 'source'>({ views, active, onSelect }: {
  views: readonly T[]; active: T; onSelect: (view: T) => void
}) {
  const names = { table: '表格', record: '记录', source: '源码' }
  return <ViewPicker tabs={views.map(kind => ({ id: kind, kind, name: names[kind], isDefault: true }))}
    activeId={active} plugins={[]} onSelect={tab => onSelect(tab.id as T)} onSelectPlugin={() => {}} />
}
