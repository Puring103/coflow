import { useState } from 'react'
import type { FileRecords, FieldValue, RecordRow, FieldPathSegment } from '../bindings/index'
import { DataCardExpanded, CardHeader } from './DataCard'
import { Icon } from './Icon'
import { typeColor } from '../utils/typeColor'

interface Props {
  data: FileRecords
  recordKey: string
  typeFilter?: string
  readOnly?: boolean
  onOpenRecord: (key: string) => void
  onWriteField?: (recordKey: string, fieldPath: FieldPathSegment[], newValue: FieldValue) => Promise<RecordRow | void>
}

export function RecordView({ data, recordKey, typeFilter, readOnly, onOpenRecord, onWriteField }: Props) {
  const record = data.records.find(r => r.key === recordKey)
  const [search, setSearch] = useState('')

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

  return (
    <div className="record-view">
      <div className="rv-sidebar">
        {sidebarRecords.map(r => (
          <div
            key={r.key}
            className={`rv-sidebar-item${r.key === recordKey ? ' selected' : ''}`}
            onClick={() => onOpenRecord(r.key)}
          >
            <span className="rv-item-type" style={{ color: typeColor(r.actual_type) }}>{r.actual_type}</span>
            <span className="rv-item-key">{r.key}</span>
          </div>
        ))}
      </div>

      <div className="rv-main">
        <CardHeader recordKey={record.key} actualType={record.actual_type} filePath={data.file_path} />
        {showSearch && (
          <div className="rv-search-bar">
            <Icon name="search" size={13} className="rv-search-icon" />
            <input
              placeholder="搜索字段"
              value={search}
              onChange={e => setSearch(e.target.value)}
            />
            {search && (
              <button className="rv-clear-search" onClick={() => setSearch('')}>
                <Icon name="close" size={13} />
              </button>
            )}
          </div>
        )}
        <DataCardExpanded
          fields={fields}
          onEdit={readOnly || !onWriteField ? undefined : (path, val) => { onWriteField(record.key, path, val) }}
        />
      </div>
    </div>
  )
}
