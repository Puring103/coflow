import type { CfdDictKey } from '../bindings/CfdDictKey'
import type { CfdObject } from '../bindings/CfdObject'
import type { CfdPathSegment } from '../bindings/CfdPathSegment'
import type { CfdValue } from '../bindings/CfdValue'
import type { DeletedRecordSnapshot } from '../bindings/DeletedRecordSnapshot'
import type { FieldCell } from '../bindings/FieldCell'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import type { RecordRow } from '../bindings/RecordRow'

export type FieldValue = CfdValue
export type DictKey = CfdDictKey
export type FieldPathSegment = CfdPathSegment
export function isComplexValue(
  value: FieldValue | undefined,
): value is FieldValue & { kind: 'object' | 'array' | 'dict' } {
  return value?.kind === 'object' || value?.kind === 'array' || value?.kind === 'dict'
}

/** 字典路径必须使用唯一的 CFD 文本身份，插件与编辑历史才能定位同一项。 */
export function dictKeyPathText(key: DictKey): string {
  if (key.kind === 'int' || key.kind === 'bool') return key.value.toString()
  if (key.kind === 'enum') {
    return key.value.variant
      ? `${key.value.enum_name}.${key.value.variant}`
      : `${key.value.enum_name}(${key.value.value})`
  }
  return `"${key.value
    .replace(/\\/g, '\\\\')
    .replace(/"/g, '\\"')
    .replace(/\n/g, '\\n')
    .replace(/\r/g, '\\r')
    .replace(/\t/g, '\\t')}"`
}

export function recordKey(row: RecordRow): string {
  return row.coordinate.key
}

export function recordActualType(row: RecordRow): string {
  return row.coordinate.actual_type
}

export function recordFields(object: CfdObject): FieldCell[] {
  return Object.entries(object.fields)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([name, value]) => ({
      name,
      value,
      missing: false,
      annotation: null,
    }))
}

export function objectFields(value: FieldValue): FieldCell[] {
  return value.kind === 'object' ? recordFields(value.value) : []
}

/** 按 schema 顺序投影对象字段，并为尚未写入 CFD 的必填字段保留可编辑行。 */
export function objectFieldCells(
  value: FieldValue,
  annotation: FieldAnnotation | null | undefined,
): FieldCell[] {
  if (value.kind !== 'object') return []

  const fields = value.value.fields
  const orderedNames = annotation?.field_order ?? []
  const orderedSet = new Set(orderedNames)
  const schemaCells = orderedNames.map(name => {
    const child = annotation?.children?.[name] ?? null
    const fieldValue = fields[name]
    return {
      name,
      value: fieldValue ?? nullValue(),
      missing: fieldValue === undefined,
      annotation: child,
    } satisfies FieldCell
  })
  const extraCells = Object.entries(fields)
    .filter(([name]) => !orderedSet.has(name))
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([name, fieldValue]) => ({
      name,
      value: fieldValue,
      missing: false,
      annotation: annotation?.children?.[name] ?? null,
    } satisfies FieldCell))

  return [...schemaCells, ...extraCells]
}

export function makeObjectValue(actualType: string, fields: FieldCell[] = []): FieldValue {
  const fieldMap: Record<string, FieldValue> = {}
  for (const field of fields) {
    if (field.value !== undefined) fieldMap[field.name] = field.value
  }
  return {
    kind: 'object',
    value: {
      actual_type: actualType,
      fields: fieldMap,
    },
  }
}

export function deletedSnapshotValue(snapshot: DeletedRecordSnapshot): FieldValue {
  return { kind: 'object', value: snapshot.record.object }
}

export function nullValue(): FieldValue {
  return { kind: 'option_none' }
}

/** 必填缺失字段直接写值；可选字段使用内部 Some 表示非空状态。 */
export function applyCreatedValue(
  optional: boolean,
  created: FieldValue,
): FieldValue {
  return optional ? { kind: 'option_some', value: created } : created
}

export function isNullValue(value: FieldValue): boolean {
  return value.kind === 'option_none'
}

export function presentationValue(value: FieldValue): FieldValue {
  switch (value.kind) {
    case 'option_some':
      return presentationValue(value.value)
    default:
      return value
  }
}

export function replacePresentationValue(original: FieldValue, value: FieldValue): FieldValue {
  if (value.kind === 'option_none') return value
  switch (original.kind) {
    case 'option_some': return {
      kind: 'option_some',
      value: replacePresentationValue(original.value, value),
    }
    case 'option_none': return { kind: 'option_some', value }
    default: return value
  }
}

export function optionalState(value: FieldValue, optional: boolean): 'some' | 'none' | null {
  if (!optional) return null
  return value.kind === 'option_some' ? 'some' : 'none'
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

export function refValue(targetKey: string): FieldValue {
  return { kind: 'ref', value: targetKey }
}

export function cloneValue(value: FieldValue): FieldValue {
  switch (value.kind) {
    case 'option_none':
      return { kind: 'option_none' }
    case 'option_some':
      return { kind: 'option_some', value: cloneValue(value.value) }
    case 'bool':
      return { kind: 'bool', value: value.value }
    case 'int':
      return { kind: 'int', value: BigInt(value.value) }
    case 'float':
      return { kind: 'float', value: value.value }
    case 'string':
      return { kind: 'string', value: value.value }
    case 'formatted_string':
      return { kind: 'formatted_string', value: { ...value.value } }
    case 'function':
      return { kind: 'function', value: { ...value.value } }
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
          actual_type: value.value.actual_type,
          fields: cloneFieldMap(value.value.fields),
        },
      }
    case 'ref':
      return {
        kind: 'ref',
        value: value.value,
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

function cloneFieldMap(fields: Record<string, FieldValue>): Record<string, FieldValue> {
  const out: Record<string, FieldValue> = {}
  for (const [key, value] of Object.entries(fields)) {
    out[key] = cloneValue(value)
  }
  return out
}

function cloneDictKey(key: DictKey): DictKey {
  switch (key.kind) {
    case 'bool': return { kind: 'bool', value: key.value }
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
