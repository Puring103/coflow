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
}

const MIN_W = 320
const MAX_W = 720

export function InspectorPanel({
  open,
  data,
  coordinate,
  readOnly,
  diagnostics,
  width,
  onWidthChange,
  onClose,
  onWriteField,
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
      // Splitter is on the LEFT edge of the panel; dragging left grows the panel.
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

  // Close on Escape when open.
  useEffect(() => {
    if (!open) return
    const h = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation()
        onClose()
      }
    }
    window.addEventListener('keydown', h)
    return () => window.removeEventListener('keydown', h)
  }, [open, onClose])

  const record = data && coordinate
    ? data.records.find(r => sameCoordinate(r.coordinate, coordinate))
    : null

  const fieldDiags = record && data && diagnostics
    ? diagnosticsForRecord(diagnostics, data.file_path, record.coordinate)
    : []

  return (
    <aside
      className={`inspector-panel${open ? ' open' : ''}${dragging ? ' dragging' : ''}`}
      style={{ width }}
      aria-hidden={!open}
    >
      <div
        className="inspector-splitter"
        onMouseDown={onSplitterDown}
        role="separator"
        aria-orientation="vertical"
        aria-label="调整记录面板宽度"
        tabIndex={open ? 0 : -1}
        onKeyDown={e => {
          if (e.key === 'ArrowLeft') onWidthChange(Math.min(MAX_W, width + 24))
          if (e.key === 'ArrowRight') onWidthChange(Math.max(MIN_W, width - 24))
        }}
      />
      <div className="inspector-head">
        <span className="inspector-title">记录详情</span>
        <button
          className="btn btn-icon"
          onClick={onClose}
          title="关闭 (Esc)"
          aria-label="关闭记录面板"
        >
          <Icon name="close" size={13} />
        </button>
      </div>
      <div className="inspector-body">
        {record && data ? (
          <>
            <CardHeader
              recordKey={recordKey(record)}
              actualType={recordActualType(record)}
              filePath={data.file_path}
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
    </aside>
  )
}
