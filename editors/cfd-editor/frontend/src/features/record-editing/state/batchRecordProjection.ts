import type { FieldCell } from '../../../bindings/FieldCell'
import type { RecordRow } from '../../../bindings/RecordRow'
import { cellDeclaredType, cellReadOnly } from '../../../wire'
import { sameFieldValue } from './fieldProjection'

export interface BatchFieldProjection {
  cell: FieldCell
  state: 'same' | 'mixed'
  editable: boolean
}
export interface BatchCellProjection {
  count: number
  cell: FieldCell
  state: 'same' | 'mixed'
  editable: boolean
}

/** A range can share one editor only when its type and editor annotations agree. */
export function projectBatchCells(cells: Iterable<FieldCell>): BatchCellProjection | null {
  let first: FieldCell | undefined
  let count = 0
  let state: 'same' | 'mixed' = 'same'
  let editable = true
  let type: ReturnType<typeof cellDeclaredType>
  let annotation = ''
  // 单遍聚合；不兼容时立即结束，不为选区创建中间数组。
  for (const cell of cells) {
    count++
    if (!first) { first = cell; type = cellDeclaredType(cell); annotation = annotationKey(cell) }
    else if (cellDeclaredType(cell) !== type || annotationKey(cell) !== annotation) return null
    if (state === 'same' && !sameFieldValue(cell.value, first.value)) state = 'mixed'
    if (cellReadOnly(cell)) editable = false
  }
  return first && count > 1 ? { cell: first, state, editable, count } : null
}

const annotationKeys = new WeakMap<NonNullable<FieldCell['annotation']>, string>()
function annotationKey(cell: FieldCell): string {
  if (!cell.annotation) return 'null'
  const cached = annotationKeys.get(cell.annotation)
  if (cached !== undefined) return cached
  const key = JSON.stringify(cell.annotation, (_key, value) =>
    typeof value === 'bigint' ? `${value}n` : value,
  )
  annotationKeys.set(cell.annotation, key)
  return key
}

export function projectBatchRecordFields(records: readonly RecordRow[]): BatchFieldProjection[] {
  const first = records[0]
  if (!first || records.length < 2) return []
  return first.fields.flatMap(cell => {
    const cells = records.map(record => { const index = record.field_index[cell.name]; return index === undefined ? undefined : record.fields[index] })
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
