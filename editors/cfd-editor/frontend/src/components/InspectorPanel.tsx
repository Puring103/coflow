import { useCallback, useEffect, useRef, useState } from 'react'
import type { FileRecords } from '../bindings/FileRecords'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import {
  recordActualType,
  recordKey,
  sameCoordinate,
  type DiagnosticItem,
  type FieldPathSegment,
  type FieldValue,
} from '../wire'
import { CardHeader, DataCardExpanded } from './DataCard'
import { Icon } from './Icon'
import { diagnosticsForRecord } from '../App'

interface Props {
  open: boolean
  collapsed: boolean
  onToggleCollapse: () => void
  data: FileRecords | null
  coordinate: RecordCoordinate | null
  readOnly?: boolean
  diagnostics?: DiagnosticItem[]
  width: number
  onWidthChange: (w: number) => void
  onClose: () => void
  onWriteField?: (
    filePath: string,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue,
  ) => Promise<RecordRow | void>
  onRenameRecord?: (
    filePath: string,
    coordinate: RecordCoordinate,
    newKey: string,
  ) => Promise<RecordRow | void>
}

const MIN_W = 280
const MAX_W = 720

export function InspectorPanel({
  open,
  collapsed,
  onToggleCollapse,
  data,
  coordinate,
  readOnly,
  diagnostics,
  width,
  onWidthChange,
  onClose,
  onWriteField,
  onRenameRecord,
}: Props) {
  const [dragging, setDragging] = useState(false)
  const widthRef = useRef(width)
  widthRef.current = width

  const onSplitterDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setDragging(true)
    const startX = e.clientX
    const startW = widthRef.current
    const onMove = (ev: MouseEvent) => {
      const next = Math.min(MAX_W, Math.max(MIN_W, startW - (ev.clientX - startX)))
      onWidthChange(next)
    }
    const onUp = () => {
      setDragging(false)
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }, [onWidthChange])

  const record = data && coordinate
    ? data.records.find(r => sameCoordinate(r.coordinate, coordinate))
    : null

  const fieldDiags = record && data && diagnostics
    ? diagnosticsForRecord(diagnostics, data.file_path, record.coordinate)
    : []

  const canRename = !readOnly && data?.capabilities.can_edit_key && !!onRenameRecord

  if (!open) return null

  return (
    <aside
      className={`inspector-panel${collapsed ? ' collapsed' : ''}${dragging ? ' dragging' : ''}`}
      style={collapsed ? undefined : { width }}
      aria-label="记录详情面板"
    >
      <div
        className="inspector-splitter"
        onMouseDown={collapsed ? undefined : onSplitterDown}
        role="separator"
        aria-orientation="vertical"
        aria-label="调整记录面板宽度"
        tabIndex={collapsed ? -1 : 0}
        onKeyDown={e => {
          if (e.key === 'ArrowLeft') onWidthChange(Math.min(MAX_W, width + 24))
          if (e.key === 'ArrowRight') onWidthChange(Math.max(MIN_W, width - 24))
        }}
      />
      <div className="inspector-head">
        <button
          className="btn btn-icon inspector-collapse-btn"
          onClick={onToggleCollapse}
          title={collapsed ? '展开面板' : '折叠面板'}
          aria-label={collapsed ? '展开面板' : '折叠面板'}
        >
          <Icon name="chevron-right" size={13} className={collapsed ? '' : 'icon-flip-h'} />
        </button>
        {!collapsed && <span className="inspector-title">记录详情</span>}
        {!collapsed && (
          <button
            className="btn btn-icon"
            onClick={onClose}
            title="关闭"
            aria-label="关闭记录面板"
          >
            <Icon name="close" size={13} />
          </button>
        )}
      </div>
      {!collapsed && (
        <div className="inspector-body">
          {record && data ? (
            <>
              <CardHeader
                recordKey={recordKey(record)}
                actualType={recordActualType(record)}
                filePath={data.file_path}
                onRename={canRename && onRenameRecord
                  ? async (next) => { await onRenameRecord(data.file_path, record.coordinate, next) }
                  : undefined}
              />
              <DataCardExpanded
                fields={record.fields}
                actualType={recordActualType(record)}
                fieldModes={data.field_modes}
                onEdit={readOnly || !onWriteField
                  ? undefined
                  : (path, val) => { onWriteField(data.file_path, record.coordinate, path, val) }}
                diagnostics={fieldDiags}
              />
            </>
          ) : (
            <div className="empty-hint">未选择记录</div>
          )}
        </div>
      )}
    </aside>
  )
}
