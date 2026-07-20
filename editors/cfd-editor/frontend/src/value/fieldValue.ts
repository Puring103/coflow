import type { DictKey, FieldValue } from '../wire'
import type { RecordRow } from '../bindings/RecordRow'

export function parseFieldValueText(original: FieldValue, raw: string): FieldValue | null {
  switch (original.kind) {
    case 'bool':
      if (raw !== 'true' && raw !== 'false') return null
      return { kind: 'bool', value: raw === 'true' }
    case 'int':
      try {
        return { kind: 'int', value: BigInt(raw) }
      } catch {
        return null
      }
    case 'float': {
      if (raw.trim() === '') return null
      const value = Number(raw)
      return Number.isFinite(value) ? { kind: 'float', value } : null
    }
    case 'string':
      return { kind: 'string', value: raw }
    case 'enum':
      return { kind: 'enum', value: { ...original.value, variant: raw } }
    case 'ref':
      return { kind: 'ref', value: raw }
    default:
      return null
  }
}

export function plainFieldValueText(value: FieldValue): string {
  return scalarText(value) ?? ''
}

function scalarText(value: FieldValue): string | null {
  switch (value.kind) {
    case 'bool': return value.value ? 'true' : 'false'
    case 'int': return String(value.value)
    case 'float': return String(value.value)
    case 'string': return value.value
    case 'enum': return enumVariantText(value)
    case 'ref': return referenceKeyText(value.value)
    default: return null
  }
}

export function referenceKeyText(reference: string): string {
  const withoutPrefix = reference.startsWith('&') ? reference.slice(1) : reference
  const separator = withoutPrefix.lastIndexOf('.')
  return separator >= 0 ? withoutPrefix.slice(separator + 1) : withoutPrefix
}

export function scalarDefaultForDeclaredType(declaredType?: string): FieldValue | null {
  if (!declaredType) return null
  const stripped = stripNullableType(declaredType)
  switch (stripped) {
    case 'string': return { kind: 'string', value: '' }
    case 'int': return { kind: 'int', value: 0n }
    case 'float': return { kind: 'float', value: 0 }
    case 'bool': return { kind: 'bool', value: false }
    default: return collectionShapeForDeclaredType(stripped)
  }
}

export function collectionShapeForDeclaredType(declaredType?: string): FieldValue | null {
  if (!declaredType) return null
  const stripped = stripNullableType(declaredType)
  if (stripped.startsWith('[') && stripped.endsWith(']')) return { kind: 'array', value: [] }
  if (stripped.startsWith('{') && stripped.endsWith('}')) return { kind: 'dict', value: [] }
  return null
}

export function summaryOf(value: FieldValue): string {
  const scalar = scalarText(value)
  if (scalar !== null) return scalar
  switch (value.kind) {
    case 'null': return '-'
    case 'object': return value.value.actual_type
    case 'array': {
      if (value.value.length === 0) return '[]'
      const allScalar = value.value.every(item => (
        item.kind === 'bool' || item.kind === 'int' || item.kind === 'float'
        || item.kind === 'string' || item.kind === 'enum'
      ))
      if (allScalar && value.value.length <= 6) {
        const joined = value.value.map(summaryOf).join(', ')
        if (joined.length <= 60) return `[${joined}]`
      }
      return `${valueKindLabel(value.value[0])}[${value.value.length}]`
    }
    case 'dict': {
      if (value.value.length === 0) return '{}'
      const [key, item] = value.value[0]
      return `${dictKindLabel(key)}->${valueKindLabel(item)}  (${value.value.length})`
    }
    default: return ''
  }
}

export function recordMatchesSearch(record: RecordRow, query: string): boolean {
  const normalized = query.trim().toLowerCase()
  if (!normalized) return true
  if (record.coordinate.key.toLowerCase().includes(normalized)) return true
  return record.fields.some(field => (
    field.name.toLowerCase().includes(normalized)
    || summaryOf(field.value).toLowerCase().includes(normalized)
  ))
}

function stripNullableType(declaredType: string): string {
  return declaredType.endsWith('?') ? declaredType.slice(0, -1) : declaredType
}

function enumVariantText(value: FieldValue & { kind: 'enum' }): string {
  return value.value.variant ?? String(value.value.value)
}

function dictKindLabel(key: DictKey): string {
  switch (key.kind) {
    case 'string': return 'string'
    case 'int': return 'int'
    case 'enum': return key.value.enum_name
  }
}

function valueKindLabel(value: FieldValue): string {
  switch (value.kind) {
    case 'null': return 'null'
    case 'bool': return 'bool'
    case 'int': return 'int'
    case 'float': return 'float'
    case 'string': return 'string'
    case 'enum': return value.value.enum_name
    case 'object': return value.value.actual_type
    case 'ref': return '&'
    case 'array': return value.value[0] ? `${valueKindLabel(value.value[0])}[]` : '[]'
    case 'dict': return value.value[0]
      ? `{${dictKindLabel(value.value[0][0])}:${valueKindLabel(value.value[0][1])}}`
      : '{}'
  }
}
