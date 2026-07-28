import type { FieldCell } from '../bindings/FieldCell'
import type { CfdDictKey } from '../bindings/CfdDictKey'
import type { RecordRow } from '../bindings/RecordRow'
import { cellDeclaredType, cellReadOnly, type FieldValue } from '../wire'

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
    state: cells.every(cell => fieldValuesEqual(cell.value, first.value)) ? 'same' : 'mixed',
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
      state: present.every(item => fieldValuesEqual(item.value, cell.value)) ? 'same' : 'mixed',
      editable: present.every(item => !cellReadOnly(item)),
    }]
  })
}

export function fieldValuesEqual(left: FieldValue, right: FieldValue): boolean {
  if (left.kind !== right.kind) return false
  switch (left.kind) {
    case 'null': return true
    case 'bool': return right.kind === 'bool' && left.value === right.value
    case 'int': return right.kind === 'int' && left.value === right.value
    case 'float': return right.kind === 'float' && left.value === right.value
    case 'string': return right.kind === 'string' && left.value === right.value
    case 'enum': return right.kind === 'enum'
      && left.value.enum_name === right.value.enum_name
      && left.value.variant === right.value.variant
      && left.value.value === right.value.value
    case 'ref': return right.kind === 'ref' && left.value === right.value
    case 'object': {
      if (right.kind !== 'object' || left.value.actual_type !== right.value.actual_type) return false
      const leftEntries = Object.entries(left.value.fields)
      const rightEntries = Object.entries(right.value.fields)
      return leftEntries.length === rightEntries.length && leftEntries.every(([name, value]) => {
        const other = right.value.fields[name]
        return value === undefined || value === null
          ? other === undefined || other === null
          : !!other && fieldValuesEqual(value, other)
      })
    }
    case 'array': return right.kind === 'array'
      && left.value.length === right.value.length
      && left.value.every((value, index) => fieldValuesEqual(value, right.value[index]))
    case 'dict': return right.kind === 'dict'
      && left.value.length === right.value.length
      && left.value.every(([key, value], index) => {
        const other = right.value[index]
        return !!other && dictKeysEqual(key, other[0]) && fieldValuesEqual(value, other[1])
      })
  }
}

function dictKeysEqual(left: CfdDictKey, right: CfdDictKey): boolean {
  if (left.kind !== right.kind) return false
  if (left.kind === 'enum' && right.kind === 'enum') {
    return left.value.enum_name === right.value.enum_name
      && left.value.variant === right.value.variant
      && left.value.value === right.value.value
  }
  if (left.kind === 'string' && right.kind === 'string') return left.value === right.value
  return left.kind === 'int' && right.kind === 'int' && left.value === right.value
}
