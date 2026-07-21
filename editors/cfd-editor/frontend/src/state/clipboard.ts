import type { BatchWriteFieldInput } from '../bindings/BatchWriteFieldInput'
import type { CfdValue } from '../bindings/CfdValue'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import { fieldPathField, fieldPathIndex, type FieldPathSegment } from '../wire'
import type { CellAnchor } from './editorSelection'
import { fieldValuesEqual } from './batchRecordProjection'

export async function serializeCellMatrix(
  rows: readonly (readonly CellAnchor[])[],
  render: (coordinate: RecordCoordinate, path: FieldPathSegment[]) => Promise<string>,
): Promise<string> {
  const rendered: string[] = []
  for (const row of rows) {
    const cells: string[] = []
    for (const cell of row) cells.push(escapeTsv(await render(cell.coordinate, cell.fieldPath)))
    rendered.push(cells.join('\t'))
  }
  return rendered.join('\n')
}

export function serializeRecordsToRefColumn(records: readonly RecordCoordinate[]): string {
  return records.map(record => escapeTsv(`&${record.key}`)).join('\n')
}

function escapeTsv(text: string): string {
  return /[\t\r\n"]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text
}

export function parseTsv(text: string): string[][] {
  if (text.length === 0) return [['']]
  const rows: string[][] = []
  let row: string[] = []
  let cell = ''
  let quoted = false
  let quoteClosed = false
  for (let i = 0; i < text.length; i++) {
    const char = text[i]
    if (quoted) {
      if (char === '"' && text[i + 1] === '"') {
        cell += '"'
        i++
      } else if (char === '"') {
        quoted = false
        quoteClosed = true
      } else {
        cell += char
      }
      continue
    }
    if (quoteClosed && char !== '\t' && char !== '\r' && char !== '\n') {
      throw new Error('TSV quoted field has trailing characters')
    }
    if (char === '"') {
      if (cell.length > 0 || quoteClosed) throw new Error('TSV quote must start a field')
      quoted = true
      continue
    }
    if (char === '\t') {
      row.push(cell)
      cell = ''
      quoteClosed = false
      continue
    }
    if (char === '\r' || char === '\n') {
      if (char === '\r' && text[i + 1] === '\n') i++
      row.push(cell)
      rows.push(row)
      row = []
      cell = ''
      quoteClosed = false
      continue
    }
    cell += char
  }
  if (quoted) throw new Error('TSV quoted field is not closed')
  if (row.length > 0 || cell.length > 0 || text.endsWith('\t')) {
    row.push(cell)
    rows.push(row)
  }
  const width = Math.max(0, ...rows.map(item => item.length))
  for (const item of rows) while (item.length < width) item.push('')
  return rows
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
    if (cell.value.kind !== 'array' && cell.value.kind !== 'null') {
      errors.push({ cell, message: '当前目标值不是 array 或 null' })
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
  if (cell.value.kind !== 'array' && cell.value.kind !== 'null') {
    errors.push({ cell, message: '当前目标值不是 array 或 null' })
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
