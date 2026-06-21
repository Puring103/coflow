import { useState, useEffect, useMemo, type CSSProperties } from 'react'
import { useReactTable, getCoreRowModel, flexRender, createColumnHelper } from '@tanstack/react-table'
import type { FileRecords, RecordRow } from '../bindings/index'
import { DataCardCompact, DataCardExpanded } from './DataCard'
import { Icon } from './Icon'
import { typeColor } from '../utils/typeColor'

interface Props {
  data: FileRecords
  activeType: string
  onOpenRecord: (key: string) => void
}

export function TableView({ data, activeType, onOpenRecord }: Props) {
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

  const allFieldNames = useMemo(
    () => Array.from(new Set(filtered.flatMap(r => r.fields.map(f => f.name)))),
    [filtered]
  )

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
            return f ? <DataCardCompact value={f.value} /> : <span className="dc-null">—</span>
          },
        })
      ),
    ]
  }, [allFieldNames])

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
              {table.getRowModel().rows.map(row => (
                <tr
                  key={row.id}
                  className={`table-row${selectedKey === row.original.key ? ' selected' : ''}`}
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
              ))}
            </tbody>
          </table>
          {filtered.length === 0 && (
            <div className="empty-hint">暂无 {activeType} 类型的记录</div>
          )}
        </div>

        <div className="table-footer">
          {!showNewRecord ? (
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
            <span className="rv-type-badge" style={{ '--type-color': typeColor(selectedRecord.actual_type) } as CSSProperties}>{selectedRecord.actual_type}</span>
            <span className="rv-key">{selectedRecord.key}</span>
            <span className="topbar-spacer" />
            <button className="btn btn-icon" onClick={() => setSelectedKey(null)} title="关闭面板">
              <Icon name="close" size={13} />
            </button>
          </div>
          <div className="table-detail-body">
            <DataCardExpanded
              fields={selectedRecord.fields}
              onEdit={(name, val) => alert(`写入 ${selectedRecord.key}.${name} = ${val} — 原型演示`)}
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
          <div className="ctx-item ctx-danger" onClick={() => {
            alert(`删除 ${contextMenu.key} — 原型演示`)
            setContextMenu(null)
          }}>
            <Icon name="close" size={13} />
            删除记录
          </div>
        </div>
      )}
    </div>
  )
}
