import { useState, useMemo, useEffect, useRef } from 'react'
import { diagnosticDisplayMessage, diagnosticKey, type DiagnosticItem } from '../wire'
import { Icon } from './Icon'

interface Props {
  diagnostics: DiagnosticItem[]
  /** Focus request from outside (e.g. a record/field corner badge click).
   *  When `tick` changes we scroll to and pulse the matching item; if the
   *  panel is collapsed we auto-expand it first. */
  focus?: { key: string; tick: number } | null
  onFocusConsumed?: () => void
  /** Predicate that decides whether a "跳转" button should be offered.
   *  Defaults to "always available when the diagnostic carries a file". */
  isJumpable?: (filePath: string) => boolean
  onJumpToRecord?: (file: string, key: string, actualType: string | null) => void
  onJumpToField?: (file: string, key: string, actualType: string | null, fieldPath: string) => void
}

type SevFilter = 'all' | 'error' | 'warning' | 'info'

export function DiagnosticsPanel({ diagnostics, focus, onFocusConsumed, isJumpable, onJumpToRecord, onJumpToField }: Props) {
  const [collapsed, setCollapsed] = useState(false)
  const [sevFilter, setSevFilter] = useState<SevFilter>('all')
  const [fileFilter, setFileFilter] = useState<string>('all')
  const [groupByFile, setGroupByFile] = useState(true)
  const listRef = useRef<HTMLDivElement>(null)
  const [flashKey, setFlashKey] = useState<string | null>(null)

  const errors   = diagnostics.filter(d => d.severity === 'error').length
  const warnings = diagnostics.filter(d => d.severity === 'warning').length

  // No diagnostics → keep header only, no body region.
  const isEmpty = diagnostics.length === 0

  const files = useMemo(() => {
    const set = new Set<string>()
    for (const d of diagnostics) if (d.file_path) set.add(d.file_path)
    return Array.from(set).sort()
  }, [diagnostics])

  const filtered = useMemo(() => {
    return diagnostics.filter(d => {
      if (sevFilter !== 'all' && d.severity !== sevFilter) return false
      if (fileFilter !== 'all' && d.file_path !== fileFilter) return false
      return true
    })
  }, [diagnostics, sevFilter, fileFilter])

  // Ensure a focused diagnostic passes the current filters. Otherwise the
  // node exists in `diagnostics` but not in the rendered `filtered` list, so
  // querySelector would return null. We only override filters when the
  // focused item is currently hidden.
  useEffect(() => {
    if (!focus) return
    const target = diagnostics.find(d => diagnosticKey(d) === focus.key)
    if (!target) return
    if (sevFilter !== 'all' && target.severity !== sevFilter) setSevFilter('all')
    if (fileFilter !== 'all' && target.file_path !== fileFilter) setFileFilter('all')
    setCollapsed(false)
  }, [focus, diagnostics])

  // Scroll to and pulse the focused item after the reveal effect above has
  // (potentially) mutated filters/collapsed state and React re-rendered.
  useEffect(() => {
    if (!focus) return
    const el = listRef.current?.querySelector<HTMLElement>(
      `[data-diag-key="${cssEscape(focus.key)}"]`,
    )
    if (el) {
      el.scrollIntoView({ block: 'center', behavior: 'smooth' })
      setFlashKey(focus.key)
      const t = window.setTimeout(() => {
        setFlashKey(prev => (prev === focus.key ? null : prev))
      }, 1600)
      onFocusConsumed?.()
      return () => window.clearTimeout(t)
    }
    onFocusConsumed?.()
  }, [focus, filtered, onFocusConsumed])

  // Group by file_path (null files bucketed under '(项目级)').
  const groups = useMemo(() => {
    const m = new Map<string, DiagnosticItem[]>()
    for (const d of filtered) {
      const key = d.file_path ?? '(项目级)'
      const list = m.get(key) ?? []
      list.push(d)
      m.set(key, list)
    }
    return Array.from(m.entries())
  }, [filtered])

  return (
    <div className={`diag-panel${collapsed || isEmpty ? ' collapsed' : ''}`}>
      <button
        type="button"
        className="diag-header"
        aria-expanded={!collapsed && !isEmpty}
        aria-controls="diag-list-region"
        disabled={isEmpty}
        onClick={() => setCollapsed(c => !c)}
      >
        <span className="diag-title">
          <Icon name={collapsed ? 'chevron-right' : 'chevron-down'} size={11} aria-hidden />
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
          <div className="diag-toolbar">
            <div className="diag-sev-filters" role="group" aria-label="按严重程度过滤">
              {(['all', 'error', 'warning', 'info'] as SevFilter[]).map(s => (
                <button
                  key={s}
                  className={`diag-filter-chip${sevFilter === s ? ' active' : ''}`}
                  onClick={() => setSevFilter(s)}
                  aria-pressed={sevFilter === s}
                >
                  {s === 'all' ? '全部' : s === 'error' ? '错误' : s === 'warning' ? '警告' : '信息'}
                </button>
              ))}
            </div>
            {files.length > 1 && (
              <div className="diag-file-pills" role="group" aria-label="按文件过滤">
                <button
                  className={`diag-file-pill${fileFilter === 'all' ? ' active' : ''}`}
                  onClick={() => setFileFilter('all')}
                  aria-pressed={fileFilter === 'all'}
                >
                  全部
                </button>
                {files.map(f => (
                  <button
                    key={f}
                    className={`diag-file-pill${fileFilter === f ? ' active' : ''}`}
                    onClick={() => setFileFilter(f)}
                    aria-pressed={fileFilter === f}
                  >
                    {f.split('/').pop()}
                  </button>
                ))}
              </div>
            )}
            <label className="diag-group-toggle">
              <input
                type="checkbox"
                checked={groupByFile}
                onChange={e => setGroupByFile(e.target.checked)}
              />
              按文件分组
            </label>
          </div>
          <div className="diag-list" role="list" ref={listRef}>
            {(groupByFile ? groups : ([['', filtered]] as [string, DiagnosticItem[]][])).map(([gname, items]) => (
              <div key={gname} className="diag-group">
                {groupByFile && items.length > 0 && (
                  <div className="diag-group-head">{gname} <span className="diag-group-count">{items.length}</span></div>
                )}
                {items.map((d, i) => {
                  const key = diagnosticKey(d)
                  const canJump = !!d.file_path && !!d.record_key && (!isJumpable || isJumpable(d.file_path))
                  const showFieldJump = canJump && !!d.field_path && !!onJumpToField
                  const showRecordJump = canJump && !showFieldJump && !!onJumpToRecord
                  return (
                    <div
                      key={i}
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
                      <span className="diag-msg">{diagnosticDisplayMessage(d)}</span>
                      {d.code && <span className="diag-code">{d.code}</span>}
                      {showFieldJump ? (
                        <button
                          className="diag-jump"
                          onClick={() => onJumpToField!(d.file_path!, d.record_key!, d.actual_type, d.field_path!)}
                          title={`跳转到字段 ${d.field_path}`}
                        >
                          <Icon name="jump" size={11} aria-hidden />
                          {d.field_path}
                        </button>
                      ) : showRecordJump ? (
                        <button
                          className="diag-jump"
                          onClick={() => onJumpToRecord!(d.file_path!, d.record_key!, d.actual_type)}
                          aria-label={`跳转到记录 ${d.record_key}`}
                        >
                          <Icon name="jump" size={11} aria-hidden />
                          跳转
                        </button>
                      ) : null}
                    </div>
                  )
                })}
              </div>
            ))}
            {filtered.length === 0 && (
              <div className="diag-empty">当前过滤条件下无诊断</div>
            )}
          </div>
        </div>
      )}
    </div>
  )
}

function cssEscape(s: string): string {
  if (typeof CSS !== 'undefined' && typeof CSS.escape === 'function') return CSS.escape(s)
  return s.replace(/["\\]/g, '\\$&')
}
