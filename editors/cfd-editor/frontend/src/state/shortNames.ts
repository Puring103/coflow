import type { FieldCell } from '../bindings/FieldCell'
import type { FieldPathSegment } from '../wire'

export function recordShortName(fields: FieldCell[], field: string | undefined): string | undefined {
  const cell = fields.find(cell => cell.name === field)
  if (!cell || cell.missing) return undefined
  const value = cell.value
  const text = value.kind === 'string' ? value.value : value.kind === 'formatted_string' ? value.value.rendered : undefined
  return text || undefined
}

export function shortNameLabel(key: string, shortName?: string | null): string {
  return shortName ? `${shortName}(${key})` : key
}

export function shortNameCandidate(fields: FieldCell[], path: FieldPathSegment[]): string | undefined {
  if (path.length !== 1 || path[0].kind !== 'field') return undefined
  const cell = fields.find(field => field.name === path[0].value)
  return cell?.annotation?.declared_type === 'string' ? cell.name : undefined
}
