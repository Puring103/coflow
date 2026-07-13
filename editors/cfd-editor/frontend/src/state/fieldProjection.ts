import type { FileRecords } from '../bindings/FileRecords'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import { summaryOf } from '../value/fieldValue'
import {
  cloneValue,
  sameCoordinate,
  type DictKey,
  type FieldPathSegment,
  type FieldValue,
} from '../wire'

export interface FieldProjection {
  changed: boolean
  records: FileRecords
  row?: RecordRow
  oldValue?: FieldValue
}

export function projectFieldValueAtRevision(
  records: FileRecords,
  revision: number,
  coordinate: RecordCoordinate,
  path: FieldPathSegment[],
  nextValue: FieldValue,
): FieldProjection | undefined {
  if (records.revision !== revision) return undefined
  return projectFieldValue(records, coordinate, path, nextValue)
}

export function projectFieldValue(
  records: FileRecords,
  coordinate: RecordCoordinate,
  path: FieldPathSegment[],
  nextValue: FieldValue,
): FieldProjection {
  if (path.length === 0 || path[0].kind !== 'field') return { changed: true, records }
  const rowIndex = records.records.findIndex(row => sameCoordinate(row.coordinate, coordinate))
  if (rowIndex < 0) return { changed: true, records }
  const row = records.records[rowIndex]
  const fieldName = path[0].value
  const fieldIndex = row.field_index[fieldName]
    ?? row.fields.findIndex(field => field.name === fieldName)
  if (fieldIndex < 0 || fieldIndex >= row.fields.length) return { changed: true, records }
  const current = row.fields[fieldIndex].value
  const oldValue = valueAtPath(current, path, 1)
  const projected = replaceAtPath(current, path, 1, nextValue)
  if (!projected || !oldValue) return { changed: true, records }
  if (sameFieldValue(current, projected)) {
    return { changed: false, records, row, oldValue: cloneValue(oldValue) }
  }
  const fields = row.fields.slice()
  fields[fieldIndex] = { ...fields[fieldIndex], value: projected }
  const nextRow: RecordRow = {
    ...row,
    fields,
    field_summaries: {
      ...row.field_summaries,
      [fieldName]: summaryOf(projected),
    },
  }
  const nextRows = records.records.slice()
  nextRows[rowIndex] = nextRow
  return {
    changed: true,
    records: { ...records, records: nextRows },
    row: nextRow,
    oldValue: cloneValue(oldValue),
  }
}

export function sameFieldValue(left: FieldValue, right: FieldValue): boolean {
  if (left.kind !== right.kind) return false
  switch (left.kind) {
    case 'null':
      return true
    case 'bool':
    case 'float':
    case 'string':
    case 'ref':
      return left.value === (right as typeof left).value
    case 'int':
      return BigInt(left.value) === BigInt((right as typeof left).value)
    case 'enum': {
      const value = (right as typeof left).value
      return left.value.enum_name === value.enum_name
        && left.value.variant === value.variant
        && BigInt(left.value.value) === BigInt(value.value)
    }
    case 'object': {
      const value = (right as typeof left).value
      const leftKeys = Object.keys(left.value.fields).sort()
      const rightKeys = Object.keys(value.fields).sort()
      return left.value.actual_type === value.actual_type
        && sameStrings(leftKeys, rightKeys)
        && leftKeys.every(key => {
          const leftField = left.value.fields[key]
          const rightField = value.fields[key]
          return leftField === undefined
            ? rightField === undefined
            : rightField !== undefined && sameFieldValue(leftField, rightField)
        })
    }
    case 'array': {
      const value = (right as typeof left).value
      return left.value.length === value.length
        && left.value.every((item, index) => sameFieldValue(item, value[index]))
    }
    case 'dict': {
      const value = (right as typeof left).value
      return left.value.length === value.length
        && left.value.every(([key, item], index) => (
          sameDictKey(key, value[index][0]) && sameFieldValue(item, value[index][1])
        ))
    }
  }
}

function replaceAtPath(
  current: FieldValue,
  path: FieldPathSegment[],
  index: number,
  nextValue: FieldValue,
): FieldValue | null {
  if (index === path.length) return cloneValue(nextValue)
  const segment = path[index]
  if (segment.kind === 'field' && current.kind === 'object') {
    const child = current.value.fields[segment.value]
    if (!child) return null
    const next = replaceAtPath(child, path, index + 1, nextValue)
    if (!next) return null
    return {
      kind: 'object',
      value: {
        actual_type: current.value.actual_type,
        fields: { ...current.value.fields, [segment.value]: next },
      },
    }
  }
  if (segment.kind === 'index' && current.kind === 'array') {
    const child = current.value[segment.value]
    if (!child) return null
    const next = replaceAtPath(child, path, index + 1, nextValue)
    if (!next) return null
    const values = current.value.slice()
    values[segment.value] = next
    return { kind: 'array', value: values }
  }
  if (segment.kind === 'dict_key' && current.kind === 'dict') {
    const entryIndex = current.value.findIndex(([key]) => dictKeyText(key) === segment.value)
    if (entryIndex < 0) return null
    const next = replaceAtPath(current.value[entryIndex][1], path, index + 1, nextValue)
    if (!next) return null
    const entries = current.value.slice()
    entries[entryIndex] = [entries[entryIndex][0], next]
    return { kind: 'dict', value: entries }
  }
  return null
}

function valueAtPath(
  current: FieldValue,
  path: FieldPathSegment[],
  index: number,
): FieldValue | null {
  if (index === path.length) return current
  const segment = path[index]
  if (segment.kind === 'field' && current.kind === 'object') {
    const child = current.value.fields[segment.value]
    return child ? valueAtPath(child, path, index + 1) : null
  }
  if (segment.kind === 'index' && current.kind === 'array') {
    const child = current.value[segment.value]
    return child ? valueAtPath(child, path, index + 1) : null
  }
  if (segment.kind === 'dict_key' && current.kind === 'dict') {
    const entry = current.value.find(([key]) => dictKeyText(key) === segment.value)
    return entry ? valueAtPath(entry[1], path, index + 1) : null
  }
  return null
}

function sameDictKey(left: DictKey, right: DictKey): boolean {
  if (left.kind !== right.kind) return false
  if (left.kind === 'string') return left.value === (right as typeof left).value
  if (left.kind === 'int') return BigInt(left.value) === BigInt((right as typeof left).value)
  const value = (right as typeof left).value
  return left.value.enum_name === value.enum_name
    && left.value.variant === value.variant
    && BigInt(left.value.value) === BigInt(value.value)
}

function dictKeyText(key: DictKey): string {
  if (key.kind === 'int') return key.value.toString()
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

function sameStrings(left: string[], right: string[]): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index])
}
