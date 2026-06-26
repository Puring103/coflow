import { useState, useRef } from 'react'
import type { FileRecords } from '../bindings/FileRecords'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import {
  coordinateId,
  diagnosticMatchesCoordinate,
  recordActualType,
  recordKey,
  sameCoordinate,
  type DiagnosticItem,
  type FieldPathSegment,
  type FieldValue,
} from '../wire'
import { DataCardExpanded, CardHeader } from './DataCard'
import { Icon } from './Icon'
import { typeColor } from '../utils/typeColor'
import { diagnosticsForRecord } from '../App'

interface Props {
  data: FileRecords
  coordinate: RecordCoordinate
  typeFilter?: string
  readOnly?: boolean
  diagnostics?: DiagnosticItem[]
  highlightField?: string | null
  onHighlightConsumed?: () => void
  onOpenRecord: (coordinate: RecordCoordinate) => void
  onWriteField?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], newValue: FieldValue) => Promise<RecordRow | void>
  onRenameRecord?: (coordinate: RecordCoordinate, newKey: string) => Promise<RecordRow | void>
}

export function RecordView({ data, coordinate, typeFilter, readOnly, diagnostics, highlightField, onHighlightConsumed, onOpenRecord, onWriteField, onRenameRecord }: Props) {
  const record = data.records.find(r => sameCoordinate(r.coordinate, coordinate))
  const [search, setSearch] = useState('')
  const sidebarRef = useRef<HTMLDivElement>(null)
  const activeId = coordinateId(coordinate)

  const sidebarRecords = typeFilter
    ? data.records.filter(r => recordActualType(r) === typeFilter)
    : data.records

  if (!record) {
    return <div className="record-view"><div className="empty-hint">记录 "{coordinate.actual_type}.{coordinate.key}" 未找到</div></div>
  }

  const showSearch = record.fields.length > 6
  const fields = search
    ? record.fields.filter(f => f.name.toLowerCase().includes(search.toLowerCase()))
    : record.fields

  const fieldDiags = diagnostics
    ? diagnosticsForRecord(diagnostics, data.file_path, record.coordinate)
    : []
  const canRename = !readOnly && data.capabilities.can_edit_key && !!onRenameRecord
  // Per-record severity for sidebar dots: any error in any field, or a record-
  // level diagnostic (field_path is null) attached to that record.
  const recordSeverity = (coordinate: RecordCoordinate): 'error' | 'warning' | null => {
    if (!diagnostics) return null
    let sev: 'error' | 'warning' | null = null
    for (const d of diagnostics) {
      if (d.file_path !== data.file_path || !diagnosticMatchesCoordinate(d, coordinate)) continue
      if (d.severity === 'error') return 'error'
      if (d.severity === 'warning') sev = 'warning'
    }
    return sev
  }

  const onSidebarKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp' && e.key !== 'Enter') return
    const ids = sidebarRecords.map(r => coordinateId(r.coordinate))
    if (ids.length === 0) return
    const cur = document.activeElement as HTMLElement | null
    const idx = ids.indexOf(cur?.dataset.coordinateId ?? '')
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      const next = ids[Math.min(idx + 1, ids.length - 1)]
      sidebarRef.current?.querySelector<HTMLElement>(`[data-coordinate-id="${cssEscape(next)}"]`)?.focus()
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      const prev = ids[Math.max(idx - 1, 0)]
      sidebarRef.current?.querySelector<HTMLElement>(`[data-coordinate-id="${cssEscape(prev)}"]`)?.focus()
    } else if (e.key === 'Enter') {
      const id = cur?.dataset.coordinateId
      const next = id ? sidebarRecords.find(r => coordinateId(r.coordinate) === id) : null
      if (next) {
        e.preventDefault()
        onOpenRecord(next.coordinate)
      }
    }
  }

  return (
    <div className="record-view">
      <div className="rv-sidebar" role="listbox" aria-label="记录列表" onKeyDown={onSidebarKeyDown} ref={sidebarRef}>
        {sidebarRecords.map(r => {
          const sev = recordSeverity(r.coordinate)
          const id = coordinateId(r.coordinate)
          return (
            <div
              key={id}
              className={`rv-sidebar-item${id === activeId ? ' selected' : ''}${sev ? ' rv-sidebar-' + sev : ''}`}
              role="option"
              aria-selected={id === activeId}
              tabIndex={id === activeId ? 0 : -1}
              data-coordinate-id={id}
              style={{ '--type-color': typeColor(recordActualType(r)) } as React.CSSProperties}
              onClick={() => onOpenRecord(r.coordinate)}
              onKeyDown={e => {
                if (e.key === 'Enter') {
                  e.preventDefault()
                  e.stopPropagation()
                  onOpenRecord(r.coordinate)
                }
              }}
            >
              <span className="rv-item-type" style={{ color: typeColor(recordActualType(r)) }}>{recordActualType(r)}</span>
              <span className="rv-item-key">{recordKey(r)}</span>
            </div>
          )
        })}
      </div>

      <div className="rv-main">
        <CardHeader recordKey={recordKey(record)} actualType={recordActualType(record)} filePath={data.file_path} />
        {canRename && (
          <div className="record-actions">
            <button
              className="btn btn-outlined"
              onClick={async () => {
                const next = window.prompt('重命名 Key', recordKey(record))?.trim()
                if (!next || next === recordKey(record)) return
                await onRenameRecord!(record.coordinate, next)
              }}
            >
              <Icon name="edit" size={13} />
              重命名 Key
            </button>
          </div>
        )}
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
          actualType={recordActualType(record)}
          onEdit={readOnly || !onWriteField ? undefined : (path, val) => { onWriteField(record.coordinate, path, val) }}
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

