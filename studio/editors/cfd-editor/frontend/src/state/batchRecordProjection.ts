import type { FieldCell } from '../bindings/FieldCell'
import type { RecordRow } from '../bindings/RecordRow'
import { cellDeclaredType, cellReadOnly } from '../wire'
import { sameFieldValue } from './fieldProjection'

export interface BatchFieldProjection {
  cell: FieldCell
  state: 'same' | 'mixed'
  editable: boolean
}
export interface BatchCellProjection {
  cell: FieldCell
  state: 'same' | 'mixed'
  editable: boolean
}

/** A range can share one editor only when its type and editor annotations agree. */
export function projectBatchCells(cells: readonly FieldCell[]): BatchCellProjection | null {
  const first = cells[0]
  if (!first || cells.length < 2) return null
  const type = cellDeclaredType(first)
  const annotation = annotationKey(first)
  if (cells.some(cell => cellDeclaredType(cell) !== type || annotationKey(cell) !== annotation)) return null
  return {
    cell: first,
    state: cells.every(cell => sameFieldValue(cell.value, first.value)) ? 'same' : 'mixed',
    editable: cells.every(cell => !cellReadOnly(cell)),
  }
}

function annotationKey(cell: FieldCell): string {
  return JSON.stringify(cell.annotation, (_key, value) =>
    typeof value === 'bigint' ? `${value}n` : value,
  )
}

export function projectBatchRecordFields(records: readonly RecordRow[]): BatchFieldProjection[] {
  const first = records[0]
  if (!first || records.length < 2) return []
  return first.fields.flatMap(cell => {
    const cells = records.map(record => record.fields.find(item => item.name === cell.name))
    if (cells.some(item => !item)) return []
    const present = cells as FieldCell[]
    const declaredType = cellDeclaredType(cell)
    if (present.some(item => cellDeclaredType(item) !== declaredType)) return []
    return [{
      cell,
      state: present.every(item => sameFieldValue(item.value, cell.value)) ? 'same' : 'mixed',
      editable: present.every(item => !cellReadOnly(item)),
    }]
  })
}
