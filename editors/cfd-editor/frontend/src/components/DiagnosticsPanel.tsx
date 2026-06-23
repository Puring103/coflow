import { useState, useMemo } from 'react'
import type { DiagnosticItem } from '../bindings/index'
import { Icon } from './Icon'

interface Props {
  diagnostics: DiagnosticItem[]
  onJumpToRecord?: (file: string, key: string) => void
  onJumpToField?: (file: string, key: string, fieldPath: string) => void
}

type SevFilter = 'all' | 'error' | 'warning' | 'info'

export function DiagnosticsPanel({ diagnostics, onJumpToRecord, onJumpToField }: Props) {
  const [collapsed, setCollapsed] = useState(false)
  const [sevFilter, setSevFilter] = useState<SevFilter>('all')
  const [fileFilter, setFileFilter] = useState<string>('all')
  const [groupByFile, setGroupByFile] = useState(true)

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
              <select
                className="diag-file-filter"
                value={fileFilter}
                onChange={e => setFileFilter(e.target.value)}
                aria-label="按文件过滤"
              >
                <option value="all">全部文件 ({diagnostics.length})</option>
                {files.map(f => (
                  <option key={f} value={f}>{f.split('/').pop()}</option>
                ))}
              </select>
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
          <div className="diag-list" role="list">
            {(groupByFile ? groups : ([['', filtered]] as [string, DiagnosticItem[]][])).map(([gname, items]) => (
              <div key={gname} className="diag-group">
                {groupByFile && items.length > 0 && (
                  <div className="diag-group-head">{gname} <span className="diag-group-count">{items.length}</span></div>
                )}
                {items.map((d, i) => (
                  <div key={i} className={`diag-item ${d.severity}`} role="listitem">
                    <span className="diag-icon">
                      <Icon
                        name={d.severity === 'error' ? 'error' : d.severity === 'warning' ? 'warning' : 'info'}
                        size={14}
                        aria-hidden
                      />
                    </span>
                    <span className="diag-msg">{d.message}</span>
                    {d.code && <span className="diag-code">{d.code}</span>}
                    {d.field_path && d.file_path && d.record_key && onJumpToField ? (
                      <button
                        className="diag-jump"
                        onClick={() => onJumpToField(d.file_path!, d.record_key!, d.field_path!)}
                        title={`跳转到字段 ${d.field_path}`}
                      >
                        <Icon name="jump" size={11} aria-hidden />
                        {d.field_path}
                      </button>
                    ) : d.file_path && d.record_key && onJumpToRecord ? (
                      <button
                        className="diag-jump"
                        onClick={() => onJumpToRecord(d.file_path!, d.record_key!)}
                        aria-label={`跳转到记录 ${d.record_key}`}
                      >
                        <Icon name="jump" size={11} aria-hidden />
                        跳转
                      </button>
                    ) : null}
                  </div>
                ))}
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
