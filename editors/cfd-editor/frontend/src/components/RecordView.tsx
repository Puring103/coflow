import { useState, useRef } from 'react'
import type { FileRecords, FieldValue, RecordRow, FieldPathSegment, DiagnosticItem } from '../bindings/index'
import { DataCardExpanded, CardHeader } from './DataCard'
import { Icon } from './Icon'
import { typeColor } from '../utils/typeColor'
import { diagnosticsForRecord } from '../App'

interface Props {
  data: FileRecords
  recordKey: string
  typeFilter?: string
  readOnly?: boolean
  diagnostics?: DiagnosticItem[]
  highlightField?: string | null
  onHighlightConsumed?: () => void
  onOpenRecord: (key: string) => void
  onWriteField?: (recordKey: string, fieldPath: FieldPathSegment[], newValue: FieldValue) => Promise<RecordRow | void>
}

export function RecordView({ data, recordKey, typeFilter, readOnly, diagnostics, highlightField, onHighlightConsumed, onOpenRecord, onWriteField }: Props) {
  const record = data.records.find(r => r.key === recordKey)
  const [search, setSearch] = useState('')
  const sidebarRef = useRef<HTMLDivElement>(null)

  const sidebarRecords = typeFilter
    ? data.records.filter(r => r.actual_type === typeFilter)
    : data.records

  if (!record) {
    return <div className="record-view"><div className="empty-hint">记录 "{recordKey}" 未找到</div></div>
  }

  const showSearch = record.fields.length > 6
  const fields = search
    ? record.fields.filter(f => f.name.toLowerCase().includes(search.toLowerCase()))
    : record.fields

  const fieldDiags = diagnostics
    ? diagnosticsForRecord(diagnostics, data.file_path, recordKey)
    : []
  // Per-record severity for sidebar dots: any error in any field, or a record-
  // level diagnostic (field_path is null) attached to that record.
  const recordSeverity = (key: string): 'error' | 'warning' | null => {
    if (!diagnostics) return null
    let sev: 'error' | 'warning' | null = null
    for (const d of diagnostics) {
      if (d.file_path !== data.file_path || d.record_key !== key) continue
      if (d.severity === 'error') return 'error'
      if (d.severity === 'warning') sev = 'warning'
    }
    return sev
  }

  const onSidebarKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp' && e.key !== 'Enter') return
    const keys = sidebarRecords.map(r => r.key)
    if (keys.length === 0) return
    const cur = document.activeElement as HTMLElement | null
    const idx = keys.indexOf(cur?.dataset.key ?? '')
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      const next = keys[Math.min(idx + 1, keys.length - 1)]
      sidebarRef.current?.querySelector<HTMLElement>(`[data-key="${cssEscape(next)}"]`)?.focus()
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      const prev = keys[Math.max(idx - 1, 0)]
      sidebarRef.current?.querySelector<HTMLElement>(`[data-key="${cssEscape(prev)}"]`)?.focus()
    } else if (e.key === 'Enter') {
      const key = cur?.dataset.key
      if (key) {
        e.preventDefault()
        onOpenRecord(key)
      }
    }
  }

  return (
    <div className="record-view">
      <div className="rv-sidebar" role="listbox" aria-label="记录列表" onKeyDown={onSidebarKeyDown} ref={sidebarRef}>
        {sidebarRecords.map(r => {
          const sev = recordSeverity(r.key)
          return (
            <div
              key={r.key}
              className={`rv-sidebar-item${r.key === recordKey ? ' selected' : ''}${sev ? ' rv-sidebar-' + sev : ''}`}
              role="option"
              aria-selected={r.key === recordKey}
              tabIndex={r.key === recordKey ? 0 : -1}
              data-key={r.key}
              onClick={() => onOpenRecord(r.key)}
              onKeyDown={e => {
                if (e.key === 'Enter') {
                  e.preventDefault()
                  e.stopPropagation()
                  onOpenRecord(r.key)
                }
              }}
            >
              <span className="rv-item-type" style={{ color: typeColor(r.actual_type) }}>{r.actual_type}</span>
              <span className="rv-item-key">{r.key}</span>
            </div>
          )
        })}
      </div>

      <div className="rv-main">
        <CardHeader recordKey={record.key} actualType={record.actual_type} filePath={data.file_path} />
        {showSearch && (
          <div className="rv-search-bar">
            <Icon name="search" size={13} className="rv-search-icon" aria-hidden />
            <input
              placeholder="搜索字段"
              value={search}
              onChange={e => setSearch(e.target.value)}
              aria-label="搜索字段"
            />
            {search && (
              <button className="rv-clear-search" onClick={() => setSearch('')} aria-label="清除搜索">
                <Icon name="close" size={13} aria-hidden />
              </button>
            )}
          </div>
        )}
        <DataCardExpanded
          fields={fields}
          onEdit={readOnly || !onWriteField ? undefined : (path, val) => { onWriteField(record.key, path, val) }}
          diagnostics={fieldDiags}
          highlightField={highlightField}
          onHighlightConsumed={onHighlightConsumed}
        />
      </div>
    </div>
  )
}

function cssEscape(s: string): string {
  if (typeof CSS !== 'undefined' && typeof CSS.escape === 'function') return CSS.escape(s)
  return s.replace(/["\\]/g, '\\$&')
}

