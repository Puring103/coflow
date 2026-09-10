import { useRef, useState, type ReactNode } from 'react'
import { moveViewTab } from '../state/views'

interface Props {
  ids: string[]
  onReorder: (ids: string[]) => void
  children: ReactNode
}

export function ReorderableViewTabs({ ids, onReorder, children }: Props) {
  const drag = useRef<{ id: string; x: number; y: number; moved: boolean } | null>(null)
  const suppressClick = useRef(false)
  const [target, setTarget] = useState<{ id: string; after: boolean } | null>(null)
  const findTarget = (x: number, y: number, root: HTMLElement) => {
    const tab = document.elementFromPoint(x, y)?.closest<HTMLElement>('[data-tab-id]')
    if (!tab || !root.contains(tab) || !ids.includes(tab.dataset.tabId!)) return null
    const rect = tab.getBoundingClientRect()
    return { id: tab.dataset.tabId!, after: x > rect.left + rect.width / 2 }
  }
  return <div className="document-view-tabs" role="tablist" aria-label="视图"
    style={{ touchAction: 'none', userSelect: 'none' }}
    onPointerDown={event => {
      if (event.button !== 0) return
      suppressClick.current = false
      const tab = (event.target as HTMLElement).closest<HTMLElement>('[data-tab-id]')
      if (!tab || !ids.includes(tab.dataset.tabId!)) return
      drag.current = { id: tab.dataset.tabId!, x: event.clientX, y: event.clientY, moved: false }
    }}
    onPointerMove={event => {
      const current = drag.current
      if (!current) return
      if (event.buttons === 0) { drag.current = null; setTarget(null); return }
      // 超过拖动阈值才捕获指针，普通点击仍由原有视图按钮处理。
      if (!current.moved && Math.hypot(event.clientX - current.x, event.clientY - current.y) < 5) return
      current.moved = true
      event.currentTarget.setPointerCapture(event.pointerId)
      setTarget(findTarget(event.clientX, event.clientY, event.currentTarget))
    }}
    onPointerUp={event => {
      const current = drag.current
      drag.current = null
      setTarget(null)
      if (!current?.moved) return
      suppressClick.current = true
      const destination = findTarget(event.clientX, event.clientY, event.currentTarget)
      if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId)
      if (!destination) return
      const next = moveViewTab(ids, current.id, destination.id, destination.after)
      if (next.some((id, index) => id !== ids[index])) onReorder(next)
    }}
    onLostPointerCapture={() => { drag.current = null; setTarget(null) }}
    onPointerCancel={() => { drag.current = null; setTarget(null) }}
    onClickCapture={event => {
      if (suppressClick.current) { event.preventDefault(); event.stopPropagation(); suppressClick.current = false }
    }}
    data-drop-tab={target?.id}
  >
    {children}
    {target && <style>{`.document-view-tabs [data-tab-id="${CSS.escape(target.id)}"] { box-shadow: ${target.after ? '-2px' : '2px'} 0 0 0 var(--accent) inset; }`}</style>}
  </div>
}
