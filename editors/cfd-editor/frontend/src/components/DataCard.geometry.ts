import type { FieldCell } from '../bindings/FieldCell'
import { dictKeyPathText, objectFieldCells, presentationValue, type FieldValue } from '../wire'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'

export const NODE_PEEK_FIELDS = 5

export function countVisibleRows(
  fields: FieldCell[],
  expandedPaths: ReadonlySet<string>,
  prefix = '',
): number {
  function rows(original: FieldValue, annotation: FieldAnnotation | null | undefined, path: string): number {
    if (!expandedPaths.has(path)) return 1
    const value = presentationValue(original)
    if (value.kind === 'object') {
      return 1 + countVisibleRows(objectFieldCells(value, annotation), expandedPaths, path)
    }
    if (value.kind === 'array') return 1 + value.value.reduce((sum, item, index) => sum
      + rows(item, annotation?.children[String(index)] ?? annotation?.item_annotation, `${path}[${index}]`), 0)
    if (value.kind === 'dict') return 1 + value.value.reduce((sum, [key, item]) => sum
      + rows(item, annotation?.children[dictKeyPathText(key)] ?? annotation?.item_annotation, `${path}[${dictKeyPathText(key)}]`), 0)
    return 1
  }
  return fields.reduce((sum, field) => sum + rows(field.value, field.annotation,
    prefix ? `${prefix}.${field.name}` : field.name), 0)
}
