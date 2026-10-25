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
      if (raw.trim() === '') return null
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
    case 'formatted_string':
      return automaticStringValue(raw)
    case 'enum':
      return { kind: 'enum', value: { ...original.value, variant: raw } }
    case 'ref':
      return { kind: 'ref', value: raw }
    default:
      return null
  }
}

export function plainFieldValueText(value: FieldValue): string {
  if (value.kind === 'formatted_string') return formattedSourceText(value.value.source)
  return scalarText(value) ?? ''
}

function formattedSourceText(source: string): string {
  const quoted = source.slice(1)
  try {
    const value: unknown = JSON.parse(quoted)
    return typeof value === 'string' ? value.replace(/\{\{|\}\}/g, brace => brace[0]) : source
  } catch {
    return source
  }
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
    case 'formatted_string': return value.value.rendered
    case 'enum': return enumVariantText(value)
    case 'ref': return referenceIdText(value.value)
    default: return null
  }
}

export function referenceIdText(reference: string): string {
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
  return !normalized || record.coordinate.id.toLowerCase().includes(normalized)
}

/** Matches a query against a record's complete, recursively traversed value tree. */
export function recordMatchesFullTextSearch(record: RecordRow, query: string): boolean {
  const normalized = query.trim().toLowerCase()
  if (!normalized) return true
  if (record.coordinate.id.toLowerCase().includes(normalized)) return true
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
    case 'enum': return `${key.value.enum_name} ${key.value.variant ?? String(key.value.value)}`
  }
}

export function optionDepthForDeclaredType(declaredType?: string): number {
  if (!declaredType) return 0
  let current = declaredType
  let depth = 0
  while (current.startsWith('Option<') && current.endsWith('>')) {
    depth += 1
    current = current.slice(7, -1)
  }
  return depth
}

function stripNullableType(declaredType: string): string {
  let current = declaredType
  while (current.startsWith('Option<') && current.endsWith('>')) {
    current = current.slice(7, -1)
  }
  return current.endsWith('?') ? current.slice(0, -1) : current
}

export function enumVariantText(value: FieldValue & { kind: 'enum' }): string {
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
    case 'option_none': return 'None'
    case 'option_some': return valueKindLabel(value.value)
    case 'bool': return 'bool'
    case 'int': return 'int'
    case 'float': return 'float'
    case 'string': return 'string'
    case 'formatted_string': return 'string'
    case 'enum': return value.value.enum_name
    case 'object': return value.value.actual_type
    case 'ref': return '&'
    case 'array': return value.value[0] ? `${valueKindLabel(value.value[0])}[]` : '[]'
    case 'dict': return value.value[0]
      ? `{${dictKindLabel(value.value[0][0])}:${valueKindLabel(value.value[0][1])}}`
      : '{}'
  }
}

/** 仅匹配记录键，不把名称、类型或字段内容纳入搜索。 */
export function recordMatchesKeySearch(record: RecordRow, query: string): boolean {
  return record.coordinate.id.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())
}

const INTERPOLATION_NAME = '[\\p{XID_Start}_][\\p{XID_Continue}_]*'
const INTERPOLATION_PATH = new RegExp(`^(?:self|&${INTERPOLATION_NAME}::${INTERPOLATION_NAME})(?:\\.${INTERPOLATION_NAME})+$`, 'u')

/** 编辑器按合法插值自动选择字符串类型；正文中的其他花括号作为文本保留。 */
export function automaticStringValue(text: string): FieldValue & { kind: 'string' | 'formatted_string' } {
  let found = false
  const source = text.replace(/\{[^{}]*\}|[{}]/g, token => {
    const expression = token.slice(1, -1).trim()
    if (token.length > 2 && INTERPOLATION_PATH.test(expression)
      && expression.split('.').length <= (expression.startsWith('self.') ? 128 : 129)) {
      found = true
      return token
    }
    return token.replace(/[{}]/g, brace => brace + brace)
  })
  return found
    ? { kind: 'formatted_string', value: { source: `f${JSON.stringify(source)}`, rendered: text } }
    : { kind: 'string', value: text }
}
