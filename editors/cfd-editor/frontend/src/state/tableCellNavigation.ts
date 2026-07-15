import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import { fieldPathField, sameCoordinate } from '../wire'
import {
  recordSelection,
  valueSelection,
  type EditorSelection,
} from './editorSelection'

export type TableDirection = 'ArrowLeft' | 'ArrowRight' | 'ArrowUp' | 'ArrowDown'

export type TableEditIntent =
  | { kind: 'replace'; text: string }
  | { kind: 'edit' }

export function moveTableSelection(
  selection: EditorSelection,
  direction: TableDirection,
  rows: readonly RecordCoordinate[],
  columns: readonly string[],
): EditorSelection {
  const rowIndex = rows.findIndex(row => sameCoordinate(row, selection.coordinate))
  if (rowIndex < 0) return selection

  if (direction === 'ArrowUp' || direction === 'ArrowDown') {
    const nextIndex = Math.max(0, Math.min(
      rows.length - 1,
      rowIndex + (direction === 'ArrowDown' ? 1 : -1),
    ))
    if (nextIndex === rowIndex) return selection
    return selection.kind === 'record'
      ? recordSelection(selection.filePath, rows[nextIndex])
      : valueSelection(selection.filePath, rows[nextIndex], selection.fieldPath)
  }

  if (selection.kind === 'record') {
    return direction === 'ArrowRight' && columns.length > 0
      ? valueSelection(selection.filePath, rows[rowIndex], [fieldPathField(columns[0])])
      : selection
  }

  const field = selection.fieldPath.length === 1 && selection.fieldPath[0].kind === 'field'
    ? selection.fieldPath[0].value
    : null
  const columnIndex = field === null ? -1 : columns.indexOf(field)
  if (columnIndex < 0) return selection
  if (direction === 'ArrowLeft') {
    return columnIndex === 0
      ? recordSelection(selection.filePath, rows[rowIndex])
      : valueSelection(selection.filePath, rows[rowIndex], [fieldPathField(columns[columnIndex - 1])])
  }
  return columnIndex < columns.length - 1
    ? valueSelection(selection.filePath, rows[rowIndex], [fieldPathField(columns[columnIndex + 1])])
    : selection
}

export function editIntentForKey(key: string, modified: boolean): TableEditIntent | null {
  if (modified) return null
  if (key === 'Enter' || key === 'F2') return { kind: 'edit' }
  return key.length === 1 ? { kind: 'replace', text: key } : null
}
