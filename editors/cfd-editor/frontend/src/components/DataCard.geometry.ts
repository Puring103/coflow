import type { FieldCell } from '../bindings/FieldCell'
import { objectFields } from '../wire'

export const NODE_PEEK_FIELDS = 5

export function countVisibleRows(
  fields: FieldCell[],
  expandedPaths: ReadonlySet<string>,
  prefix = '',
): number {
  let count = 0
  for (const field of fields) {
    count++
    const path = prefix ? `${prefix}.${field.name}` : field.name
    if (!expandedPaths.has(path)) continue
    if (field.value.kind === 'object') {
      count += countVisibleRows(objectFields(field.value), expandedPaths, path)
    } else if (field.value.kind === 'array') {
      count += field.value.value.length
    } else if (field.value.kind === 'dict') {
      count += field.value.value.length
    }
  }
  return count
}
