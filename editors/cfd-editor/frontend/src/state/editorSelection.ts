import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import { sameCoordinate, type FieldPathSegment } from '../wire'

export type EditorSelection = RecordSelection | ValueSelection

export interface RecordSelection {
  kind: 'record'
  filePath: string
  /** Focused record. Existing single-record consumers use this coordinate. */
  coordinate: RecordCoordinate
  coordinates: RecordCoordinate[]
  anchor: RecordCoordinate
}

export type RecordSelectionMode = 'replace' | 'toggle' | 'range'

export interface ValueSelection {
  kind: 'value'
  filePath: string
  coordinate: RecordCoordinate
  fieldPath: FieldPathSegment[]
  rangeAnchor: CellAnchor
}

export interface CellAnchor {
  coordinate: RecordCoordinate
  fieldPath: FieldPathSegment[]
}

export type ValueSelectionMode = 'replace' | 'range'

export function recordSelection(
  filePath: string,
  coordinate: RecordCoordinate,
): RecordSelection {
  return { kind: 'record', filePath, coordinate, coordinates: [coordinate], anchor: coordinate }
}

export function recordSelectionCoordinates(
  selection: EditorSelection | null,
): readonly RecordCoordinate[] {
  return selection?.kind === 'record' ? selection.coordinates : []
}

export function updateRecordSelection(
  selection: EditorSelection | null,
  filePath: string,
  coordinate: RecordCoordinate,
  visibleCoordinates: readonly RecordCoordinate[],
  mode: RecordSelectionMode,
): RecordSelection | null {
  if (mode === 'replace' || selection?.kind !== 'record' || selection.filePath !== filePath) {
    return recordSelection(filePath, coordinate)
  }

  if (mode === 'toggle') {
    const selected = selection.coordinates.some(item => sameCoordinate(item, coordinate))
    const coordinates = selected
      ? selection.coordinates.filter(item => !sameCoordinate(item, coordinate))
      : [...selection.coordinates, coordinate]
    if (coordinates.length === 0) return null
    return {
      ...selection,
      coordinate: selected && sameCoordinate(selection.coordinate, coordinate)
        ? coordinates[coordinates.length - 1]
        : coordinate,
      coordinates,
      anchor: coordinates.some(item => sameCoordinate(item, selection.anchor))
        ? selection.anchor
        : coordinates[0],
    }
  }

  const anchorIndex = visibleCoordinates.findIndex(item => sameCoordinate(item, selection.anchor))
  const targetIndex = visibleCoordinates.findIndex(item => sameCoordinate(item, coordinate))
  if (anchorIndex < 0 || targetIndex < 0) return recordSelection(filePath, coordinate)
  const start = Math.min(anchorIndex, targetIndex)
  const end = Math.max(anchorIndex, targetIndex)
  return {
    ...selection,
    coordinate,
    coordinates: visibleCoordinates.slice(start, end + 1),
  }
}

export function valueSelection(
  filePath: string,
  coordinate: RecordCoordinate,
  fieldPath: FieldPathSegment[],
): ValueSelection {
  return {
    kind: 'value',
    filePath,
    coordinate,
    fieldPath,
    rangeAnchor: { coordinate, fieldPath },
  }
}

export function updateValueSelection(
  selection: EditorSelection | null,
  filePath: string,
  coordinate: RecordCoordinate,
  fieldPath: FieldPathSegment[],
  mode: ValueSelectionMode,
): ValueSelection {
  if (mode === 'replace' || selection?.kind !== 'value' || selection.filePath !== filePath) {
    return valueSelection(filePath, coordinate, fieldPath)
  }
  return { ...selection, coordinate, fieldPath }
}

export interface CellRange {
  rowStart: number
  rowEnd: number
  columnStart: number
  columnEnd: number
}

export function valueSelectionRange(
  selection: EditorSelection | null,
  rows: readonly RecordCoordinate[],
  columns: readonly string[],
): CellRange | null {
  if (selection?.kind !== 'value') return null
  const anchorRow = rows.findIndex(row => sameCoordinate(row, selection.rangeAnchor.coordinate))
  const focusRow = rows.findIndex(row => sameCoordinate(row, selection.coordinate))
  const anchorColumn = columns.indexOf(topLevelField(selection.rangeAnchor.fieldPath) ?? '')
  const focusColumn = columns.indexOf(topLevelField(selection.fieldPath) ?? '')
  if (anchorRow < 0 || focusRow < 0 || anchorColumn < 0 || focusColumn < 0) return null
  return {
    rowStart: Math.min(anchorRow, focusRow),
    rowEnd: Math.max(anchorRow, focusRow),
    columnStart: Math.min(anchorColumn, focusColumn),
    columnEnd: Math.max(anchorColumn, focusColumn),
  }
}

export function valueSelectionCells(
  selection: EditorSelection | null,
  rows: readonly RecordCoordinate[],
  columns: readonly string[],
): CellAnchor[] {
  const range = valueSelectionRange(selection, rows, columns)
  if (!range) return []
  const cells: CellAnchor[] = []
  for (let row = range.rowStart; row <= range.rowEnd; row++) {
    for (let column = range.columnStart; column <= range.columnEnd; column++) {
      cells.push({ coordinate: rows[row], fieldPath: [{ kind: 'field', value: columns[column] }] })
    }
  }
  return cells
}

export function selectionMatchesRecord(
  selection: EditorSelection | null,
  filePath: string,
  coordinate: RecordCoordinate,
): boolean {
  return selection?.kind === 'record'
    && selection.filePath === filePath
    && selection.coordinates.some(item => sameCoordinate(item, coordinate))
}

export function selectionMatchesValue(
  selection: EditorSelection | null,
  filePath: string,
  coordinate: RecordCoordinate,
  fieldPath: FieldPathSegment[],
  visibleCoordinates?: readonly RecordCoordinate[],
  visibleFields?: readonly string[],
): boolean {
  if (selection?.kind !== 'value' || selection.filePath !== filePath) return false
  if (!visibleCoordinates || !visibleFields) {
    return sameCoordinate(selection.coordinate, coordinate) && sameFieldPath(selection.fieldPath, fieldPath)
  }
  return valueSelectionCells(selection, visibleCoordinates, visibleFields)
    .some(cell => sameCoordinate(cell.coordinate, coordinate) && sameFieldPath(cell.fieldPath, fieldPath))
}

export function rebindSelection(
  selection: EditorSelection | null,
  filePath: string,
  oldCoordinate: RecordCoordinate,
  newCoordinate: RecordCoordinate,
): EditorSelection | null {
  if (!selection || selection.filePath !== filePath) return selection
  if (selection.kind === 'value') {
    const focusMatches = sameCoordinate(selection.coordinate, oldCoordinate)
    const anchorMatches = sameCoordinate(selection.rangeAnchor.coordinate, oldCoordinate)
    if (!focusMatches && !anchorMatches) return selection
    return {
      ...selection,
      coordinate: focusMatches ? newCoordinate : selection.coordinate,
      rangeAnchor: anchorMatches
        ? { ...selection.rangeAnchor, coordinate: newCoordinate }
        : selection.rangeAnchor,
    }
  }
  if (!selection.coordinates.some(item => sameCoordinate(item, oldCoordinate))) return selection
  return {
    ...selection,
    coordinate: sameCoordinate(selection.coordinate, oldCoordinate)
      ? newCoordinate
      : selection.coordinate,
    anchor: sameCoordinate(selection.anchor, oldCoordinate) ? newCoordinate : selection.anchor,
    coordinates: selection.coordinates.map(item => sameCoordinate(item, oldCoordinate) ? newCoordinate : item),
  }
}

export function removeSelection(
  selection: EditorSelection | null,
  filePath: string,
  coordinate: RecordCoordinate,
): EditorSelection | null {
  if (!selection || selection.filePath !== filePath) return selection
  if (selection.kind === 'value') {
    const focusMatches = sameCoordinate(selection.coordinate, coordinate)
    const anchorMatches = sameCoordinate(selection.rangeAnchor.coordinate, coordinate)
    if (!focusMatches && !anchorMatches) return selection
    if (focusMatches && anchorMatches) return null
    const remaining = focusMatches
      ? selection.rangeAnchor
      : { coordinate: selection.coordinate, fieldPath: selection.fieldPath }
    return valueSelection(selection.filePath, remaining.coordinate, remaining.fieldPath)
  }
  const coordinates = selection.coordinates.filter(item => !sameCoordinate(item, coordinate))
  if (coordinates.length === selection.coordinates.length) return selection
  if (coordinates.length === 0) return null
  return {
    ...selection,
    coordinates,
    coordinate: sameCoordinate(selection.coordinate, coordinate)
      ? coordinates[coordinates.length - 1]
      : selection.coordinate,
    anchor: sameCoordinate(selection.anchor, coordinate) ? coordinates[0] : selection.anchor,
  }
}

function topLevelField(path: FieldPathSegment[]): string | null {
  return path.length === 1 && path[0].kind === 'field' ? path[0].value : null
}

function sameFieldPath(left: FieldPathSegment[], right: FieldPathSegment[]): boolean {
  return left.length === right.length && left.every((segment, index) => {
    const other = right[index]
    return segment.kind === other.kind && segment.value === other.value
  })
}
