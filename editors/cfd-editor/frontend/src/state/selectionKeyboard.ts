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
  return key.length === 1 ? { kind: 'replace', text: key } : null
}
