import { useEffect, useLayoutEffect, useRef, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { typeColor } from '../../../shared/lib/typeColor'
import { flushDataEdits } from '../state/editSession'
import { useSurfaceExit } from '../../../shared/motion/useSurfaceExit'
import { motionDuration, motionToken, reducedMotion } from '../../../shared/motion/motion'

/** 复用 Inspector 的浮层容器；退出前等待草稿保存，并恢复原视图焦点。 */
export function ReferencePanel({ title, recordId, actualType, children, depth, onBack, onClose, onNavigate }: {
  title: string; recordId: string; actualType: string; children: ReactNode; depth: number
  onBack: () => void; onClose: () => void; onNavigate: () => void
}) {
  const panel = useRef<HTMLDivElement>(null)
  const identity = JSON.stringify([actualType, recordId, depth])
  const canGoBack = depth > 1
  const exit = useSurfaceExit(panel, identity)
  const previousDepth = useRef(0)
  useLayoutEffect(() => {
    const element = panel.current!
    const from = previousDepth.current === 0 || previousDepth.current === depth ? 'translateY(6px)'
      : depth < previousDepth.current ? 'translateX(-10px)' : 'translateX(10px)'
    previousDepth.current = depth
    if (reducedMotion()) return
    // 深入与返回使用相反方向，记录刷新保持原位；正文始终保持正常比例。
    const animation = element.animate([{ opacity: 0, transform: from }, { opacity: 1, transform: 'none' }], {
      duration: motionDuration('enter'), easing: motionToken('enter-ease'),
    })
    return () => animation.cancel()
  }, [identity, depth])
  const leaving = useRef(false)
  const leave = async (action: () => void) => {
    if (leaving.current) return
    leaving.current = true
    try {
      if (!await flushDataEdits()) return
      // 返回保留遮罩，关闭整层才让遮罩一并退出。
      if (action === onBack) await exit(action, { transform: 'translateX(10px)', fadeOverlay: false })
      else await exit(action)
    } finally { leaving.current = false }
  }
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null
    panel.current?.focus({ preventScroll: true })
    return () => { if (previous?.isConnected) previous.focus({ preventScroll: true }) }
  }, [])
  useEffect(() => { panel.current?.focus({ preventScroll: true }) }, [identity])
  return createPortal(<div className="reference-panel-overlay" onClick={event => {
    // 完整点击后才关闭当前层，避免卸载后点击穿透到下层遮罩。
    event.stopPropagation()
    if (event.target === event.currentTarget) void leave(canGoBack ? onBack : onClose)
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
