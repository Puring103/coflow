import type { DictKey, FieldValue } from '../wire'
import type { RecordRow } from '../bindings/RecordRow'

export function parseFieldValueText(original: FieldValue, raw: string): FieldValue | null {
  if (original.kind === 'option_some') {
    const parsed = parseFieldValueText(original.value, raw)
    if (!parsed) return null
    return { kind: 'option_some', value: parsed }
  }
  switch (original.kind) {
    case 'bool':
      if (raw !== 'true' && raw !== 'false') return null
      return { kind: 'bool', value: raw === 'true' }
    case 'int':
      try {
        const value = BigInt(raw)
        return value >= -2147483648n && value <= 2147483647n ? { kind: 'int', value } : null
      } catch {
        return null
      }
    case 'float': {
      if (raw.trim() === '') return null
      if (raw === 'inf' || raw === '-inf') return { kind: 'float', value: raw }
      if (!/^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$/.test(raw)) return null
      const value = Math.fround(Number(raw))
      return { kind: 'float', value: Number.isFinite(value) ? value : value < 0 ? '-inf' : 'inf' }
    }
    case 'string':
      return { kind: 'string', value: raw }
    case 'formatted_string':
      return raw.startsWith('f"') ? { kind: 'formatted_string', value: { source: raw } } : null
    case 'enum':
      return { kind: 'enum', value: { ...original.value, variant: raw } }
    case 'ref':
      return { kind: 'ref', value: raw }
    default:
      return null
  }
}

export function plainFieldValueText(value: FieldValue): string {
  if (value.kind === 'formatted_string') return value.value.source
  return scalarText(value) ?? ''
}

function scalarText(value: FieldValue): string | null {
  if (value.kind === 'option_some') {
    return scalarText(value.value)
  }
  switch (value.kind) {
    case 'bool': return value.value ? 'true' : 'false'
    case 'int': return String(value.value)
    case 'float': return String(value.value)
    case 'string': return value.value
    case 'formatted_string': return value.value.source
    case 'function': return value.value.source
    case 'enum': return enumVariantText(value)
    case 'ref': return referenceKeyText(value.value)
    default: return null
  }
}

export function referenceKeyText(reference: string): string {
  const withoutPrefix = reference.startsWith('&') ? reference.slice(1) : reference
  const separator = withoutPrefix.lastIndexOf('::')
  return separator >= 0 ? withoutPrefix.slice(separator + 2) : withoutPrefix
}

export function scalarDefaultForDeclaredType(declaredType?: string): FieldValue | null {
  if (!declaredType) return null
  const stripped = stripNullableType(declaredType)
  switch (stripped) {
    case 'string': return { kind: 'string', value: '' }
    case 'fstring': return { kind: 'formatted_string', value: { source: 'f""' } }
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
    case 'option_none': return '-'
    case 'option_some': return summaryOf(value.value)
    case 'object': return value.value.actual_type
    case 'array': {
      if (value.value.length === 0) return '[]'
      const allScalar = value.value.every(item => (
        item.kind === 'bool' || item.kind === 'int' || item.kind === 'float'
        || item.kind === 'string' || item.kind === 'enum'
        || item.kind === 'formatted_string'
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
  return !normalized || record.coordinate.key.toLowerCase().includes(normalized)
}

/** Matches a query against a record's complete, recursively traversed value tree. */
export function recordMatchesFullTextSearch(record: RecordRow, query: string): boolean {
  const normalized = query.trim().toLowerCase()
  if (!normalized) return true
  if (record.coordinate.key.toLowerCase().includes(normalized)) return true
  return record.fields.some(field => (
    field.name.toLowerCase().includes(normalized)
    || fullTextOf(field.value).toLowerCase().includes(normalized)
  ))
}

function fullTextOf(value: FieldValue): string {
  const scalar = scalarText(value)
  if (scalar !== null) return scalar
  switch (value.kind) {
    case 'option_none': return 'None'
    case 'option_some': return fullTextOf(value.value)
    case 'object': return [
      value.value.actual_type,
      ...Object.entries(value.value.fields).flatMap(([name, child]) => child ? [name, fullTextOf(child)] : [name]),
    ].join(' ')
    case 'array': return value.value.map(fullTextOf).join(' ')
    case 'dict': return value.value.map(([key, child]) => `${dictKeyShortLabel(key)} ${fullTextOf(child)}`).join(' ')
    default: return ''
  }
}

/** 字典键短标签（仅搜索显示用）；CFD 文本身份用 `wire.dictKeyPathText`，两者语义不同。 */
export function dictKeyShortLabel(key: DictKey): string {
  switch (key.kind) {
    case 'string': return key.value
    case 'int': return String(key.value)
    case 'bool': return String(key.value)
    case 'enum': return `${key.value.enum_name} ${key.value.variant ?? String(key.value.value)}`
  }
}

export function optionDepthForDeclaredType(declaredType?: string): number {
  return declaredType?.endsWith('?') ? 1 : 0
}

function stripNullableType(declaredType: string): string {
  return declaredType.endsWith('?') ? declaredType.slice(0, -1) : declaredType
}

export function enumVariantText(value: FieldValue & { kind: 'enum' }): string {
  return value.value.variant ?? String(value.value.value)
}

function dictKindLabel(key: DictKey): string {
  switch (key.kind) {
    case 'string': return 'string'
    case 'int': return 'int'
    case 'bool': return 'bool'
    case 'enum': return key.value.enum_name
  }
}

function valueKindLabel(value: FieldValue): string {
  switch (value.kind) {
    case 'option_none': return 'None'
    case 'option_some': return valueKindLabel(value.value)
    case 'bool': return 'bool'
    case 'int': return 'int'
    case 'float': return 'float'
    case 'string': return 'string'
    case 'formatted_string': return 'string'
    case 'function': return 'fn'
    case 'enum': return value.value.enum_name
    case 'object': return value.value.actual_type
    case 'ref': return '&'
    case 'array': return value.value[0] ? `${valueKindLabel(value.value[0])}[]` : '[]'
    case 'dict': return value.value[0]
      ? `{${dictKindLabel(value.value[0][0])}:${valueKindLabel(value.value[0][1])}}`
      : '{}'
  }
}
