import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import { sameCoordinate, type FieldPathSegment } from '../wire'

export type EditorSelection = RecordSelection | ValueSelection

export interface RecordSelection {
  kind: 'record'
  filePath: string
  coordinate: RecordCoordinate
}

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
  return { kind: 'record', filePath, coordinate }
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
    && sameCoordinate(selection.coordinate, coordinate)
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
  return selection
    && selection.filePath === filePath
    && sameCoordinate(selection.coordinate, oldCoordinate)
    ? { ...selection, coordinate: newCoordinate }
    : selection
}

export function removeSelection(
  selection: EditorSelection | null,
  filePath: string,
  coordinate: RecordCoordinate,
): EditorSelection | null {
  return selection
    && selection.filePath === filePath
    && sameCoordinate(selection.coordinate, coordinate)
    ? null
    : selection
}

function sameFieldPath(left: FieldPathSegment[], right: FieldPathSegment[]): boolean {
  return left.length === right.length && left.every((segment, index) => {
    const other = right[index]
    return segment.kind === other.kind && segment.value === other.value
  })
}
