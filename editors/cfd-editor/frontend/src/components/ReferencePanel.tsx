import { useEffect, useRef, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { typeColor } from '../utils/typeColor'
import { flushDataEdits } from '../state/editSession'

/** 复用 Inspector 的浮层容器；退出前等待草稿保存，并恢复原视图焦点。 */
export function ReferencePanel({ title, recordId, actualType, children, canGoBack, onBack, onClose, onNavigate }: {
  title: string; recordId: string; actualType: string; children: ReactNode; canGoBack: boolean
  onBack: () => void; onClose: () => void; onNavigate: () => void
}) {
  const panel = useRef<HTMLDivElement>(null)
  const leave = async (action: () => void) => { if (await flushDataEdits()) action() }
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null
    panel.current?.focus({ preventScroll: true })
    return () => { if (previous?.isConnected) previous.focus({ preventScroll: true }) }
  }, [])
  return createPortal(<div className="reference-panel-overlay" onMouseDown={event => {
    if (event.target === event.currentTarget) void leave(onClose)
  }}><div ref={panel} tabIndex={-1} role="dialog" aria-modal="true" aria-label={`引用记录：${title}`} className="reference-panel" onKeyDown={event => {
    if (event.key === 'Escape' && !event.defaultPrevented) { event.preventDefault(); void leave(onClose) }
    if (event.key === 'Tab' && !event.defaultPrevented) {
      const items = Array.from(panel.current?.querySelectorAll<HTMLElement>('button:not([disabled]), input:not([disabled]), textarea:not([disabled]), [tabindex="0"]') ?? []).filter(item => item.getClientRects().length > 0)
      const target = document.activeElement
      if (event.shiftKey && (target === items[0] || target === panel.current)) { event.preventDefault(); items[items.length - 1]?.focus() }
      else if (!event.shiftKey && target === items[items.length - 1]) { event.preventDefault(); items[0]?.focus() }
    }
    if (!((event.ctrlKey || event.metaKey) && ['z', 'y'].includes(event.key.toLowerCase()))) event.stopPropagation()
  }}>
    <div className="reference-panel-toolbar" style={{ '--node-color': typeColor(actualType) } as React.CSSProperties}>
      {canGoBack && <button className="btn" onClick={() => void leave(onBack)}>返回</button>}
      <strong><span className="reference-record-id">{recordId}</span>{title !== recordId && <span>{title}</span>}</strong>
      <button className="btn" onClick={() => void leave(onNavigate)}>打开所在位置</button>
      <button className="btn" aria-label="关闭引用记录" onClick={() => void leave(onClose)}>×</button>
    </div>{children}
  </div></div>, document.body)
}
