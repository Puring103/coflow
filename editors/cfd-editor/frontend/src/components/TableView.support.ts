import type { FileRecords } from '../bindings/FileRecords'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import {
  cellEnumType,
  cellReadOnly,
  cellRefTargetType,
  coordinateId,
  diagnosticDisplayMessage,
  diagnosticMatchesCoordinate,
  diagnosticRecordTarget,
  fieldPathField,
  nullValue,
  recordActualType,
  sameCoordinate,
  type DiagnosticItem,
  type FieldPathSegment,
} from '../wire'
import { valueSelectionRange, type CellAnchor, type ValueSelection } from '../state/editorSelection'
import type { PasteCell } from '../state/clipboard'
import type { AxisRange } from '../state/scrollVisibility'

/** Approximate visual width in pixels for `text` at 12px UI font. Uses a
 *  codepoint scan (East Asian Wide ≈ 2 ASCII cells) instead of canvas
 *  measurement — deterministic across webview builds and cheap enough to
 *  run over every cell. Not pixel-perfect, but the caller adds chrome
 *  padding and clamps to a max, so approximate is enough. */
export function estimateTextWidth(text: string, monospace = false): number {
  const narrow = monospace ? 7.3 : 6.6
  const wide = monospace ? 13.6 : 13.2
  let w = 0
  for (const ch of text) {
    const cp = ch.codePointAt(0)!
    w += isEastAsianWide(cp) ? wide : narrow
  }
  return w
}

export function elementBounds(elements: readonly HTMLElement[]): DOMRect {
  const rects = elements.map(element => element.getBoundingClientRect())
  const left = Math.min(...rects.map(rect => rect.left))
  const top = Math.min(...rects.map(rect => rect.top))
  const right = Math.max(...rects.map(rect => rect.right))
  const bottom = Math.max(...rects.map(rect => rect.bottom))
  return new DOMRect(left, top, right - left, bottom - top)
}

export function fitsAxis(start: number, end: number, visibleStart: number, visibleEnd: number): boolean {
  const target: AxisRange = { start, end }
  const viewport: AxisRange = { start: visibleStart, end: visibleEnd }
  return target.end - target.start <= viewport.end - viewport.start
}

/** Rough East Asian Wide detection covering the ranges that show up in game
 *  data: CJK ideographs, Hangul, kana, full-width forms, CJK punctuation.
 *  Doesn't need to be exhaustive — misses under-count width by ~1 char,
 *  which the column-width clamp absorbs. */
function isEastAsianWide(cp: number): boolean {
  return (
    (cp >= 0x1100 && cp <= 0x115F) ||        // Hangul Jamo
    (cp >= 0x2E80 && cp <= 0x9FFF) ||        // CJK Radicals..Unified Ideographs
    (cp >= 0xA960 && cp <= 0xA97F) ||        // Hangul Jamo Extended-A
    (cp >= 0xAC00 && cp <= 0xD7A3) ||        // Hangul Syllables
    (cp >= 0xF900 && cp <= 0xFAFF) ||        // CJK Compatibility Ideographs
    (cp >= 0xFE30 && cp <= 0xFE4F) ||        // CJK Compatibility Forms
    (cp >= 0xFF00 && cp <= 0xFF60) ||        // Full-width Forms
    (cp >= 0xFFE0 && cp <= 0xFFE6) ||        // Full-width signs
    (cp >= 0x20000 && cp <= 0x3FFFD)         // CJK Extension B..F, supplements
  )
}

export function selectionCellMatrix(
  selection: ValueSelection,
  rows: readonly RecordCoordinate[],
  columns: readonly string[],
): CellAnchor[][] {
  const range = valueSelectionRange(selection, rows, columns)
  if (!range) return [[{ coordinate: selection.coordinate, fieldPath: selection.fieldPath }]]
  const matrix: CellAnchor[][] = []
  for (let row = range.rowStart; row <= range.rowEnd; row++) {
    const cells: CellAnchor[] = []
    for (let column = range.columnStart; column <= range.columnEnd; column++) {
      cells.push({ coordinate: rows[row], fieldPath: [fieldPathField(columns[column])] })
    }
    matrix.push(cells)
  }
  return matrix
}

export function boundedPasteMatrix(
  start: CellAnchor,
  rowCount: number,
  columnCount: number,
  rows: readonly RecordCoordinate[],
  columns: readonly string[],
): CellAnchor[][] {
  const rowStart = rows.findIndex(row => sameCoordinate(row, start.coordinate))
  const field = selectedTopLevelField(start.fieldPath)
  const columnStart = field ? columns.indexOf(field) : -1
  if (rowStart < 0 || columnStart < 0) return [[start]]
  const matrix: CellAnchor[][] = []
  for (let row = rowStart; row < Math.min(rows.length, rowStart + rowCount); row++) {
    const cells: CellAnchor[] = []
    for (let column = columnStart; column < Math.min(columns.length, columnStart + columnCount); column++) {
      cells.push({ coordinate: rows[row], fieldPath: [fieldPathField(columns[column])] })
    }
    matrix.push(cells)
  }
  return matrix
}

export function pasteCellFor(
  anchor: CellAnchor,
  rows: readonly RecordRow[],
  editable: boolean,
): PasteCell {
  const field = selectedTopLevelField(anchor.fieldPath)
  const row = rows.find(candidate => sameCoordinate(candidate.coordinate, anchor.coordinate))
  const cell = field && row ? fieldCell(row, field) : undefined
  return {
    coordinate: anchor.coordinate,
    fieldPath: anchor.fieldPath,
    annotation: cell?.annotation ?? null,
    value: cell?.value ?? nullValue(),
    writable: editable && !!cell && !cellReadOnly(cell),
  }
}

export function fieldCell(record: RecordRow, fieldName: string) {
  const index = record.field_index[fieldName]
  return typeof index === 'number' ? record.fields[index] : undefined
}

export function inferredCellType(cell: RecordRow['fields'][number] | undefined): string | undefined {
  if (!cell) return undefined
  const value = cell.value
  if (value.kind === 'enum') return value.value.enum_name
  if (value.kind === 'ref') return 'ref'
  if (value.kind === 'object') return value.value.actual_type
  if (value.kind === 'array') return 'array'
  if (value.kind === 'dict') return 'dict'
  if (value.kind === 'option_none') return 'None'
  if (value.kind === 'option_some' || value.kind === 'result_ok' || value.kind === 'result_err') {
    return inferredCellType({ ...cell, value: value.value })
  }
  return value.kind
}



/** Stable identity for the column set: file path + column names joined.
 *  Used as a memo key so column widths only recompute when the schema-
 *  determined column set changes, not when a cell value updates. */
export function columnKeySignature(data: FileRecords): string {
  return data.columns.map(c => c.name).join('')
}

export function columnDropdownKind(
  data: FileRecords,
  fieldName: string,
  activeType: string,
): 'ref' | 'enum' | 'bool' | null {
  for (const record of data.records) {
    if (recordActualType(record) !== activeType) continue
    const f = fieldCell(record, fieldName)
    if (!f) continue
    if (f.value.kind === 'ref') return 'ref'
    if (f.value.kind === 'enum') return 'enum'
    if (f.value.kind === 'bool') return 'bool'
    if (cellRefTargetType(f)) return 'ref'
    if (cellEnumType(f)) return 'enum'
  }
  return null
}

export function severityForCoordinate(
  diagnostics: DiagnosticItem[] | undefined,
  filePath: string,
  coordinate: RecordCoordinate,
): 'error' | 'warning' | null {
  if (!diagnostics) return null
  let sev: 'error' | 'warning' | null = null
  for (const d of diagnostics) {
    const target = diagnosticRecordTarget(d)
    if (!target || target.file_path !== filePath || !diagnosticMatchesCoordinate(d, coordinate)) continue
    if (d.severity === 'error') return 'error'
    if (d.severity === 'warning') sev = 'warning'
  }
  return sev
}

export function findDiagMessage(
  diags: DiagnosticItem[] | undefined,
  filePath: string,
  coordinate: RecordCoordinate,
  topField: string,
): string | undefined {
  if (!diags) return undefined
  const msgs: string[] = []
  for (const d of diags) {
    const target = diagnosticRecordTarget(d)
    if (!target || target.file_path !== filePath || !diagnosticMatchesCoordinate(d, coordinate)) continue
    const top = target.kind === 'table_field' ? target.field_path.split(/[.[]/, 1)[0] : null
    if (top !== topField) continue
    msgs.push(diagnosticDisplayMessage(d))
  }
  return msgs.length ? msgs.join('\n') : undefined
}

export function selectedTopLevelField(path: FieldPathSegment[]): string | null {
  return path.length === 1 && path[0].kind === 'field' ? path[0].value : null
}

export function tableCellKey(coordinate: RecordCoordinate, fieldName: string): string {
  return `${coordinateId(coordinate)}::${fieldName}`
}

