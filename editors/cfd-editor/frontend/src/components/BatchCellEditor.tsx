import { useState } from 'react'
import type { BatchWriteFieldInput } from '../bindings/BatchWriteFieldInput'
import type { FieldCell } from '../bindings/FieldCell'
import type { CellAnchor } from '../state/editorSelection'
import { projectBatchCells } from '../state/batchRecordProjection'
import {
  cellDeclaredType,
  cellEnumType,
  cellNullable,
  cellRefTargetType,
  type FieldValue,
  type FieldPathSegment,
} from '../wire'
import { DataCardExpanded } from './DataCard'
import { MixedFieldRow } from './BatchRecordEditor'

interface Props {
  cells: readonly { anchor: CellAnchor; cell: FieldCell }[]
  readOnly?: boolean
  onWriteBatch?: (writes: readonly BatchWriteFieldInput[]) => Promise<void>
}

export function BatchCellEditor({ cells, readOnly, onWriteBatch }: Props) {
  const projection = projectBatchCells(cells.map(item => item.cell))
  const [busy, setBusy] = useState(false)
  if (!projection) return null
  const commit = async (value: FieldValue, relativePath: readonly FieldPathSegment[] = []) => {
    if (busy || !onWriteBatch) return
    setBusy(true)
    try {
      await onWriteBatch(cells.map(({ anchor }) => ({
        coordinate: anchor.coordinate,
        field_path: [...anchor.fieldPath, ...relativePath],
        new_value: value,
      })))
    } finally {
      setBusy(false)
    }
  }
  const editable = !readOnly && projection.editable && !!onWriteBatch && !busy
  const cell = projection.cell
  return (
    <div className="batch-record-editor">
      <div className="batch-record-header">
        <strong>已选择 {cells.length} 个单元格</strong>
        <span>{cellDeclaredType(cell) ?? '未知类型'}</span>
      </div>
      {projection.state === 'same' ? (
        <DataCardExpanded
          fields={[cell]}
          onEdit={editable ? (path, value) => { void commit(value, path.slice(1)) } : undefined}
        />
      ) : (
        <MixedFieldRow
          label="多个值"
          sample={cell.value}
          declaredType={cellDeclaredType(cell)}
          enumType={cellEnumType(cell)}
          refTargetType={cellRefTargetType(cell)}
          nullable={cellNullable(cell)}
          disabled={!editable}
          busy={busy}
          onCommit={value => { void commit(value) }}
        />
      )}
    </div>
  )
}
