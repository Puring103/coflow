import { useEffect, useMemo, useRef, useState } from 'react'
import type { DimensionFileRecords, DimensionFileRow } from '../api'
import type { DimensionValueState } from '../bindings/DimensionValueState'
import type { EditorProjectSettings } from '../bindings/EditorProjectSettings'
import type { EditorRecordGroup } from '../bindings/EditorRecordGroup'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { FieldPathSegment, FieldValue } from '../wire'
import { coordinateId, fieldPathField, sameCoordinate } from '../wire'
import { summaryOf as valueSummary } from '../value/fieldValue'
import { DataCardCompact, DirectEditor, InlineEditor } from './DataCard'
import { Icon } from './Icon'
import { RecordGroupHeader, RecordUngroupedHeader, recordGroupColorStyle } from './RecordGroupHeader'

interface Props {
  data: DimensionFileRecords
  mode: 'table' | 'record'
  recordGroupsByFile?: EditorProjectSettings['record_groups']
  onRenameGroup?: (filePath: string, actualType: string, groupId: string, name: string) => void
  onColorGroup?: (filePath: string, actualType: string, groupId: string, color: string | null) => void
  onWrite: (
    row: DimensionFileRow,
    variant: string,
    expected: DimensionValueState,
    next: DimensionValueState,
  ) => Promise<void>
  onRenderCellText?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[]) => Promise<string>
  onParseCellText?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], text: string) => Promise<FieldValue>
  onExitLeft?: () => void
  onExitUp?: () => void
  focusRequest?: number
  onFocusRequestConsumed?: (request: number) => void
}

export interface DimensionCellSelection { row: number; column: number }

export interface DimensionRecordGroupView {
  key: string
  ownerFilePath: string
  settingsType: string
  group: EditorRecordGroup
  rows: DimensionFileRow[]
}

export type DimensionDisplayItem =
  | { kind: 'group'; view: DimensionRecordGroupView }
  | { kind: 'ungrouped'; rows: DimensionFileRow[] }
  | { kind: 'row'; row: DimensionFileRow; group?: EditorRecordGroup }

export function organizeDimensionRows(
  rows: readonly DimensionFileRow[],
  groupsByFile: EditorProjectSettings['record_groups'],
  collapsed: ReadonlySet<string>,
): DimensionDisplayItem[] {
  const views = new Map<string, DimensionRecordGroupView>()
  const ungrouped: DimensionFileRow[] = []
  for (const row of rows) {
    const typeGroups = groupsByFile[row.owner_file_path] ?? {}
    let match: DimensionRecordGroupView | undefined
    for (const [settingsType, groups] of Object.entries(typeGroups)) {
      const group = groups?.find(candidate => candidate.records.some(member => sameCoordinate(member, row.coordinate)))
      if (!group) continue
      const key = `${row.owner_file_path}\u001f${settingsType}\u001f${group.id}`
      match = views.get(key) ?? { key, ownerFilePath: row.owner_file_path, settingsType, group, rows: [] }
      if (!views.has(key)) views.set(key, match)
      break
    }
    if (!match) {
      ungrouped.push(row)
      continue
    }
    match.rows.push(row)
  }

  const items: DimensionDisplayItem[] = []
  for (const view of views.values()) {
    items.push({ kind: 'group', view })
    if (!collapsed.has(view.key)) {
      items.push(...view.rows.map(row => ({ kind: 'row' as const, row, group: view.group })))
    }
  }
  if (views.size > 0) items.push({ kind: 'ungrouped', rows: ungrouped })
  items.push(...ungrouped.map(row => ({ kind: 'row' as const, row })))
  return items
}

export function moveDimensionCell(
  selection: DimensionCellSelection,
  direction: 'ArrowLeft' | 'ArrowRight' | 'ArrowUp' | 'ArrowDown',
  rowCount: number,
  columnCount: number,
): DimensionCellSelection {
  return {
    row: Math.max(0, Math.min(rowCount - 1, selection.row
      + (direction === 'ArrowDown' ? 1 : direction === 'ArrowUp' ? -1 : 0))),
    column: Math.max(0, Math.min(columnCount - 1, selection.column
      + (direction === 'ArrowRight' ? 1 : direction === 'ArrowLeft' ? -1 : 0))),
  }
}

export function DimensionTableView({
  data,
  mode,
  recordGroupsByFile = {},
  onRenameGroup,
  onColorGroup,
  onWrite,
  onRenderCellText,
  onParseCellText,
  onExitLeft,
  onExitUp,
  focusRequest = 0,
  onFocusRequestConsumed,
}: Props) {
  const [selectedKey, setSelectedKey] = useState(data.rows[0] ? coordinateId(data.rows[0].coordinate) : '')
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(() => new Set())
  const [recordField, setRecordField] = useState(0)
  const tableRef = useRef<HTMLDivElement>(null)
  const listRef = useRef<HTMLElement>(null)
  const recordRef = useRef<HTMLElement>(null)
  useEffect(() => {
    if (!data.rows.some(row => coordinateId(row.coordinate) === selectedKey)) {
      setSelectedKey(data.rows[0] ? coordinateId(data.rows[0].coordinate) : '')
    }
  }, [data, selectedKey])
  const items = useMemo(
    () => organizeDimensionRows(data.rows, recordGroupsByFile, collapsedGroups),
    [data.rows, recordGroupsByFile, collapsedGroups],
  )
  const visibleRows = useMemo(
    () => items.flatMap(item => item.kind === 'row' ? [item.row] : []),
    [items],
  )
  const selected = data.rows.find(row => coordinateId(row.coordinate) === selectedKey) ?? data.rows[0]
  const toggleGroup = (key: string) => setCollapsedGroups(current => {
    const next = new Set(current)
    if (next.has(key)) next.delete(key); else next.add(key)
    return next
  })

  useEffect(() => {
    if (!focusRequest) return
    if (mode === 'table') tableRef.current?.focus({ preventScroll: true })
    else listRef.current?.querySelector<HTMLElement>('.selected')?.focus({ preventScroll: true })
    onFocusRequestConsumed?.(focusRequest)
  }, [focusRequest, mode, onFocusRequestConsumed])

  return (
    <div className="dimension-table-view">
      <div className="dimension-table-meta">
        <strong>{data.field}</strong>
        <span>{data.rows.length} 条</span>
      </div>
      {mode === 'table' ? (
        <DimensionGrid
          data={data}
          onWrite={onWrite}
          onRenderCellText={onRenderCellText}
          onParseCellText={onParseCellText}
          rootRef={tableRef}
          items={items}
          collapsedGroups={collapsedGroups}
          onToggleGroup={toggleGroup}
          onRenameGroup={onRenameGroup}
          onColorGroup={onColorGroup}
          onExitLeft={onExitLeft}
          onExitUp={onExitUp}
        />
      ) : (
        <div className="dimension-record-layout">
          <aside
            className="dimension-record-list"
            aria-label="维度记录"
            ref={listRef}
            onKeyDown={event => {
              if (!isDirection(event.key)) return
              const index = Math.max(0, visibleRows.findIndex(row => coordinateId(row.coordinate) === selectedKey))
              if (event.key === 'ArrowLeft') {
                event.preventDefault()
                onExitLeft?.()
                return
              }
              if (event.key === 'ArrowRight') {
                event.preventDefault()
                recordRef.current?.focus({ preventScroll: true })
                return
              }
              if (event.key === 'ArrowUp' && index === 0) {
                event.preventDefault()
                onExitUp?.()
                return
              }
              if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return
              event.preventDefault()
              const next = Math.max(0, Math.min(visibleRows.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)))
              const row = visibleRows[next]
              if (!row) return
              setSelectedKey(coordinateId(row.coordinate))
              requestAnimationFrame(() => listRef.current?.querySelector<HTMLElement>(
                `[data-record-index="${next}"]`,
              )?.focus({ preventScroll: true }))
            }}
          >
            {items.map(item => item.kind === 'group' ? (
              <RecordGroupHeader
                key={`group:${item.view.key}`}
                name={item.view.group.name}
                groupId={item.view.key}
                count={item.view.rows.length}
                collapsed={collapsedGroups.has(item.view.key)}
                color={item.view.group.color}
                className="dimension-record-group-header"
                onToggle={() => toggleGroup(item.view.key)}
                onRename={name => onRenameGroup?.(item.view.ownerFilePath, item.view.settingsType, item.view.group.id, name)}
                onColorChange={color => onColorGroup?.(item.view.ownerFilePath, item.view.settingsType, item.view.group.id, color)}
              />
            ) : item.kind === 'ungrouped' ? (
              <RecordUngroupedHeader key="ungrouped" count={item.rows.length} className="dimension-record-group-header" />
            ) : (
              <button
                key={coordinateId(item.row.coordinate)}
                type="button"
                className={`${coordinateId(item.row.coordinate) === selectedKey ? 'selected' : ''}${item.group?.color ? ' has-group-color' : ''}`}
                style={recordGroupColorStyle(item.group?.color)}
                data-record-index={visibleRows.indexOf(item.row)}
                onClick={() => { setSelectedKey(coordinateId(item.row.coordinate)); setRecordField(0) }}
              >
                {item.row.coordinate.key}
              </button>
            ))}
          </aside>
          <main
            className="dimension-record-main"
            ref={recordRef}
            tabIndex={0}
            onKeyDown={event => {
              if (isNativeEditorTarget(event.target) || !isDirection(event.key)) return
              if (event.key === 'ArrowLeft') {
                event.preventDefault()
                listRef.current?.querySelector<HTMLElement>('.selected')?.focus({ preventScroll: true })
                return
              }
              if (event.key === 'ArrowUp' || event.key === 'ArrowDown') {
                event.preventDefault()
                setRecordField(current => Math.max(0, Math.min(
                  data.variants.length,
                  current + (event.key === 'ArrowDown' ? 1 : -1),
                )))
                return
              }
              if (event.key === 'ArrowRight' || event.key === 'Enter') {
                event.preventDefault()
                focusDimensionEditor(recordRef.current, recordField)
              }
            }}
          >
            {selected ? <DimensionRecord row={selected} variants={data.variants} onWrite={onWrite} selectedField={recordField} onSelectField={setRecordField} /> : (
              <div className="empty-hint">没有可显示的记录</div>
            )}
          </main>
        </div>
      )}
    </div>
  )
}

interface DimensionRange { rowStart: number; rowEnd: number; colStart: number; colEnd: number }

function rangeCells(range: DimensionRange): DimensionCellSelection[] {
  const cells: DimensionCellSelection[] = []
  for (let r = range.rowStart; r <= range.rowEnd; r++) {
    for (let c = range.colStart; c <= range.colEnd; c++) {
      cells.push({ row: r, column: c })
    }
  }
  return cells
}

function inRange(range: DimensionRange, row: number, column: number): boolean {
  return row >= range.rowStart && row <= range.rowEnd && column >= range.colStart && column <= range.colEnd
}

// column 0 = key (readonly), column 1 = default (readonly), column 2+ = variants
function isEditableColumn(column: number): boolean { return column >= 2 }

function DimensionGrid({ data, onWrite, onRenderCellText, onParseCellText, rootRef, onExitLeft, onExitUp, items, collapsedGroups, onToggleGroup, onRenameGroup, onColorGroup }: Pick<Props, 'data' | 'onWrite' | 'onRenderCellText' | 'onParseCellText' | 'onExitLeft' | 'onExitUp' | 'onRenameGroup' | 'onColorGroup'> & {
  rootRef: React.RefObject<HTMLDivElement | null>
  items: DimensionDisplayItem[]
  collapsedGroups: ReadonlySet<string>
  onToggleGroup: (key: string) => void
}) {
  const [anchor, setAnchor] = useState<DimensionCellSelection>({ row: 0, column: 0 })
  const [rangeEnd, setRangeEnd] = useState<DimensionCellSelection | null>(null)
  // inlineEdit: { row, column, initialText } — overlay syntax editor on that cell
  const [inlineEdit, setInlineEdit] = useState<{ row: number; column: number; initialText: string } | null>(null)
  const clipboardBusyRef = useRef(false)

  const columnCount = data.variants.length + 2
  const visibleRows = items.flatMap(item => item.kind === 'row' ? [item.row] : [])

  const selection = anchor
  const range: DimensionRange = rangeEnd ? {
    rowStart: Math.min(anchor.row, rangeEnd.row),
    rowEnd: Math.max(anchor.row, rangeEnd.row),
    colStart: Math.min(anchor.column, rangeEnd.column),
    colEnd: Math.max(anchor.column, rangeEnd.column),
  } : { rowStart: anchor.row, rowEnd: anchor.row, colStart: anchor.column, colEnd: anchor.column }

  const isRangeSelection = rangeEnd !== null && (rangeEnd.row !== anchor.row || rangeEnd.column !== anchor.column)

  const select = (next: DimensionCellSelection, extend = false) => {
    if (extend) {
      setRangeEnd(next)
    } else {
      setAnchor(next)
      setRangeEnd(null)
    }
    requestAnimationFrame(() => rootRef.current?.querySelector<HTMLElement>(
      `[data-dimension-row="${next.row}"][data-dimension-column="${next.column}"]`,
    )?.scrollIntoView({ block: 'nearest', inline: 'nearest' }))
  }

  // Collect all selected (row, variantIndex) pairs — only editable columns (col >= 2)
  const selectedVariantCells = (): Array<{ row: DimensionFileRow; variantIndex: number; variant: string }> => {
    return rangeCells(range).flatMap(({ row, column }) => {
      if (!isEditableColumn(column)) return []
      const r = visibleRows[row]
      const variantIndex = column - 2
      const variant = data.variants[variantIndex]
      if (!r || !variant) return []
      return [{ row: r, variantIndex, variant }]
    })
  }

  const fieldPathForVariant = (variant: string): FieldPathSegment[] =>
    [fieldPathField(data.field), fieldPathField(variant)]

  return (
    <div
      className="dimension-table-scroll"
      ref={rootRef}
      tabIndex={0}
      onKeyDown={async event => {
        if (isNativeEditorTarget(event.target)) return
        const modified = event.ctrlKey || event.metaKey

        // Ctrl+A: select all
        if (modified && event.key.toLowerCase() === 'a') {
          event.preventDefault()
          if (visibleRows.length > 0) {
            setAnchor({ row: 0, column: 2 })
            setRangeEnd({ row: visibleRows.length - 1, column: columnCount - 1 })
          }
          return
        }

        // Ctrl+C: copy selected variant cells as TSV
        if (modified && event.key.toLowerCase() === 'c') {
          event.preventDefault()
          if (clipboardBusyRef.current) return
          clipboardBusyRef.current = true
          try {
            const cells = selectedVariantCells()
            if (cells.length === 0) return
            // Build TSV: rows are grouped by row index, columns by variant order
            const rowIndices = [...new Set(rangeCells(range).filter(c => isEditableColumn(c.column)).map(c => c.row))]
            const colIndices = [...new Set(rangeCells(range).filter(c => isEditableColumn(c.column)).map(c => c.column))].sort((a, b) => a - b)
            const lines: string[] = []
            for (const ri of rowIndices) {
              const row = visibleRows[ri]
              if (!row) continue
              const parts: string[] = []
              for (const ci of colIndices) {
                const variant = data.variants[ci - 2]
                if (!variant) { parts.push(''); continue }
                const state = row.values[variant]
                let text = ''
                if (state?.kind === 'value') {
                  if (onRenderCellText) {
                    try { text = await onRenderCellText(row.coordinate, fieldPathForVariant(variant)) } catch { text = valueSummary(state.value) }
                  } else {
                    text = valueSummary(state.value)
                  }
                }
                parts.push(escapeTsvCell(text))
              }
              lines.push(parts.join('\t'))
            }
            await navigator.clipboard.writeText(lines.join('\n'))
          } finally {
            clipboardBusyRef.current = false
          }
          return
        }

        // Ctrl+V: paste TSV into selected variant cells
        if (modified && event.key.toLowerCase() === 'v') {
          event.preventDefault()
          if (!onParseCellText || clipboardBusyRef.current) return
          clipboardBusyRef.current = true
          try {
            const text = await navigator.clipboard.readText()
            const rows = parseTsvSimple(text)
            const broadcast = rows.length === 1 && rows[0].length === 1
            // Determine target cells: start from anchor, extend by paste size if single-cell selection
            const targetColStart = Math.max(2, range.colStart)
            const pasteColCount = rows[0]?.length ?? 1
            const pasteRowCount = rows.length
            const targetRowEnd = broadcast ? range.rowEnd : range.rowStart + pasteRowCount - 1
            const targetColEnd = broadcast ? range.colEnd : targetColStart + pasteColCount - 1
            const writes: Array<() => Promise<void>> = []
            for (let ri = range.rowStart; ri <= Math.min(targetRowEnd, visibleRows.length - 1); ri++) {
              const row = visibleRows[ri]
              if (!row) continue
              for (let ci = targetColStart; ci <= Math.min(targetColEnd, columnCount - 1); ci++) {
                const variant = data.variants[ci - 2]
                if (!variant) continue
                const sourceText = broadcast ? rows[0][0] : (rows[ri - range.rowStart]?.[ci - targetColStart] ?? '')
                const state = row.values[variant] ?? { kind: 'missing' as const }
                const capturedRow = row; const capturedVariant = variant; const capturedState = state
                writes.push(async () => {
                  try {
                    const value = await onParseCellText(capturedRow.coordinate, fieldPathForVariant(capturedVariant), sourceText)
                    await onWrite(capturedRow, capturedVariant, capturedState, { kind: 'value', value })
                  } catch {
                    // skip unparseable cells
                  }
                })
              }
            }
            await Promise.all(writes.map(fn => fn()))
          } finally {
            clipboardBusyRef.current = false
          }
          return
        }

        // Delete: reset selected variant cells to missing
        if (event.key === 'Delete' && !modified) {
          event.preventDefault()
          const cells = selectedVariantCells()
          await Promise.all(cells.map(({ row, variant }) => {
            const state = row.values[variant] ?? { kind: 'missing' as const }
            if (state.kind === 'missing') return Promise.resolve()
            return onWrite(row, variant, state, { kind: 'missing' })
          }))
          return
        }

        // Enter / F2: open editor on current single cell
        if ((event.key === 'Enter' || event.key === 'F2') && !modified) {
          event.preventDefault()
          if (isEditableColumn(selection.column)) {
            const variant = data.variants[selection.column - 2]
            const row = visibleRows[selection.row]
            if (variant && row && onParseCellText) {
              const state = row.values[variant]
              let initialText = ''
              if (state?.kind === 'value') {
                if (onRenderCellText) {
                  try { initialText = await onRenderCellText(row.coordinate, fieldPathForVariant(variant)) } catch { initialText = valueSummary(state.value) }
                } else {
                  initialText = valueSummary(state.value)
                }
              }
              setInlineEdit({ row: selection.row, column: selection.column, initialText })
            } else {
              focusDimensionEditor(rootRef.current, selection.column, selection.row)
            }
          } else {
            focusDimensionEditor(rootRef.current, selection.column, selection.row)
          }
          return
        }

        // Printable character: start replace-mode inline edit
        if (!modified && event.key.length === 1 && isEditableColumn(selection.column) && onParseCellText) {
          const row = visibleRows[selection.row]
          const variant = data.variants[selection.column - 2]
          if (row && variant) {
            setInlineEdit({ row: selection.row, column: selection.column, initialText: event.key })
          }
          return
        }

        if (!isDirection(event.key)) return
        if (event.key === 'ArrowLeft' && selection.column === 0 && !event.shiftKey) {
          event.preventDefault()
          onExitLeft?.()
          return
        }
        if (event.key === 'ArrowUp' && selection.row === 0 && !event.shiftKey) {
          event.preventDefault()
          onExitUp?.()
          return
        }
        event.preventDefault()
        const next = moveDimensionCell(selection, event.key, visibleRows.length, columnCount)
        select(next, event.shiftKey)
      }}
    >
      <table className="dimension-table">
        <thead>
          <tr>
            <th>键</th>
            <th>default</th>
            {data.variants.map(variant => <th key={variant}>{variant}</th>)}
          </tr>
        </thead>
        <tbody>
          {items.map(item => item.kind === 'group' ? (
            <tr key={`group:${item.view.key}`} className="dimension-group-row">
              <th colSpan={columnCount}>
                <RecordGroupHeader
                  name={item.view.group.name}
                  groupId={item.view.key}
                  count={item.view.rows.length}
                  collapsed={collapsedGroups.has(item.view.key)}
                  color={item.view.group.color}
                  onToggle={() => onToggleGroup(item.view.key)}
                  onRename={name => onRenameGroup?.(item.view.ownerFilePath, item.view.settingsType, item.view.group.id, name)}
                  onColorChange={color => onColorGroup?.(item.view.ownerFilePath, item.view.settingsType, item.view.group.id, color)}
                />
              </th>
            </tr>
          ) : item.kind === 'ungrouped' ? (
            <tr key="ungrouped" className="dimension-group-row">
              <th colSpan={columnCount}><RecordUngroupedHeader count={item.rows.length} /></th>
            </tr>
          ) : (() => {
            const rowIndex = visibleRows.indexOf(item.row)
            return (
              <tr
                key={coordinateId(item.row.coordinate)}
                className={item.group?.color ? 'has-group-color' : undefined}
                style={recordGroupColorStyle(item.group?.color)}
              >
                <th scope="row" {...cellProps(rowIndex, 0, anchor, isRangeSelection, range, select)}>{item.row.coordinate.key}</th>
                <td {...cellProps(rowIndex, 1, anchor, isRangeSelection, range, select)}><DataCardCompact value={item.row.default_value} label="default" /></td>
                {data.variants.map((variant, variantIndex) => {
                  const colIndex = variantIndex + 2
                  const isInline = inlineEdit?.row === rowIndex && inlineEdit.column === colIndex
                  return (
                    <td key={variant} {...cellProps(rowIndex, colIndex, anchor, isRangeSelection, range, select)}>
                      {isInline && onParseCellText ? (
                        <DimensionInlineEditor
                          row={item.row}
                          variant={variant}
                          initialText={inlineEdit.initialText}
                          onWrite={onWrite}
                          onParseCellText={onParseCellText}
                          fieldPath={fieldPathForVariant(variant)}
                          onDone={() => {
                            setInlineEdit(null)
                            requestAnimationFrame(() => rootRef.current?.focus({ preventScroll: true }))
                          }}
                        />
                      ) : (
                        <DimensionCellEditor row={item.row} variant={variant} onWrite={onWrite} />
                      )}
                    </td>
                  )
                })}
              </tr>
            )
          })())}
        </tbody>
      </table>
      {data.rows.length === 0 && <div className="empty-hint">没有可显示的记录</div>}
    </div>
  )
}

function DimensionRecord({ row, variants, onWrite, selectedField, onSelectField }: {
  row: DimensionFileRow
  variants: string[]
  onWrite: Props['onWrite']
  selectedField: number
  onSelectField: (index: number) => void
}) {
  return (
    <div className="dimension-record-card">
      <header><strong>{row.coordinate.key}</strong></header>
      <div className={`dimension-record-row readonly${selectedField === 0 ? ' keyboard-selected' : ''}`} data-dimension-record-field="0" onMouseDown={() => onSelectField(0)}>
        <span>default</span>
        <DataCardCompact value={row.default_value} label="default" />
      </div>
      {variants.map((variant, index) => (
        <div className={`dimension-record-row${selectedField === index + 1 ? ' keyboard-selected' : ''}`} key={variant} data-dimension-record-field={index + 1} onMouseDown={() => onSelectField(index + 1)}>
          <span>{variant}</span>
          <DimensionCellEditor row={row} variant={variant} onWrite={onWrite} />
        </div>
      ))}
    </div>
  )
}

function cellProps(
  row: number,
  column: number,
  anchor: DimensionCellSelection,
  isRangeSelection: boolean,
  range: DimensionRange,
  onSelect: (selection: DimensionCellSelection, extend?: boolean) => void,
) {
  const isAnchor = anchor.row === row && anchor.column === column
  const isSelected = isRangeSelection ? inRange(range, row, column) : isAnchor
  return {
    'data-dimension-row': row,
    'data-dimension-column': column,
    className: isSelected ? (isAnchor ? 'keyboard-selected' : 'range-selected') : undefined,
    onMouseDown: (e: React.MouseEvent) => onSelect({ row, column }, e.shiftKey),
  }
}

function escapeTsvCell(text: string): string {
  return /[\t\r\n"]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text
}

function parseTsvSimple(text: string): string[][] {
  if (!text) return [['']]
  const lines = text.split(/\r?\n/)
  const rows = lines.filter((_, i) => i < lines.length - 1 || lines[i].length > 0)
  return rows.map(line => line.split('\t'))
}

function isDirection(key: string): key is 'ArrowLeft' | 'ArrowRight' | 'ArrowUp' | 'ArrowDown' {
  return key === 'ArrowLeft' || key === 'ArrowRight' || key === 'ArrowUp' || key === 'ArrowDown'
}

function isNativeEditorTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  return target !== target.closest('.dimension-table-scroll, .dimension-record-main')
    && (target.matches('input, textarea, select, button') || target.isContentEditable)
}

function focusDimensionEditor(root: HTMLElement | null, column: number, row?: number) {
  if (!root) return
  const cell = row === undefined
    ? root.querySelector<HTMLElement>(`[data-dimension-record-field="${column}"]`)
    : root.querySelector<HTMLElement>(`[data-dimension-row="${row}"][data-dimension-column="${column}"]`)
  const editor = cell?.querySelector<HTMLElement>('input, textarea, select, button.dimension-value-missing')
  editor?.focus({ preventScroll: true })
  if (editor instanceof HTMLInputElement || editor instanceof HTMLTextAreaElement) editor.select()
}

function DimensionInlineEditor({ row, variant, initialText, onWrite, onParseCellText, fieldPath, onDone }: {
  row: DimensionFileRow
  variant: string
  initialText: string
  onWrite: Props['onWrite']
  onParseCellText: NonNullable<Props['onParseCellText']>
  fieldPath: FieldPathSegment[]
  onDone: () => void
}) {
  const state = row.values[variant] ?? { kind: 'missing' as const }
  const [draft, setDraft] = useState(initialText)
  const [busy, setBusy] = useState(false)

  const commit = async () => {
    if (busy) return
    setBusy(true)
    try {
      const value = await onParseCellText(row.coordinate, fieldPath, draft)
      await onWrite(row, variant, state, { kind: 'value', value })
      onDone()
    } catch {
      onDone()
    } finally {
      setBusy(false)
    }
  }

  return (
    <input
      className="dimension-inline-input"
      value={draft}
      autoFocus
      onChange={e => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={e => {
        e.stopPropagation()
        if (e.key === 'Enter') { e.preventDefault(); void commit() }
        if (e.key === 'Escape') { e.preventDefault(); onDone() }
      }}
    />
  )
}

function DimensionCellEditor({ row, variant, onWrite }: {
  row: DimensionFileRow
  variant: string
  onWrite: Props['onWrite']
}) {
  const state = row.values[variant] ?? { kind: 'missing' as const }
  const [creating, setCreating] = useState(false)
  const [busy, setBusy] = useState(false)

  const commit = async (value: FieldValue) => {
    setBusy(true)
    try {
      await onWrite(row, variant, state, { kind: 'value', value })
      setCreating(false)
    } finally {
      setBusy(false)
    }
  }
  if (state.kind === 'missing') {
    if (creating) {
      return (
        <div className={`dimension-cell-editor${busy ? ' busy' : ''}`}>
          <InlineEditor
            value={row.default_value}
            onCommit={commit}
            onCancel={() => setCreating(false)}
          />
        </div>
      )
    }
    return (
      <button
        type="button"
        className="dimension-value-missing"
        onClick={() => setCreating(true)}
        title="设置值"
      >
        -
      </button>
    )
  }
  return (
    <div className={`dimension-cell-editor${busy ? ' busy' : ''}`}>
      <DirectEditor value={state.value} onCommit={commit} />
      <button
        type="button"
        className="dimension-cell-clear"
        onClick={async () => {
          setBusy(true)
          try {
            await onWrite(row, variant, state, { kind: 'missing' })
          } finally {
            setBusy(false)
          }
        }}
        disabled={busy}
        title="清除变体值"
        aria-label="清除变体值"
      >
        <Icon name="close" size={11} aria-hidden />
      </button>
    </div>
  )
}
