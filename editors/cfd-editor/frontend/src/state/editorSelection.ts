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
}

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
  return { kind: 'value', filePath, coordinate, fieldPath }
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
): boolean {
  return selection?.kind === 'value'
    && selection.filePath === filePath
    && sameCoordinate(selection.coordinate, coordinate)
    && sameFieldPath(selection.fieldPath, fieldPath)
}

export function rebindSelection(
  selection: EditorSelection | null,
  filePath: string,
  oldCoordinate: RecordCoordinate,
  newCoordinate: RecordCoordinate,
): EditorSelection | null {
  if (!selection || selection.filePath !== filePath) return selection
  if (selection.kind === 'value') {
    return sameCoordinate(selection.coordinate, oldCoordinate)
      ? { ...selection, coordinate: newCoordinate }
      : selection
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
    return sameCoordinate(selection.coordinate, coordinate) ? null : selection
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

function sameFieldPath(left: FieldPathSegment[], right: FieldPathSegment[]): boolean {
  return left.length === right.length && left.every((segment, index) => {
    const other = right[index]
    return segment.kind === other.kind && segment.value === other.value
  })
}
