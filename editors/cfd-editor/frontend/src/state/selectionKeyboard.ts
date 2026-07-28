import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import type { FieldValue } from '../wire'

export type SelectionEditIntent =
  | { kind: 'clear' }
  | { kind: 'toggle-bool' }
  | { kind: 'edit' }
  | { kind: 'replace'; text: string }

export function selectionEditIntentForKey(
  key: string,
  modified: boolean,
  valueKind: FieldValue['kind'],
): SelectionEditIntent | null {
  if (modified) return null
  if (key === 'Delete') return { kind: 'clear' }
  if (key === 'Enter' && valueKind === 'bool') return { kind: 'toggle-bool' }
  if (key === 'Enter' || key === 'F2') return { kind: 'edit' }
  if (valueKind === 'array' || valueKind === 'dict' || valueKind === 'object') return null
  return key.length === 1 ? { kind: 'replace', text: key } : null
}

/** Returns the value to write when the user presses Delete on a cell. */
export function defaultValueForClear(annotation: FieldAnnotation | null, valueKind: FieldValue['kind']): FieldValue {
  if (annotation?.nullable) return { kind: 'null' }
  if (valueKind === 'array') return { kind: 'array', value: [] }
  if (valueKind === 'dict') return { kind: 'dict', value: [] }
  if (valueKind === 'bool') return { kind: 'bool', value: false }
  if (valueKind === 'int') return { kind: 'int', value: 0n }
  if (valueKind === 'float') return { kind: 'float', value: 0 }
  if (valueKind === 'string') return { kind: 'string', value: '' }
  // enum / ref / object / null — fall back to null
  return { kind: 'null' }
}
