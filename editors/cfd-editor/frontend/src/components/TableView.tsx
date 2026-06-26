import { useState, useEffect, useMemo, useRef } from 'react'
import {
  useReactTable,
  getCoreRowModel,
  getSortedRowModel,
  getFilteredRowModel,
  flexRender,
  createColumnHelper,
  type SortingState,
  type ColumnSizingState,
} from '@tanstack/react-table'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { FileRecords } from '../bindings/FileRecords'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import {
  coordinateId,
  diagnosticMatchesCoordinate,
  diagnosticSeverity,
  fieldPathField,
  makeObjectValue,
  recordActualType,
  recordKey,
  type DiagnosticItem,
  type FieldPathSegment,
  type FieldValue,
} from '../wire'
import { DataCardCompact, DataCardExpanded, CardHeader, InlineEditor, summaryOf } from './DataCard'
import { Icon } from './Icon'
import { diagnosticsForRecord } from '../App'

interface Props {
  data: FileRecords
  activeType: string
  readOnly?: boolean
  diagnostics?: DiagnosticItem[]
  onOpenRecord: (coordinate: RecordCoordinate) => void
  onWriteField?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], newValue: FieldValue) => Promise<RecordRow | void>
  onRenameRecord?: (coordinate: RecordCoordinate, newKey: string) => Promise<RecordRow | void>
  /** Create a new record. Resolves once the back-end has persisted and the
   *  parent has refreshed `data` for this file. */
  onInsertRecord?: (recordKey: string, actualType: string, fields: FieldValue) => Promise<void>
  /** Delete an existing record by key. */
  onDeleteRecord?: (coordinate: RecordCoordinate) => Promise<void>
  /** Build a defaulted Object FieldValue for a given type — used to seed
   *  the "new record" payload before sending it to the back-end. */
  onMakeDefaultObject?: (typeName: string) => Promise<FieldValue | null>
}

const ROW_H = 30

export function TableView({ data, activeType, readOnly, diagnostics, onOpenRecord, onWriteField, onRenameRecord, onInsertRecord, onDeleteRecord, onMakeDefaultObject }: Props) {
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; row: RecordRow } | null>(null)
  const [showNewRecord, setShowNewRecord] = useState(false)
  const [newKey, setNewKey] = useState('')
  const [newType, setNewType] = useState<string>(activeType || data.type_names[0] || '')
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [sorting, setSorting] = useState<SortingState>([])
  const [columnSizing, setColumnSizing] = useState<ColumnSizingState>({})
  const [globalFilter, setGlobalFilter] = useState('')

  const tableScrollRef = useRef<HTMLDivElement>(null)

  // Reset transient UI state when active file/type changes.
  useEffect(() => {
    setSelectedId(null)
    setSorting([])
    setGlobalFilter('')
  }, [data.file_path, activeType])

  useEffect(() => {
    setNewType(activeType || data.type_names[0] || '')
  }, [activeType, data.type_names])

  const filtered = useMemo(
    () => data.records.filter(r => recordActualType(r) === activeType),
    [data.records, activeType]
  )

  // Build a (recordKey, topLevelFieldName) → severity index for this file so
  // table cells can light up red/yellow without recomputing on every render.
  const cellDiagIndex = useMemo(() => {
    const m = new Map<string, 'error' | 'warning' | 'info'>()
    if (!diagnostics) return m
    for (const d of diagnostics) {
      if (d.file_path !== data.file_path || !d.record_key) continue
      // Take the first path segment as the column we'll mark.
      const top = d.field_path
        ? d.field_path.split(/[.[]/, 1)[0]
        : null
      const coordinates = d.actual_type === null
        ? data.records
            .filter(r => r.coordinate.key === d.record_key)
            .map(r => r.coordinate)
        : [{ actual_type: d.actual_type, key: d.record_key }]
      const rank = (s: 'error' | 'warning' | 'info') => s === 'error' ? 3 : s === 'warning' ? 2 : 1
      const severity = diagnosticSeverity(d.severity)
      for (const coordinate of coordinates) {
        const coordKey = coordinateId(coordinate)
        const key = top ? `${coordKey}::${top}` : `${coordKey}::*`
        const cur = m.get(key)
        if (!cur || rank(severity) > rank(cur)) m.set(key, severity)
      }
    }
    return m
  }, [diagnostics, data.file_path, data.records])
  const recordSeverity = (coordinate: RecordCoordinate): 'error' | 'warning' | null => {
    let sev: 'error' | 'warning' | null = null
    for (const d of diagnostics ?? []) {
      if (d.file_path !== data.file_path || !diagnosticMatchesCoordinate(d, coordinate)) continue
      if (d.severity === 'error') return 'error'
      if (d.severity === 'warning') sev = 'warning'
    }
    return sev
  }

  const allFieldNames = useMemo(
    () => Array.from(new Set(filtered.flatMap(r => r.fields.map(f => f.name)))),
    [filtered]
  )

  const canEdit = !readOnly && !!onWriteField
  const canRename = !readOnly && data.capabilities.can_edit_key && !!onRenameRecord
  const columns = useMemo(() => {
    const helper = createColumnHelper<RecordRow>()
    return [
      helper.accessor(row => recordKey(row), {
        id: 'key',
        header: 'Key',
        cell: info => (
          <EditableKeyCell
            value={info.getValue()}
            editable={canRename}
            onCommit={canRename ? next => onRenameRecord!(info.row.original.coordinate, next) : undefined}
          />
        ),
        size: 140,
      }),
      ...allFieldNames.map(name =>
        helper.display({
          id: name,
          header: name,
          cell: ({ row }) => {
            const f = row.original.fields.find(f => f.name === name)
            const sev = cellDiagIndex.get(`${coordinateId(row.original.coordinate)}::${name}`)
            if (!f) return <span className={`dc-null${sev ? ' dc-cell-diag dc-cell-diag-' + sev : ''}`}>—</span>
            const isDimensionDefault = isDimensionDefaultField(row.original, f.name)
            const cellEditable = canEdit && !isDimensionDefault
            const title = isDimensionDefault
              ? '由源记录决定，不可编辑'
              : sev ? findDiagMessage(diagnostics, data.file_path, row.original.coordinate, name) : undefined
            return (
              <span className={sev ? `dc-cell-diag dc-cell-diag-${sev}` : undefined} title={title}>
                <EditableCell
                  value={f.value}
                  editable={cellEditable}
                  onCommit={cellEditable ? next => onWriteField!(row.original.coordinate, [fieldPathField(name)], next) : undefined}
                />
              </span>
            )
          },
        }),
      ),
    ]
  }, [allFieldNames, canEdit, canRename, onWriteField, onRenameRecord, cellDiagIndex, diagnostics, data.file_path])

  // Global filter: match key or any scalar field value (via summaryOf).
  const globalFilterFn = useMemo(
    () => (row: { original: RecordRow }, _columnId: string, filterValue: string) => {
      const q = filterValue.trim().toLowerCase()
      if (!q) return true
      const r = row.original
      if (recordKey(r).toLowerCase().includes(q)) return true
      for (const f of r.fields) {
        const s = summaryOf(f.value).toLowerCase()
        if (s.includes(q)) return true
      }
      return false
    },
  [],
  )

  const table = useReactTable({
    data: filtered,
    columns,
    state: { sorting, columnSizing, globalFilter },
    onSortingChange: setSorting,
    onColumnSizingChange: setColumnSizing,
    onGlobalFilterChange: setGlobalFilter,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    getRowId: row => coordinateId(row.coordinate),
    globalFilterFn,
    columnResizeMode: 'onChange',
    enableColumnResizing: true,
  })

  const rows = table.getRowModel().rows
  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => tableScrollRef.current,
    estimateSize: () => ROW_H,
    overscan: 12,
  })
  const virtualRows = rowVirtualizer.getVirtualItems()
  const totalHeight = rowVirtualizer.getTotalSize()
  const padBefore = virtualRows.length > 0 ? virtualRows[0].start : 0
  const padAfter = virtualRows.length > 0 ? totalHeight - virtualRows[virtualRows.length - 1].end : 0

  const selectedRecord = selectedId ? filtered.find(r => coordinateId(r.coordinate) === selectedId) ?? null : null

  // Close context menu on Escape.
  useEffect(() => {
    if (!contextMenu) return
    const h = (e: KeyboardEvent) => { if (e.key === 'Escape') setContextMenu(null) }
    window.addEventListener('keydown', h)
    return () => window.removeEventListener('keydown', h)
  }, [contextMenu])

  return (
    <div className="table-view" onClick={() => setContextMenu(null)}>
      <div className={`table-main${selectedRecord ? ' has-detail' : ''}`}>
        {(filtered.length > 8 || globalFilter) && (
          <div className="table-toolbar">
            <div className="table-search">
              <Icon name="search" size={13} className="table-search-icon" aria-hidden />
              <input
                placeholder={`搜索 ${activeType}…`}
                value={globalFilter}
                onChange={e => setGlobalFilter(e.target.value)}
                aria-label="搜索记录"
              />
              {globalFilter && (
                <button className="rv-clear-search" onClick={() => setGlobalFilter('')} aria-label="清除搜索">
                  <Icon name="close" size={13} aria-hidden />
                </button>
              )}
            </div>
            <span className="table-row-count">
              {rows.length}{rows.length !== filtered.length ? ` / ${filtered.length}` : ''} 条
            </span>
          </div>
        )}

        <div className="table-scroll" ref={tableScrollRef}>
          <table className="data-table" style={{ width: table.getTotalSize() }}>
            <thead>
              {table.getHeaderGroups().map(hg => (
                <tr key={hg.id}>
                  {hg.headers.map(h => {
                    const sort = h.column.getIsSorted()
                    return (
                      <th
                        key={h.id}
                        style={{ width: h.getSize() }}
                        aria-sort={sort === 'asc' ? 'ascending' : sort === 'desc' ? 'descending' : 'none'}
                      >
                        <button
                          type="button"
                          className="th-sort-btn"
                          onClick={h.column.getToggleSortingHandler()}
                          disabled={!h.column.getCanSort()}
                          title={h.column.getCanSort() ? '点击排序' : undefined}
                        >
                          {flexRender(h.column.columnDef.header, h.getContext())}
                          {sort === 'asc' && <Icon name="chevron-down" size={10} className="th-sort-icon asc" aria-hidden />}
                          {sort === 'desc' && <Icon name="chevron-right" size={10} className="th-sort-icon desc" aria-hidden />}
                        </button>
                        {h.column.getCanResize() && (
                          <div
                            className="th-resizer"
                            onMouseDown={h.getResizeHandler()}
                            onClick={e => e.stopPropagation()}
                            aria-hidden
                          />
                        )}
                      </th>
                    )
                  })}
                </tr>
              ))}
            </thead>
            <tbody>
              {padBefore > 0 && (
                <tr style={{ height: padBefore }}>
                  <td colSpan={columns.length} aria-hidden />
                </tr>
              )}
              {virtualRows.map(vr => {
                const row = rows[vr.index]
                const rowSev = recordSeverity(row.original.coordinate)
                return (
                  <tr
                    key={row.id}
                    className={`table-row${selectedId === coordinateId(row.original.coordinate) ? ' selected' : ''}${rowSev ? ' table-row-' + rowSev : ''}`}
                    onClick={() => setSelectedId(coordinateId(row.original.coordinate))}
                    onContextMenu={e => {
                      e.preventDefault()
                      setContextMenu({ x: e.clientX, y: e.clientY, row: row.original })
                    }}
                  >
                    {row.getVisibleCells().map(cell => (
                      <td key={cell.id} style={{ width: cell.column.getSize() }}>
                        {flexRender(cell.column.columnDef.cell, cell.getContext())}
                      </td>
                    ))}
                  </tr>
                )
              })}
              {padAfter > 0 && (
                <tr style={{ height: padAfter }}>
                  <td colSpan={columns.length} aria-hidden />
                </tr>
              )}
            </tbody>
          </table>
          {filtered.length === 0 && (
            <div className="empty-hint">暂无 {activeType} 类型的记录</div>
          )}
          {filtered.length > 0 && rows.length === 0 && (
            <div className="empty-hint">无匹配 "{globalFilter}" 的记录</div>
          )}
        </div>

        <div className="table-footer">
          {readOnly ? (
            <span className="table-footer-readonly">该文件为只读</span>
          ) : !data.capabilities.can_insert_record || !onInsertRecord ? (
            <span className="table-footer-readonly">该来源不支持新建记录</span>
          ) : !showNewRecord ? (
            <button className="btn btn-outlined" onClick={() => setShowNewRecord(true)}>
              <Icon name="plus" size={13} />
              新建记录
            </button>
          ) : (
            <span className="new-record-form">
              <input
                placeholder="记录 Key"
                value={newKey}
                autoFocus
                onChange={e => setNewKey(e.target.value)}
                onKeyDown={e => { if (e.key === 'Escape') setShowNewRecord(false) }}
                aria-label="新记录 Key"
              />
              <select value={newType} onChange={e => setNewType(e.target.value)} aria-label="新记录类型">
                {data.type_names.map(t => <option key={t} value={t}>{t}</option>)}
              </select>
              <button
                className="btn btn-primary"
                disabled={!newKey.trim() || !data.capabilities.can_insert_record || !onInsertRecord}
                onClick={async () => {
                  const key = newKey.trim()
                  if (!key || !onInsertRecord) return
                  const seedFields = onMakeDefaultObject
                    ? await onMakeDefaultObject(newType)
                    : null
                  const fields: FieldValue = seedFields ?? makeObjectValue(newType)
                  await onInsertRecord(key, newType, fields)
                  setShowNewRecord(false); setNewKey('')
                }}
              >创建</button>
              <button className="btn" onClick={() => { setShowNewRecord(false); setNewKey('') }} aria-label="取消新建">
                <Icon name="close" size={13} />
              </button>
            </span>
          )}
        </div>
      </div>

      {selectedRecord && (
        <aside className="table-detail">
          <div className="table-detail-header">
            <CardHeader recordKey={recordKey(selectedRecord)} actualType={recordActualType(selectedRecord)} filePath={data.file_path} />
            <button className="btn btn-icon table-detail-close" onClick={() => setSelectedId(null)} title="关闭面板" aria-label="关闭详情面板">
              <Icon name="close" size={13} />
            </button>
          </div>
          <div className="table-detail-body">
            <DataCardExpanded
              fields={selectedRecord.fields}
              actualType={recordActualType(selectedRecord)}
              onEdit={readOnly || !onWriteField ? undefined : (path, val) => { onWriteField(selectedRecord.coordinate, path, val) }}
              diagnostics={diagnostics ? diagnosticsForRecord(diagnostics, data.file_path, selectedRecord.coordinate) : []}
            />
          </div>
        </aside>
      )}

      {contextMenu && (
        <div
          className="context-menu"
          style={{ left: contextMenu.x, top: contextMenu.y }}
          onClick={e => e.stopPropagation()}
          role="menu"
        >
          <div className="ctx-item" role="menuitem" onClick={() => { onOpenRecord(contextMenu.row.coordinate); setContextMenu(null) }}>
            <Icon name="record" size={13} aria-hidden />
            跳转到记录视图
          </div>
          {!readOnly && data.capabilities.can_edit_key && onRenameRecord && (
            <div className="ctx-item" role="menuitem" onClick={async () => {
              const key = recordKey(contextMenu.row)
              const next = window.prompt('重命名 Key', key)?.trim()
              const coordinate = contextMenu.row.coordinate
              setContextMenu(null)
              if (!next || next === key) return
              await onRenameRecord(coordinate, next)
            }}>
              <Icon name="edit" size={13} aria-hidden />
              重命名 Key
            </div>
          )}
          {!readOnly && data.capabilities.can_delete_record && onDeleteRecord && (
            <div className="ctx-item ctx-danger" role="menuitem" onClick={async () => {
              const key = recordKey(contextMenu.row)
              const coordinate = contextMenu.row.coordinate
              setContextMenu(null)
              if (!window.confirm(`确认删除记录 ${key}？此操作不可撤销。`)) return
              if (selectedId === coordinateId(coordinate)) setSelectedId(null)
              await onDeleteRecord(coordinate)
            }}>
              <Icon name="close" size={13} aria-hidden />
              删除记录
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function isDimensionDefaultField(record: RecordRow, fieldName: string): boolean {
  return recordActualType(record).endsWith('Variants') && fieldName === 'default'
}

function findDiagMessage(
  diags: DiagnosticItem[] | undefined,
  filePath: string,
  coordinate: RecordCoordinate,
  topField: string,
): string | undefined {
  if (!diags) return undefined
  const msgs: string[] = []
  for (const d of diags) {
    if (d.file_path !== filePath || !diagnosticMatchesCoordinate(d, coordinate)) continue
    const top = d.field_path ? d.field_path.split(/[.[]/, 1)[0] : null
    if (top !== topField) continue
    msgs.push(d.message)
  }
  return msgs.length ? msgs.join('\n') : undefined
}

function EditableCell({
  value, editable, onCommit,
}: {
  value: FieldValue
  editable: boolean
  onCommit?: (next: FieldValue) => void
}) {
  const [editing, setEditing] = useState(false)
  const isScalar = value.kind === 'bool' || value.kind === 'int' || value.kind === 'float'
                || value.kind === 'string' || value.kind === 'enum' || value.kind === 'ref'
  const canEdit = editable && isScalar && !!onCommit

  if (editing && canEdit) {
    return (
      <InlineEditor
        value={value}
        onCommit={next => { onCommit!(next); setEditing(false) }}
        onCancel={() => setEditing(false)}
      />
    )
  }
  return (
    <div
      className={`cell-edit-wrap${canEdit ? ' editable' : ''}`}
      onClick={canEdit ? (e: React.MouseEvent) => {
        // Single-click enters edit mode (matches RecordView's DirectEditor).
        // stopPropagation so the row's selection handler doesn't also fire
        // and steal focus away from the freshly-mounted input.
        e.stopPropagation()
        setEditing(true)
      } : undefined}
      onDoubleClick={canEdit ? () => setEditing(true) : undefined}
      title={canEdit ? '点击编辑' : undefined}
    >
      <DataCardCompact value={value} />
    </div>
  )
}

function EditableKeyCell({
  value, editable, onCommit,
}: {
  value: string
  editable: boolean
  onCommit?: (next: string) => void
}) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(value)

  useEffect(() => {
    if (!editing) setDraft(value)
  }, [value, editing])

  const commit = () => {
    const next = draft.trim()
    if (next && next !== value && onCommit) onCommit(next)
    setEditing(false)
  }

  if (editing && editable) {
    return (
      <input
        className="inline-editor key-editor"
        value={draft}
        autoFocus
        onChange={e => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={e => {
          if (e.key === 'Enter') commit()
          if (e.key === 'Escape') {
            setDraft(value)
            setEditing(false)
          }
        }}
        onClick={e => e.stopPropagation()}
        aria-label="重命名记录 Key"
      />
    )
  }

  return (
    <span
      className={`cell-key${editable ? ' editable' : ''}`}
      onClick={editable ? e => {
        e.stopPropagation()
        setEditing(true)
      } : undefined}
      title={editable ? '点击重命名 Key' : undefined}
    >
      {value}
    </span>
  )
}
