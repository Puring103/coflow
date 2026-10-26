import type { FieldCell } from '../../../bindings/FieldCell'
import type { FieldPathSegment } from '../../../wire'
import type { RefTarget } from '../../../bindings/RefTarget'
import type { SearchableOption } from '../../../shared/ui/SearchableSelect'

interface ReferenceProjection {
  byId: Map<string, RefTarget>
  options: SearchableOption[]
  nullableOptions: SearchableOption[]
}
const referenceProjections = new WeakMap<RefTarget[], ReferenceProjection>()
/** 同一 lookup 结果只建立一次索引与候选展示；刷新后由新数组身份失效。 */
export function referenceProjection(targets: RefTarget[]): ReferenceProjection {
  let projection = referenceProjections.get(targets)
  if (!projection) {
    const options = targets.map(target => ({ value: target.coordinate.id,
      label: shortNameLabel(target.coordinate.id, target.short_name),
      selectedLabel: target.short_name ?? target.coordinate.id }))
    projection = { byId: new Map(targets.map(target => [target.coordinate.id, target])),
      options, nullableOptions: [{ value: 'None' }, ...options] }
    referenceProjections.set(targets, projection)
  }
  return projection
}

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
