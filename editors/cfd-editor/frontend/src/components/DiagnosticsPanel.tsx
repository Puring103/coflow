import { useState, useEffect, useRef, type PointerEvent as ReactPointerEvent } from 'react'
import { diagnosticDisplayMessage, diagnosticKey, type DiagnosticItem } from '../wire'
import type { DiagnosticTarget } from '../bindings/DiagnosticTarget'
import { Icon } from './Icon'
import { cssEscape } from '../utils/dom'

interface Props {
  /** 初始收起完整列表，诊断定位仍会主动展开。 */
  initialCollapsed?: boolean
  currentFile?: string
  diagnostics: DiagnosticItem[]
  /** Focus request from outside (e.g. a record/field corner badge click).
   *  When `tick` changes we scroll to and pulse the matching item; if the
   *  panel is collapsed we auto-expand it first. */
  focus?: { key: string; tick: number } | null
  onFocusConsumed?: () => void
  onJump?: (target: DiagnosticTarget) => void
}
const DEFAULT_HEIGHT = 200
const MIN_HEIGHT = 112
const MIN_EDITOR_HEIGHT = 120
const HEIGHT_STORAGE_KEY = 'cfd-editor-diagnostics-h'

export function clampDiagnosticsHeight(height: number, editorHeight: number): number {
  const maximum = Math.max(MIN_HEIGHT, editorHeight - MIN_EDITOR_HEIGHT)
  return Math.min(maximum, Math.max(MIN_HEIGHT, height))
}

function initialHeight(): number {
  if (typeof localStorage === 'undefined') return DEFAULT_HEIGHT
  return diagnosticsHeightFromStorage(localStorage.getItem(HEIGHT_STORAGE_KEY))
}

export function diagnosticsHeightFromStorage(value: string | null): number {
  if (value === null) return DEFAULT_HEIGHT
  const stored = Number(value)
  return Number.isFinite(stored) ? Math.max(MIN_HEIGHT, stored) : DEFAULT_HEIGHT
}

export function DiagnosticsPanel({ currentFile, diagnostics, focus, onFocusConsumed, onJump, initialCollapsed = true }: Props) {
  const [severity, setSeverity] = useState('all')
  const [onlyCurrentFile, setOnlyCurrentFile] = useState(false)
  const filtered = diagnostics.filter(item => (severity === 'all' || item.severity === severity) && (!onlyCurrentFile || !currentFile || (item.target.kind !== 'none' && item.target.file_path === currentFile)))
  const [collapsed, setCollapsed] = useState(initialCollapsed)
  const [height, setHeight] = useState(initialHeight)
  const [resizing, setResizing] = useState(false)
  const panelRef = useRef<HTMLDivElement>(null)
  const resizeRef = useRef<{ pointerId: number; startY: number; startHeight: number } | null>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const flashTimer = useRef<number | undefined>(undefined)
  useEffect(() => () => window.clearTimeout(flashTimer.current), [])
  const [flashKey, setFlashKey] = useState<string | null>(null)

  const errors   = diagnostics.filter(d => d.severity === 'error').length
  const warnings = diagnostics.filter(d => d.severity === 'warning').length

  // No diagnostics → keep header only, no body region.
  const isEmpty = diagnostics.length === 0

  useEffect(() => {
    try { localStorage.setItem(HEIGHT_STORAGE_KEY, String(height)) } catch { /* quota */ }
  }, [height])

  const editorHeight = () => panelRef.current?.parentElement?.clientHeight ?? window.innerHeight
  const resizeTo = (next: number) => setHeight(clampDiagnosticsHeight(next, editorHeight()))

  const onResizePointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault()
    event.currentTarget.setPointerCapture(event.pointerId)
    resizeRef.current = {
      pointerId: event.pointerId,
      startY: event.clientY,
      startHeight: panelRef.current?.getBoundingClientRect().height ?? height,
    }
    setResizing(true)
  }

  const onResizePointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const resize = resizeRef.current
    if (!resize || resize.pointerId !== event.pointerId) return
    resizeTo(resize.startHeight - (event.clientY - resize.startY))
  }

  const finishResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (resizeRef.current?.pointerId !== event.pointerId) return
    resizeRef.current = null
    setResizing(false)
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId)
    }
  }

  useEffect(() => {
    if (!focus) return
    const target = diagnostics.find(d => diagnosticKey(d) === focus.key)
    if (!target) return
    setSeverity('all')
    setOnlyCurrentFile(false)
    setCollapsed(false)
  }, [focus, diagnostics])

  // Scroll to and pulse the focused item after the reveal effect above has
  // expanded the panel and React has re-rendered.
  useEffect(() => {
    if (!focus || collapsed || severity !== 'all' || onlyCurrentFile) return
    const el = listRef.current?.querySelector<HTMLElement>(
      `[data-diag-key="${cssEscape(focus.key)}"]`,
    )
    if (el) {
      el.scrollIntoView({ block: 'center', behavior: 'smooth' })
      setFlashKey(focus.key)
      window.clearTimeout(flashTimer.current)
      flashTimer.current = window.setTimeout(() => {
        setFlashKey(prev => (prev === focus.key ? null : prev))
      }, 1600)
      onFocusConsumed?.()
      return
    }
    onFocusConsumed?.()
  }, [focus, diagnostics, onFocusConsumed, collapsed, severity, onlyCurrentFile])

  return (
    <div
      ref={panelRef}
      className={`diag-panel${collapsed || isEmpty ? ' collapsed' : ''}${resizing ? ' resizing' : ''}`}
      style={collapsed || isEmpty ? undefined : { height }}
    >
      {!collapsed && !isEmpty && (
        <div
          className="diag-resizer"
          role="separator"
          aria-label="调整诊断面板高度"
          aria-orientation="horizontal"
          aria-valuemin={MIN_HEIGHT}
          aria-valuenow={height}
          tabIndex={0}
          onPointerDown={onResizePointerDown}
          onPointerMove={onResizePointerMove}
          onPointerUp={finishResize}
          onPointerCancel={finishResize}
          onKeyDown={event => {
            if (event.key === 'ArrowUp') {
              event.preventDefault()
              resizeTo(height + 24)
            } else if (event.key === 'ArrowDown') {
              event.preventDefault()
              resizeTo(height - 24)
            } else if (event.key === 'Home') {
              event.preventDefault()
              resizeTo(MIN_HEIGHT)
            } else if (event.key === 'End') {
              event.preventDefault()
              resizeTo(editorHeight())
            }
          }}
        />
      )}
      <button
        type="button"
        className="diag-header"
        aria-expanded={!collapsed && !isEmpty}
        aria-controls="diag-list-region"
        disabled={isEmpty}
        onClick={() => setCollapsed(c => !c)}
      >
        <span className="diag-title">
          <Icon name={collapsed || isEmpty ? 'panel-bottom-closed' : 'panel-bottom'} size={15} aria-hidden />
          诊断
        </span>
        {errors > 0 && (
          <span className="diag-badge error">
            <Icon name="error" size={11} aria-hidden />
            {errors}
          </span>
        )}
        {warnings > 0 && (
          <span className="diag-badge warning">
            <Icon name="warning" size={11} aria-hidden />
            {warnings}
          </span>
        )}
        {errors === 0 && warnings === 0 && (
          <span className="diag-badge ok">
            <Icon name="check" size={11} aria-hidden />
            无问题
          </span>
        )}
      </button>
      {!collapsed && !isEmpty && (
        <div className="diag-body" id="diag-list-region">
          <div className="diag-filters">
            <select aria-label="诊断级别" value={severity} onChange={event => setSeverity(event.target.value)}><option value="all">所有级别</option><option value="error">错误</option><option value="warning">警告</option><option value="info">信息</option></select>
            <label><input type="checkbox" checked={onlyCurrentFile} disabled={!currentFile} onChange={event => setOnlyCurrentFile(event.target.checked)} />当前文件</label>
            <span>{filtered.length} / {diagnostics.length}</span>
          </div>
          <div className="diag-list" role="list" ref={listRef}>
            {filtered.map((d, i) => {
              const key = diagnosticKey(d)
              const canJump = d.target.kind !== 'none' && !!onJump
              return (
                <div
                  key={`${key}:${i}`}
                  className={`diag-item ${d.severity}${flashKey === key ? ' focused' : ''}`}
                  role="listitem"
                  data-diag-key={key}
                >
                  <span className="diag-icon">
                    <Icon
                      name={d.severity === 'error' ? 'error' : d.severity === 'warning' ? 'warning' : 'info'}
                      size={14}
                      aria-hidden
                    />
                  </span>
                  <span className="diag-msg">
                    {diagnosticDisplayMessage(d)}
                    {<span className="diag-location">
                      {d.target.kind !== 'none' && <span>{d.target.file_path}{d.target.kind === 'table_field' ? ` · ${d.target.field_path}` : ''}</span>}
                      {d.code && <span className="diag-code">{d.code}</span>}
                    </span>}
                  </span>
                  {(d.code || canJump) && (
                    <span className="diag-actions">
                      {canJump ? (
                        <button
                          className="diag-jump"
                          onClick={() => onJump!(d.target)}
                          title="跳转到诊断位置"
                        >
                          <Icon name="jump" size={11} aria-hidden />
                          跳转
                        </button>
                      ) : null}
                    </span>
                  )}
                </div>
              )
            })}
          </div>
        </div>
      )}
    </div>
  )
}
