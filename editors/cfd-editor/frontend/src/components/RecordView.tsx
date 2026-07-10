import { useState, useEffect, useRef } from 'react'
import type { FileRecords } from '../bindings/FileRecords'
import type { CreateRecordDraft } from '../bindings/CreateRecordDraft'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import type { CollectionEdit } from '../bindings/CollectionEdit'
import {
  coordinateId,
  recordActualType,
  recordKey,
  sameCoordinate,
  type DiagnosticItem,
  type FieldPathSegment,
  type FieldValue,
} from '../wire'
import { DataCardExpanded, CardHeader } from './DataCard'
import { CreateRecordDialog } from './CreateRecordDialog'
import { Icon } from './Icon'
import { typeColor } from '../utils/typeColor'
import { RECORD_HIGHLIGHT_SENTINEL } from '../App'

interface Props {
  data: FileRecords
  coordinate: RecordCoordinate
  typeFilter?: string
  readOnly?: boolean
  diagnostics?: DiagnosticItem[]
  /** Filters the sidebar record list (shared global search). */
  recordSearch?: string
  highlightField?: string | null
  onHighlightConsumed?: () => void
  onOpenRecord: (coordinate: RecordCoordinate) => void
  onWriteField?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], newValue: FieldValue) => Promise<RecordRow | void>
  onCollectionEdit?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], edit: CollectionEdit) => Promise<RecordRow | void>
  onRenameRecord?: (coordinate: RecordCoordinate, newKey: string) => Promise<RecordRow | void>
  onInsertRecord?: (recordKey: string, actualType: string, fields: FieldValue) => Promise<void>
  onCreateRecordDraft?: (actualType: string) => Promise<CreateRecordDraft>
  /** Click on a corner badge — either the CardHeader (fieldPath = null) or
   *  a field row (top-level fieldPath). Forwarded up to App so the
   *  diagnostics panel can focus the matching item. */
  onDiagnosticBadgeClick?: (coordinate: RecordCoordinate, fieldPath: string | null) => void
}

export function RecordView({ data, coordinate, typeFilter, readOnly, diagnostics, recordSearch, highlightField, onHighlightConsumed, onOpenRecord, onWriteField, onCollectionEdit, onRenameRecord, onInsertRecord, onCreateRecordDraft, onDiagnosticBadgeClick }: Props) {
  const record = data.records.find(r => sameCoordinate(r.coordinate, coordinate))
  const [fieldSearch, setFieldSearch] = useState('')
  const [showNewRecord, setShowNewRecord] = useState(false)
  const fieldSearchRef = useRef<HTMLInputElement>(null)
  const sidebarRef = useRef<HTMLDivElement>(null)

  const activeId = coordinateId(coordinate)

  // Record-level highlight burns off after the header flashes — the child
  // DataCardExpanded only clears the highlight for field-level jumps.
  useEffect(() => {
    if (highlightField !== RECORD_HIGHLIGHT_SENTINEL) return
    const t = setTimeout(() => onHighlightConsumed?.(), 1600)
    return () => clearTimeout(t)
  }, [highlightField, onHighlightConsumed])

  const allSidebarRecords = typeFilter
    ? data.records.filter(r => recordActualType(r) === typeFilter)
    : data.records

  const sidebarRecords = recordSearch
    ? allSidebarRecords.filter(r => {
        const q = recordSearch.toLowerCase()
        if (recordKey(r).toLowerCase().includes(q)) return true
        for (const f of r.fields) {
          if (f.name.toLowerCase().includes(q)) return true
        }
        return false
      })
    : allSidebarRecords

  if (!record) {
    return <div className="record-view"><div className="empty-hint">记录 "{coordinate.actual_type}.{coordinate.key}" 未找到</div></div>
  }

  const fields = fieldSearch
    ? record.fields.filter(f => f.name.toLowerCase().includes(fieldSearch.toLowerCase()))
    : record.fields

  const fieldDiags = record.field_diagnostics
  const canRename = !readOnly && data.capabilities.can_edit_key && !!onRenameRecord
  const rowSeverity = (row: RecordRow): 'error' | 'warning' | null =>
    row.diagnostic_severity === 'error' || row.diagnostic_severity === 'warning'
      ? row.diagnostic_severity
      : null

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

  const newRecordType = typeFilter || recordActualType(record)
  const canCreate = !readOnly
    && data.capabilities.can_insert_record
    && !!onInsertRecord
    && !!onCreateRecordDraft
    && !!newRecordType

  return (
    <div className="record-view">
      <div className="rv-sidebar-wrap">
        <div className="rv-sidebar" role="listbox" aria-label="记录列表" onKeyDown={onSidebarKeyDown} ref={sidebarRef}>
          {sidebarRecords.map(r => {
            const sev = rowSeverity(r)
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
        {canCreate && (
          <div className="rv-sidebar-footer">
            <button className="btn btn-outlined rv-sidebar-new" onClick={() => setShowNewRecord(true)}>
              <Icon name="plus" size={13} />
              新建记录
            </button>
          </div>
        )}
      </div>

      <div className="rv-main">
        <CardHeader
          recordKey={recordKey(record)}
          actualType={recordActualType(record)}
          filePath={data.file_path}
          onRename={canRename ? async (next) => { await onRenameRecord!(record.coordinate, next) } : undefined}
          diagSeverity={rowSeverity(record)}
          onDiagBadgeClick={onDiagnosticBadgeClick ? () => onDiagnosticBadgeClick(record.coordinate, null) : undefined}
          highlight={highlightField === RECORD_HIGHLIGHT_SENTINEL}
        />
        <div className="rv-search-bar">
          <Icon name="search" size={13} className="rv-search-icon" aria-hidden />
          <input
            ref={fieldSearchRef}
            placeholder="搜索字段…"
            value={fieldSearch}
            onChange={e => setFieldSearch(e.target.value)}
            aria-label="搜索字段"
          />
          {fieldSearch && (
            <button className="rv-clear-search" onClick={() => setFieldSearch('')} aria-label="清除搜索">
              <Icon name="close" size={13} aria-hidden />
            </button>
          )}
        </div>
        <DataCardExpanded
          fields={fields}
          actualType={recordActualType(record)}
          onEdit={readOnly || !onWriteField ? undefined : (path, val) => { onWriteField(record.coordinate, path, val) }}
          onCollectionEdit={readOnly || !onCollectionEdit ? undefined : (path, edit) => { onCollectionEdit(record.coordinate, path, edit) }}
          diagnostics={fieldDiags}
          highlightField={highlightField === RECORD_HIGHLIGHT_SENTINEL ? null : highlightField}
          expandAlongPath={highlightField && highlightField !== RECORD_HIGHLIGHT_SENTINEL ? highlightField : null}
          onHighlightConsumed={onHighlightConsumed}
          onDiagnosticBadgeClick={onDiagnosticBadgeClick
            ? (topPath) => onDiagnosticBadgeClick(record.coordinate, topPath)
            : undefined}
        />
      </div>
      {showNewRecord && onInsertRecord && onCreateRecordDraft && (
        <CreateRecordDialog
          actualType={newRecordType}
          existingKeys={data.records.map(r => r.coordinate.key)}
          onCreateRecordDraft={onCreateRecordDraft}
          onInsertRecord={onInsertRecord}
          onClose={() => setShowNewRecord(false)}
        />
      )}
    </div>
  )
}

function cssEscape(s: string): string {
  if (typeof CSS !== 'undefined' && typeof CSS.escape === 'function') return CSS.escape(s)
  return s.replace(/["\\]/g, '\\$&')
}

