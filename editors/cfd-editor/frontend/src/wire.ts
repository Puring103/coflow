import type { CfdDictKey } from './bindings/CfdDictKey'
import type { CfdPathSegment } from './bindings/CfdPathSegment'
import type { CfdRecord } from './bindings/CfdRecord'
import type { CfdValue } from './bindings/CfdValue'
import type { DeletedRecordSnapshot } from './bindings/DeletedRecordSnapshot'
import type { EditorError } from './bindings/EditorError'
import type { FieldCell } from './bindings/FieldCell'
import type { FlatDiagnostic } from './bindings/FlatDiagnostic'
import type { GraphEdge } from './bindings/GraphEdge'
import type { GraphNode } from './bindings/GraphNode'
import type { RecordCoordinate } from './bindings/RecordCoordinate'
import type { RecordRow } from './bindings/RecordRow'
import type { RefTarget } from './bindings/RefTarget'
import type { SpreadInfo } from './bindings/SpreadInfo'

export type FieldValue = CfdValue
export type DictKey = CfdDictKey
export type FieldPathSegment = CfdPathSegment
export type DiagnosticItem = FlatDiagnostic

export type Route =
  | { view: 'table'; file: string; typeFilter?: string }
  | { view: 'record'; file: string; coordinate: RecordCoordinate }
  | { view: 'graph'; file: string }

export type GraphNodeView = GraphNode & {
  id: string
  key: string
  actual_type: string
}

export type GraphEdgeView = {
  source: string
  target: string
  field_path: string
  raw: GraphEdge
}

export function coordinateId(coordinate: Pick<RecordCoordinate, 'actual_type' | 'key'>): string {
  return `${encodeURIComponent(coordinate.actual_type)}::${encodeURIComponent(coordinate.key)}`
}

export function sameCoordinate(
  a: Pick<RecordCoordinate, 'actual_type' | 'key'>,
  b: Pick<RecordCoordinate, 'actual_type' | 'key'>,
): boolean {
  return a.actual_type === b.actual_type && a.key === b.key
}

export function recordKey(row: RecordRow): string {
  return row.coordinate.key
}

export function recordActualType(row: RecordRow): string {
  return row.coordinate.actual_type
}

export function recordFields(record: CfdRecord): FieldCell[] {
  return Object.entries(record.fields)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([name, value]) => ({
      name,
      value: value ?? nullValue(),
      annotation: null,
    }))
}

export function objectFields(value: FieldValue): FieldCell[] {
  return value.kind === 'object' ? recordFields(value.value) : []
}

export function cellSpreadInfo(cell: FieldCell): SpreadInfo | undefined {
  return cell.annotation?.spread_info ?? undefined
}

export function isSpreadCell(cell: FieldCell): boolean {
  return !!cellSpreadInfo(cell)
}

export function fieldPathField(name: string): FieldPathSegment {
  return { kind: 'field', value: name }
}

export function fieldPathIndex(index: number): FieldPathSegment {
  return { kind: 'index', value: index }
}

export function fieldPathDictKey(value: string): FieldPathSegment {
  return { kind: 'dict_key', value }
}

export function makeObjectValue(actualType: string, fields: FieldCell[] = [], key = ''): FieldValue {
  const fieldMap: { [key: string]: FieldValue | undefined } = {}
  for (const field of fields) fieldMap[field.name] = field.value
  return {
    kind: 'object',
    value: {
      key,
      actual_type: actualType,
      fields: fieldMap,
    },
  }
}

export function deletedSnapshotValue(snapshot: DeletedRecordSnapshot): FieldValue {
  return { kind: 'object', value: snapshot.record }
}

export function nullValue(): FieldValue {
  return { kind: 'null' }
}

export function stringValue(value: string): FieldValue {
  return { kind: 'string', value }
}

export function intValue(value: bigint | number | string): FieldValue {
  return { kind: 'int', value: BigInt(value) }
}

export function floatValue(value: number): FieldValue {
  return { kind: 'float', value }
}

export function boolValue(value: boolean): FieldValue {
  return { kind: 'bool', value }
}

export function enumValue(enumName: string, variant: string | null, value: bigint | number | string): FieldValue {
  return { kind: 'enum', value: { enum_name: enumName, variant, value: BigInt(value) } }
}

export function refValue(targetType: string, targetKey: string): FieldValue {
  return { kind: 'ref', value: { target_type: targetType, target_key: targetKey } }
}

export function refValueFromTarget(target: RefTarget): FieldValue {
  return refValue(target.coordinate.actual_type, target.coordinate.key)
}

export function graphNodeView(node: GraphNode): GraphNodeView {
  return {
    ...node,
    id: coordinateId(node.coordinate),
    key: node.coordinate.key,
    actual_type: node.coordinate.actual_type,
  }
}

export function graphEdgeView(edge: GraphEdge): GraphEdgeView {
  return {
    source: coordinateId(edge.source),
    target: coordinateId(edge.target),
    field_path: edge.field_path,
    raw: edge,
  }
}

export function diagnosticMatchesCoordinate(
  diagnostic: DiagnosticItem,
  coordinate: RecordCoordinate,
): boolean {
  if (diagnostic.record_key !== coordinate.key) return false
  return diagnostic.actual_type === null || diagnostic.actual_type === coordinate.actual_type
}

export function diagnosticSeverity(severity: string): 'error' | 'warning' | 'info' {
  return severity === 'error' || severity === 'warning' ? severity : 'info'
}

export function errorMessage(err: unknown): string {
  if (isEditorError(err)) return err.message
  if (err instanceof Error) return err.message
  if (typeof err === 'string') return err
  try {
    return JSON.stringify(err)
  } catch {
    return String(err)
  }
}

export function errorDiagnostics(err: unknown): DiagnosticItem[] {
  return isEditorError(err) ? err.diagnostics : []
}

export function toIpc(value: unknown): unknown {
  if (typeof value === 'bigint') {
    return value.toString()
  }
  if (Array.isArray(value)) return value.map(toIpc)
  if (value && typeof value === 'object') {
    const out: Record<string, unknown> = {}
    for (const [key, item] of Object.entries(value)) out[key] = toIpc(item)
    return out
  }
  return value
}

export function fromIpc<T>(value: T): T {
  return normalizeWireValue(value) as T
}

function normalizeWireValue(value: unknown): unknown {
  if (!value || typeof value !== 'object') return value
  if (Array.isArray(value)) return value.map(normalizeWireValue)

  const object = value as Record<string, unknown>
  if (typeof object.kind === 'string') {
    return normalizeTaggedWireObject(object)
  }

  const out: Record<string, unknown> = {}
  for (const [key, item] of Object.entries(object)) {
    out[key] = key === 'enum_int_value' && item !== null
      ? toBigInt(item)
      : normalizeWireValue(item)
  }
  return out
}

function normalizeTaggedWireObject(object: Record<string, unknown>): unknown {
  const kind = object.kind
  switch (kind) {
    case 'int':
      return { ...object, value: toBigInt(object.value) }
    case 'enum':
      return { ...object, value: normalizeEnumWireValue(object.value) }
    case 'object':
      return { ...object, value: normalizeWireValue(object.value) }
    case 'array':
      return {
        ...object,
        value: Array.isArray(object.value)
          ? object.value.map(normalizeWireValue)
          : object.value,
      }
    case 'dict':
      return {
        ...object,
        value: Array.isArray(object.value)
          ? object.value.map((entry) => Array.isArray(entry)
            ? [normalizeWireValue(entry[0]), normalizeWireValue(entry[1])]
            : normalizeWireValue(entry))
          : object.value,
      }
    default:
      return normalizePlainObject(object)
  }
}

function normalizePlainObject(object: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const [key, item] of Object.entries(object)) out[key] = normalizeWireValue(item)
  return out
}

function normalizeEnumWireValue(value: unknown): unknown {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return value
  const enumObject = value as Record<string, unknown>
  return {
    ...enumObject,
    value: toBigInt(enumObject.value),
  }
}

function toBigInt(value: unknown): bigint {
  if (typeof value === 'bigint') return value
  if (typeof value === 'number') {
    if (!Number.isInteger(value)) throw new Error(`expected integer, got ${value}`)
    return BigInt(value)
  }
  if (typeof value === 'string') return BigInt(value)
  throw new Error(`expected integer, got ${String(value)}`)
}

function isEditorError(err: unknown): err is EditorError {
  return (
    !!err &&
    typeof err === 'object' &&
    'message' in err &&
    'diagnostics' in err &&
    Array.isArray((err as { diagnostics?: unknown }).diagnostics)
  )
}

export function cloneValue(value: FieldValue): FieldValue {
  switch (value.kind) {
    case 'null':
      return { kind: 'null' }
    case 'bool':
      return { kind: 'bool', value: value.value }
    case 'int':
      return { kind: 'int', value: BigInt(value.value) }
    case 'float':
      return { kind: 'float', value: value.value }
    case 'string':
      return { kind: 'string', value: value.value }
    case 'enum':
      return {
        kind: 'enum',
        value: {
          enum_name: value.value.enum_name,
          variant: value.value.variant,
          value: BigInt(value.value.value),
        },
      }
    case 'object':
      return {
        kind: 'object',
        value: {
          key: value.value.key,
          actual_type: value.value.actual_type,
          fields: cloneFieldMap(value.value.fields),
        },
      }
    case 'ref':
      return {
        kind: 'ref',
        value: {
          target_type: value.value.target_type,
          target_key: value.value.target_key,
        },
      }
    case 'array':
      return { kind: 'array', value: value.value.map(cloneValue) }
    case 'dict':
      return {
        kind: 'dict',
        value: value.value.map(([key, item]) => [cloneDictKey(key), cloneValue(item)]),
      }
  }
}

function cloneFieldMap(fields: { [key: string]: FieldValue | undefined }): { [key: string]: FieldValue | undefined } {
  const out: { [key: string]: FieldValue | undefined } = {}
  for (const [key, value] of Object.entries(fields)) {
    out[key] = value ? cloneValue(value) : value
  }
  return out
}

function cloneDictKey(key: DictKey): DictKey {
  switch (key.kind) {
    case 'string':
      return { kind: 'string', value: key.value }
    case 'int':
      return { kind: 'int', value: BigInt(key.value) }
    case 'enum':
      return {
        kind: 'enum',
        value: {
          enum_name: key.value.enum_name,
          variant: key.value.variant,
          value: BigInt(key.value.value),
        },
      }
  }
}
