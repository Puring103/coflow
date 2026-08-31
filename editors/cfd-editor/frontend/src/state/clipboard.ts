import type { BatchWriteFieldInput } from '../bindings/BatchWriteFieldInput'
import type { CfdDictKey } from '../bindings/CfdDictKey'
import type { CfdValue } from '../bindings/CfdValue'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import { fieldPathField, fieldPathIndex, type FieldPathSegment } from '../wire'
import type { CellAnchor } from './editorSelection'
import { fieldValuesEqual } from './batchRecordProjection'

export async function serializeCfdCellMatrix(
  rows: readonly (readonly CellAnchor[])[],
  render: (coordinate: RecordCoordinate, path: FieldPathSegment[]) => Promise<string>,
): Promise<string> {
  const rendered: string[][] = []
  for (const row of rows) {
    const cells: string[] = []
    for (const cell of row) cells.push(await render(cell.coordinate, cell.fieldPath))
    rendered.push(cells)
  }
  return serializeCfdMatrix(rendered)
}

export function serializeRecordRefsAsCfd(records: readonly RecordCoordinate[]): string {
  return serializeCfdMatrix(records.map(record => [`&${record.key}`]))
}

export function serializeCfdMatrix(rows: readonly (readonly string[])[]): string {
  const renderedRows = rows.map(row => `  [${row.map(normalizeCfdCell).join(', ')}]`)
  return renderedRows.length === 0 ? '[]' : `[\n${renderedRows.join(',\n')}\n]`
}

export function parseCfdClipboard(text: string): string[][] {
  const value = text.trim()
  if (value.length === 0) throw new Error('CFD 剪贴板内容为空')
  const outer = unwrapCfdArray(value)
  if (outer === null) return [[value]]
  const rowValues = splitCfdValues(outer)
  if (rowValues.length === 0) return [[value]]
  const rowContents = rowValues.map(unwrapCfdArray)
  if (rowContents.some(row => row === null)) return [[value]]
  const rows = rowContents.map(row => splitCfdValues(row!))
  const width = rows[0]?.length ?? 0
  if (width === 0) return [[value]]
  if (rows.some(row => row.length !== width)) {
    throw new Error('CFD 剪贴板矩阵的每一行必须具有相同列数')
  }
  return rows
}

function normalizeCfdCell(text: string): string {
  const value = text.trim()
  return value.length === 0 ? 'CfdClipboardMissing {}' : value
}

function unwrapCfdArray(text: string): string | null {
  const value = text.trim()
  if (!value.startsWith('[')) return null
  const end = matchingCfdDelimiter(value, 0)
  return end === value.length - 1 ? value.slice(1, end) : null
}

function splitCfdValues(text: string): string[] {
  if (text.trim().length === 0) return []
  const values: string[] = []
  let start = 0
  let index = 0
  while (index < text.length) {
    const char = text[index]
    if (char === '"') {
      index = skipCfdString(text, index)
      continue
    }
    if (char === '#') {
      while (index < text.length && text[index] !== '\n') index++
      continue
    }
    if (char === '[' || char === '{' || char === '(') {
      index = matchingCfdDelimiter(text, index) + 1
      continue
    }
    if (char === ',') {
      const value = text.slice(start, index).trim()
      if (value.length === 0) throw new Error('CFD 剪贴板包含空值')
      values.push(value)
      start = index + 1
    }
    index++
  }
  const tail = text.slice(start).trim()
  if (tail.length > 0) values.push(tail)
  else if (values.length > 0) throw new Error('CFD 剪贴板不接受尾随逗号')
  return values
}

function matchingCfdDelimiter(text: string, start: number): number {
  const pairs: Record<string, string> = { '[': ']', '{': '}', '(': ')' }
  const stack: string[] = [pairs[text[start]]]
  if (!stack[0]) throw new Error('CFD 剪贴板值没有有效的起始分隔符')
  for (let index = start + 1; index < text.length; index++) {
    const char = text[index]
    if (char === '"') {
      index = skipCfdString(text, index) - 1
      continue
    }
    if (char === '#') {
      while (index < text.length && text[index] !== '\n') index++
      continue
    }
    if (pairs[char]) {
      stack.push(pairs[char])
      continue
    }
    if (char === stack[stack.length - 1]) {
      stack.pop()
      if (stack.length === 0) return index
      continue
    }
    if (char === ']' || char === '}' || char === ')') {
      throw new Error('CFD 剪贴板分隔符不匹配')
    }
  }
  throw new Error('CFD 剪贴板值未闭合')
}

function skipCfdString(text: string, start: number): number {
  for (let index = start + 1; index < text.length; index++) {
    if (text[index] === '\\') {
      index++
      continue
    }
    if (text[index] === '"') return index + 1
  }
  throw new Error('CFD 剪贴板字符串未闭合')
}

export interface PasteCell {
  coordinate: RecordCoordinate
  fieldPath: FieldPathSegment[]
  annotation: FieldAnnotation | null
  value: CfdValue
  writable: boolean
}

export interface PasteContext {
  parse: (coordinate: RecordCoordinate, path: FieldPathSegment[], text: string) => Promise<CfdValue>
  mode: 'replace' | 'append'
}

export function pasteCellAtRecordPath(
  record: RecordRow,
  fieldPath: FieldPathSegment[],
  writable: boolean,
): PasteCell | null {
  const top = fieldPath[0]
  if (top?.kind !== 'field') return null
  const fieldIndex = record.field_index[top.value]
  const field = typeof fieldIndex === 'number'
    ? record.fields[fieldIndex]
    : record.fields.find(candidate => candidate.name === top.value)
  if (!field) return null

  let value = field.value
  let annotation = field.annotation
  for (const segment of fieldPath.slice(1)) {
    if (segment.kind === 'field' && value.kind === 'object') {
      const child = value.value.fields[segment.value]
      if (!child) return null
      value = child
      annotation = annotation?.children[segment.value] ?? null
      continue
    }
    if (segment.kind === 'index' && value.kind === 'array') {
      const child = value.value[segment.value]
      if (!child) return null
      value = child
      annotation = annotation?.children[String(segment.value)] ?? annotation?.item_annotation ?? null
      continue
    }
    if (segment.kind === 'dict_key' && value.kind === 'dict') {
      const entry = value.value.find(([key]) => dictKeyPathText(key) === segment.value)
      if (!entry) return null
      value = entry[1]
      annotation = annotation?.children[segment.value] ?? annotation?.item_annotation ?? null
      continue
    }
    return null
  }

  return {
    coordinate: record.coordinate,
    fieldPath,
    annotation,
    value,
    writable,
  }
}

export interface PasteError {
  cell: PasteCell
  message: string
}

export type PastePlan =
  | { ok: true; writes: BatchWriteFieldInput[] }
  | { ok: false; errors: PasteError[] }

export async function planPaste(
  source: readonly (readonly string[])[],
  targets: readonly (readonly PasteCell[])[],
  context: PasteContext,
): Promise<PastePlan> {
  if (source.length === 0 || targets.length === 0 || targets[0]?.length === 0) {
    return { ok: true, writes: [] }
  }
  const oneTarget = targets.length === 1 && targets[0].length === 1
  const target = targets[0][0]
  if (oneTarget && isComplex(target.annotation)) {
    return planComplex(source, target, context)
  }

  const writes: BatchWriteFieldInput[] = []
  const errors: PasteError[] = []
  const broadcast = source.length === 1 && source[0].length === 1
  for (let row = 0; row < targets.length; row++) {
    for (let column = 0; column < targets[row].length; column++) {
      const text = broadcast ? source[0][0] : source[row]?.[column]
      if (text === undefined) continue
      const cell = targets[row][column]
      if (!cell.writable || cell.annotation?.read_only) {
        errors.push({ cell, message: '目标单元格为只读' })
        continue
      }
      const value = await parseForCell(cell, text, context, errors)
      if (value && !fieldValuesEqual(value, cell.value)) writes.push(write(cell, value))
    }
  }
  return errors.length > 0 ? { ok: false, errors } : { ok: true, writes }
}

async function planComplex(
  source: readonly (readonly string[])[],
  cell: PasteCell,
  context: PasteContext,
): Promise<PastePlan> {
  if (!cell.writable || cell.annotation?.read_only) {
    return { ok: false, errors: [{ cell, message: '目标单元格为只读' }] }
  }
  const annotation = cell.annotation!
  const errors: PasteError[] = []
  let value: CfdValue | undefined
  if (annotation.item_annotation) {
    value = await parseArray(source, cell, context, errors)
  } else if ((annotation.field_order?.length ?? 0) > 0) {
    value = await parseObjectRow(source, cell, annotation, context, errors)
  } else {
    value = await parseDirect(cell, source[0]?.[0] ?? '', context, errors)
  }
  return value && errors.length === 0
    ? { ok: true, writes: fieldValuesEqual(value, cell.value) ? [] : [write(cell, value)] }
    : { ok: false, errors }
}

async function parseForCell(
  cell: PasteCell,
  text: string,
  context: PasteContext,
  errors: PasteError[],
): Promise<CfdValue | undefined> {
  if (context.mode === 'append') {
    if (!cell.annotation?.item_annotation) {
      errors.push({ cell, message: '追加粘贴仅支持 array 目标' })
      return undefined
    }
    const incoming = await parseArray([[text]], cell, { ...context, mode: 'replace' }, errors)
    if (!incoming || incoming.kind !== 'array') return undefined
    if (cell.value.kind !== 'array' && cell.value.kind !== 'option_none') {
      errors.push({ cell, message: '当前目标值不是 array 或 None' })
      return undefined
    }
    const current = cell.value.kind === 'array' ? cell.value.value : []
    return { kind: 'array', value: [...current, ...incoming.value] }
  }
  if (cell.annotation?.item_annotation) return parseArray([[text]], cell, context, errors)
  return parseDirect(cell, text, context, errors)
}

async function parseArray(
  source: readonly (readonly string[])[],
  cell: PasteCell,
  context: PasteContext,
  errors: PasteError[],
): Promise<CfdValue | undefined> {
  const item = cell.annotation?.item_annotation
  if (!item) return parseDirect(cell, source[0]?.[0] ?? '', context, errors)
  let incoming: CfdValue | undefined
  if (source.length === 1 && source[0].length === 1) {
    incoming = await tryParse(cell, cell.fieldPath, source[0][0], context)
    if (incoming?.kind !== 'array') {
      incoming = await tryParse(cell, [...cell.fieldPath, fieldPathIndex(0)], source[0][0], context)
      if (incoming) incoming = { kind: 'array', value: [incoming] }
    }
  } else if ((item.field_order?.length ?? 0) > 0 && source[0].length > 1) {
    if (source.some(row => row.length !== item.field_order.length)) {
      errors.push({ cell, message: `array object 需要 ${item.field_order.length} 列` })
      return undefined
    }
    const items: CfdValue[] = []
    for (const row of source) {
      const parsed = await parseObjectFields(row, cell, item, [...cell.fieldPath, fieldPathIndex(0)], context, errors)
      if (parsed) items.push(parsed)
    }
    incoming = { kind: 'array', value: items }
  } else {
    const items: CfdValue[] = []
    for (const text of source.flat()) {
      const parsed = await tryParse(cell, [...cell.fieldPath, fieldPathIndex(0)], text, context)
      if (parsed) items.push(parsed)
      else errors.push({ cell, message: `无法按 array item 类型解析“${text}”` })
    }
    incoming = { kind: 'array', value: items }
  }
  if (!incoming || incoming.kind !== 'array') {
    if (errors.length === 0) errors.push({ cell, message: '无法按 array 或 array item 类型解析' })
    return undefined
  }
  if (context.mode !== 'append') return incoming
  if (cell.value.kind !== 'array' && cell.value.kind !== 'option_none') {
    errors.push({ cell, message: '当前目标值不是 array 或 None' })
    return undefined
  }
  const current = cell.value.kind === 'array' ? cell.value.value : []
  return { kind: 'array', value: [...current, ...incoming.value] }
}

async function parseObjectRow(
  source: readonly (readonly string[])[],
  cell: PasteCell,
  annotation: FieldAnnotation,
  context: PasteContext,
  errors: PasteError[],
): Promise<CfdValue | undefined> {
  if (source.length === 1 && source[0].length === 1) {
    const direct = await tryParse(cell, cell.fieldPath, source[0][0], context)
    if (direct) return direct
  }
  if (source.length !== 1) {
    errors.push({ cell, message: '单个 object 目标只接受一行数据' })
    return undefined
  }
  return parseObjectFields(source[0], cell, annotation, cell.fieldPath, context, errors)
}

async function parseObjectFields(
  row: readonly string[],
  cell: PasteCell,
  annotation: FieldAnnotation,
  basePath: FieldPathSegment[],
  context: PasteContext,
  errors: PasteError[],
): Promise<CfdValue | undefined> {
  const fields = annotation.field_order ?? []
  if (row.length !== fields.length) {
    errors.push({ cell, message: `object 需要 ${fields.length} 列，实际为 ${row.length} 列` })
    return undefined
  }
  const actualType = objectType(cell.value, annotation)
  if (!actualType) {
    errors.push({ cell, message: '无法确定 object 的具体类型' })
    return undefined
  }
  const values: Record<string, CfdValue> = {}
  for (let index = 0; index < fields.length; index++) {
    const parsed = await tryParse(cell, [...basePath, fieldPathField(fields[index])], row[index], context)
    if (parsed) values[fields[index]] = parsed
    else errors.push({ cell, message: `字段 ${fields[index]} 解析失败` })
  }
  return errors.length > 0
    ? undefined
    : { kind: 'object', value: { actual_type: actualType, fields: values } }
}

function objectType(value: CfdValue, annotation: FieldAnnotation): string | null {
  if (value.kind === 'object') return value.value.actual_type
  return annotation.object_type ?? (annotation.polymorphic_types.length === 1 ? annotation.polymorphic_types[0] : null)
}

async function parseDirect(
  cell: PasteCell,
  text: string,
  context: PasteContext,
  errors: PasteError[],
): Promise<CfdValue | undefined> {
  const parsed = await tryParse(cell, cell.fieldPath, text, context)
  if (!parsed) errors.push({ cell, message: `无法解析“${text}”` })
  return parsed
}

async function tryParse(
  cell: PasteCell,
  path: FieldPathSegment[],
  text: string,
  context: PasteContext,
): Promise<CfdValue | undefined> {
  try {
    return await context.parse(cell.coordinate, path, text)
  } catch {
    return undefined
  }
}

function isComplex(annotation: FieldAnnotation | null): boolean {
  return !!annotation?.item_annotation || (annotation?.field_order?.length ?? 0) > 0
}

function write(cell: PasteCell, value: CfdValue): BatchWriteFieldInput {
  return { coordinate: cell.coordinate, field_path: cell.fieldPath, new_value: value }
}

export function shouldExpandSinglePasteTarget(
  source: readonly (readonly string[])[],
  target: PasteCell,
): boolean {
  const hasMultipleSourceCells = source.length > 1 || (source[0]?.length ?? 0) > 1
  return hasMultipleSourceCells && !isComplex(target.annotation)
}

function dictKeyPathText(key: CfdDictKey): string {
  if (key.kind === 'int') return String(key.value)
  if (key.kind === 'enum') {
    return key.value.variant
      ? `${key.value.enum_name}.${key.value.variant}`
      : `${key.value.enum_name}(${key.value.value})`
  }
  return JSON.stringify(key.value)
}
