import { useState, useEffect, useMemo } from 'react'
import { useReactTable, getCoreRowModel, flexRender, createColumnHelper } from '@tanstack/react-table'
import type { FileRecords, RecordRow, FieldValue, FieldPathSegment, DiagnosticItem } from '../bindings/index'
import { DataCardCompact, DataCardExpanded, CardHeader, InlineEditor } from './DataCard'
import { Icon } from './Icon'
import { diagnosticsForRecord } from '../App'

interface Props {
  data: FileRecords
  activeType: string
  readOnly?: boolean
  diagnostics?: DiagnosticItem[]
  onOpenRecord: (key: string) => void
  onWriteField?: (recordKey: string, fieldPath: FieldPathSegment[], newValue: FieldValue) => Promise<RecordRow | void>
}

export function TableView({ data, activeType, readOnly, diagnostics, onOpenRecord, onWriteField }: Props) {
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; key: string } | null>(null)
  const [showNewRecord, setShowNewRecord] = useState(false)
  const [newKey, setNewKey] = useState('')
  const [newType, setNewType] = useState<string>(activeType || data.type_names[0] || '')
  const [selectedKey, setSelectedKey] = useState<string | null>(null)

  // Reset selection when active file/type changes
  useEffect(() => {
    setSelectedKey(null)
  }, [data.file_path, activeType])

  useEffect(() => {
    setNewType(activeType || data.type_names[0] || '')
  }, [activeType, data.type_names])

  const filtered = useMemo(
    () => data.records.filter(r => r.actual_type === activeType),
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
      const key = top ? `${d.record_key}::${top}` : `${d.record_key}::*`
      const cur = m.get(key)
      const rank = (s: 'error' | 'warning' | 'info') => s === 'error' ? 3 : s === 'warning' ? 2 : 1
      if (!cur || rank(d.severity) > rank(cur)) m.set(key, d.severity)
    }
    return m
  }, [diagnostics, data.file_path])
  const recordSeverity = (key: string): 'error' | 'warning' | null => {
    let sev: 'error' | 'warning' | null = null
    for (const d of diagnostics ?? []) {
      if (d.file_path !== data.file_path || d.record_key !== key) continue
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
  const columns = useMemo(() => {
    const helper = createColumnHelper<RecordRow>()
    return [
      helper.accessor('key', {
        header: 'Key',
        cell: info => <span className="cell-key">{info.getValue()}</span>,
      }),
      ...allFieldNames.map(name =>
        helper.display({
          id: name,
          header: name,
          cell: ({ row }) => {
            const f = row.original.fields.find(f => f.name === name)
            const sev = cellDiagIndex.get(`${row.original.key}::${name}`)
            if (!f) return <span className={`dc-null${sev ? ' dc-cell-diag dc-cell-diag-' + sev : ''}`}>—</span>
            return (
              <span className={sev ? `dc-cell-diag dc-cell-diag-${sev}` : undefined} title={sev ? findDiagMessage(diagnostics, data.file_path, row.original.key, name) : undefined}>
                <EditableCell
                  value={f.value}
                  editable={canEdit}
                  onCommit={canEdit ? next => onWriteField!(row.original.key, [{ kind: 'field', name }], next) : undefined}
                />
              </span>
            )
          },
        })
      ),
    ]
  }, [allFieldNames, canEdit, onWriteField, cellDiagIndex, diagnostics, data.file_path])

  const table = useReactTable({
    data: filtered,
    columns,
    getCoreRowModel: getCoreRowModel(),
  })

  const selectedRecord = selectedKey ? filtered.find(r => r.key === selectedKey) ?? null : null

  return (
    <div className="table-view" onClick={() => setContextMenu(null)}>
      <div className={`table-main${selectedRecord ? ' has-detail' : ''}`}>
        <div className="table-scroll">
          <table className="data-table">
            <thead>
              {table.getHeaderGroups().map(hg => (
                <tr key={hg.id}>
                  {hg.headers.map(h => (
                    <th key={h.id}>{flexRender(h.column.columnDef.header, h.getContext())}</th>
                  ))}
                </tr>
              ))}
            </thead>
            <tbody>
              {table.getRowModel().rows.map(row => {
                const rowSev = recordSeverity(row.original.key)
                return (
                <tr
                  key={row.id}
                  className={`table-row${selectedKey === row.original.key ? ' selected' : ''}${rowSev ? ' table-row-' + rowSev : ''}`}
                  onClick={() => setSelectedKey(row.original.key)}
                  onContextMenu={e => {
                    e.preventDefault()
                    setContextMenu({ x: e.clientX, y: e.clientY, key: row.original.key })
                  }}
                >
                  {row.getVisibleCells().map(cell => (
                    <td key={cell.id}>
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </td>
                  ))}
                </tr>
                )
              })}
            </tbody>
          </table>
          {filtered.length === 0 && (
            <div className="empty-hint">暂无 {activeType} 类型的记录</div>
          )}
        </div>

        <div className="table-footer">
          {readOnly ? (
            <span className="table-footer-readonly">该文件为只读</span>
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
              />
              <select value={newType} onChange={e => setNewType(e.target.value)}>
                {data.type_names.map(t => <option key={t} value={t}>{t}</option>)}
              </select>
              <button className="btn btn-primary" onClick={() => {
                alert(`创建记录 ${newKey} (${newType}) — 原型演示`)
                setShowNewRecord(false); setNewKey('')
              }}>创建</button>
              <button className="btn" onClick={() => { setShowNewRecord(false); setNewKey('') }}>
                <Icon name="close" size={13} />
              </button>
            </span>
          )}
        </div>
      </div>

      {selectedRecord && (
        <aside className="table-detail">
          <div className="table-detail-header">
            <CardHeader recordKey={selectedRecord.key} actualType={selectedRecord.actual_type} filePath={data.file_path} />
            <button className="btn btn-icon table-detail-close" onClick={() => setSelectedKey(null)} title="关闭面板">
              <Icon name="close" size={13} />
            </button>
          </div>
          <div className="table-detail-body">
            <DataCardExpanded
              fields={selectedRecord.fields}
              onEdit={readOnly || !onWriteField ? undefined : (path, val) => { onWriteField(selectedRecord.key, path, val) }}
              diagnostics={diagnostics ? diagnosticsForRecord(diagnostics, data.file_path, selectedRecord.key) : []}
            />
          </div>
        </aside>
      )}

      {contextMenu && (
        <div
          className="context-menu"
          style={{ left: contextMenu.x, top: contextMenu.y }}
          onClick={e => e.stopPropagation()}
        >
          <div className="ctx-item" onClick={() => { onOpenRecord(contextMenu.key); setContextMenu(null) }}>
            <Icon name="record" size={13} />
            跳转到记录视图
          </div>
          {!readOnly && (
            <div className="ctx-item ctx-danger" onClick={() => {
              alert(`删除 ${contextMenu.key} — 原型演示`)
              setContextMenu(null)
            }}>
              <Icon name="close" size={13} />
              删除记录
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function findDiagMessage(
  diags: DiagnosticItem[] | undefined,
  filePath: string,
  recordKey: string,
  topField: string,
): string | undefined {
  if (!diags) return undefined
  const msgs: string[] = []
  for (const d of diags) {
    if (d.file_path !== filePath || d.record_key !== recordKey) continue
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
  const isScalar = value.kind === 'Bool' || value.kind === 'Int' || value.kind === 'Float'
                || value.kind === 'Str' || value.kind === 'Enum' || value.kind === 'Ref'
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
      onDoubleClick={canEdit ? () => setEditing(true) : undefined}
      title={canEdit ? '双击编辑' : undefined}
    >
      <DataCardCompact value={value} />
    </div>
  )
}
