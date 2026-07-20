import { useState, useEffect, useLayoutEffect, useMemo, useRef, memo } from 'react'
import { createPortal } from 'react-dom'
import {
  useReactTable,
  getCoreRowModel,
  getSortedRowModel,
  getFilteredRowModel,
  flexRender,
  createColumnHelper,
  type SortingState,
  type ColumnSizingState,
} from '@tanstack/react-table'
import type { Row as TanStackRow } from '@tanstack/react-table'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { FileRecords } from '../bindings/FileRecords'
import type { CreateRecordDraft } from '../bindings/CreateRecordDraft'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import type { EditorRecordGroup } from '../bindings/EditorRecordGroup'
import {
  coordinateId,
  cellDeclaredType,
  cellEnumType,
  cellNullable,
  cellReadOnly,
  cellRefTargetType,
  diagnosticMatchesCoordinate,
  diagnosticSeverity,
  errorMessage,
  fieldPathField,
  nullValue,
  recordActualType,
  recordKey,
  sameCoordinate,
  type DiagnosticItem,
  type FieldPathSegment,
  type FieldValue,
} from '../wire'
import { DataCardCompact, EnumDirectSelect, RefDirectSelect } from './DataCard'
import {
  parseFieldValueText,
  recordMatchesSearch,
  summaryOf as valueSummary,
} from '../value/fieldValue'
import { CreateRecordDialog } from './CreateRecordDialog'
import { TransferRecordDialog, type RecordTransferTarget } from './TransferRecordDialog'
import { DiagBadge } from './DiagBadge'
import { Icon } from './Icon'
import {
  recordSelection,
  recordSelectionCoordinates,
  selectionMatchesRecord,
  selectionMatchesValue,
  type EditorSelection,
  type RecordSelectionMode,
} from '../state/editorSelection'
import {
  moveTableSelection,
  type TableDirection,
} from '../state/tableCellNavigation'
import { selectionEditIntentForKey } from '../state/selectionKeyboard'
import { fieldTypeColor } from '../utils/typeColor'
import {
  organizeRecordRows,
  type RecordGroupView,
} from '../state/manualRecordGroups'
import { useRecordPointerDrag } from '../hooks/useRecordPointerDrag'
import { RecordGroupHeader, RecordUngroupedHeader, recordGroupColorStyle } from './RecordGroupHeader'
import { fitViewportPosition } from '../utils/floatingPosition'

interface Props {
  data: FileRecords
  activeType: string
  readOnly?: boolean
  diagnostics?: DiagnosticItem[]
  /** Pre-populate the search filter from the parent global search bar. */
  searchQuery?: string
  recordGroups?: readonly EditorRecordGroup[]
  collapsedGroupKeys?: ReadonlySet<string>
  onToggleGroup?: (groupKey: string) => void
  onDropRecordOntoRecord?: (sources: readonly RecordCoordinate[], target: RecordCoordinate) => void
  onCreateGroup?: (records: readonly RecordCoordinate[]) => void
  onDropRecordIntoGroup?: (sources: readonly RecordCoordinate[], groupId: string) => void
  onDropRecordIntoUngrouped?: (sources: readonly RecordCoordinate[]) => void
  onRenameGroup?: (groupId: string, name: string) => void
  onColorGroup?: (groupId: string, color: string | null) => void
  /** Current record/value selection, lifted so it can drive the inspector. */
  selection?: EditorSelection | null
  /** Click on the Key cell: select the record. */
  onSelectRecord?: (
    coordinate: RecordCoordinate,
    mode: RecordSelectionMode,
    visibleCoordinates: readonly RecordCoordinate[],
  ) => void
  /** Click on a field cell: select that value. */
  onSelectValue?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[]) => void
  onRenderCellText?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[]) => Promise<string>
  onParseCellText?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], text: string) => Promise<FieldValue>
  /** Click on blank space inside the table view: deselect / close inspector. */
  onClearSelection?: () => void
  /** Dbl-click / context-menu jump to the dedicated record view. */
  onOpenRecord: (coordinate: RecordCoordinate) => void
  onWriteField?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], newValue: FieldValue) => Promise<RecordRow | void>
  onRenameRecord?: (coordinate: RecordCoordinate, newKey: string) => Promise<RecordRow | void>
  /** Create a new record. Resolves once the back-end has persisted and the
   *  parent has refreshed `data` for this file. */
  onInsertRecord?: (recordKey: string, actualType: string, fields: FieldValue) => Promise<void>
  onCreateRecordDraft?: (actualType: string) => Promise<CreateRecordDraft>
  /** Delete an existing record by key. */
  onDeleteRecord?: (coordinate: RecordCoordinate) => Promise<void>
  onSwapRecords?: (first: RecordCoordinate, second: RecordCoordinate) => Promise<void>
  onMoveRecord?: (coordinate: RecordCoordinate, targetIndex: number) => Promise<void>
  transferTargets?: RecordTransferTarget[]
  onTransferRecord?: (
    coordinate: RecordCoordinate,
    destinationFile: string,
    targetIndex: number,
  ) => Promise<void>
  /** Click on a corner badge on a row or cell. `fieldPath` is null for
   *  record-level (the Key column badge), otherwise the column name. */
  onDiagnosticBadgeClick?: (coordinate: RecordCoordinate, fieldPath: string | null) => void
  columnWidths?: ColumnSizingState
  onColumnWidthsChange?: (widths: ColumnSizingState) => void
  onEnterInspector?: () => void
  focusRequest?: number
  firstRecordFocusRequest?: number
  onFirstRecordFocusConsumed?: (request: number) => void
  onNavigationBoundary?: (direction: TableDirection) => void
}

const ROW_H = 30
const GROUP_ROW_H = 32
const MIN_COLUMN_WIDTH = 48

type TableDisplayItem =
  | { kind: 'group'; view: RecordGroupView }
  | { kind: 'ungrouped'; records: RecordRow[] }
  | { kind: 'row'; row: TanStackRow<RecordRow>; group?: EditorRecordGroup }

interface TableContextMenu {
  anchorX: number
  anchorY: number
  x: number
  y: number
  row: RecordRow
  records: RecordCoordinate[]
  showGroupTargets: boolean
}

export const TableView = memo(function TableView({ data, activeType, readOnly, diagnostics, searchQuery, recordGroups, collapsedGroupKeys, onToggleGroup, onDropRecordOntoRecord, onCreateGroup, onDropRecordIntoGroup, onDropRecordIntoUngrouped, onRenameGroup, onColorGroup, selection, onSelectRecord, onSelectValue, onRenderCellText, onParseCellText, onClearSelection, onOpenRecord, onWriteField, onRenameRecord, onInsertRecord, onCreateRecordDraft, onDeleteRecord, onSwapRecords, onMoveRecord, transferTargets = [], onTransferRecord, onDiagnosticBadgeClick, columnWidths, onColumnWidthsChange, onEnterInspector, focusRequest, firstRecordFocusRequest, onFirstRecordFocusConsumed, onNavigationBoundary }: Props) {
  const [contextMenu, setContextMenu] = useState<TableContextMenu | null>(null)
  const [showNewRecord, setShowNewRecord] = useState(false)
  const [transferRow, setTransferRow] = useState<RecordRow | null>(null)
  const [syntaxEdit, setSyntaxEdit] = useState<{ key: string; initialText: string } | null>(null)
  const [cellNotice, setCellNotice] = useState<string | null>(null)
  const [sorting, setSorting] = useState<SortingState>([])
  const [columnSizing, setColumnSizing] = useState<ColumnSizingState>(() => columnWidths ?? {})
  const [globalFilter, setGlobalFilter] = useState(searchQuery ?? '')
  const [tableZoom, setTableZoom] = useState(1)

  const tableScrollRef = useRef<HTMLDivElement>(null)
  const contextMenuRef = useRef<HTMLDivElement>(null)
  const columnSizingRef = useRef(columnSizing)
  const columnResizeRef = useRef<{
    pointerId: number
    columnId: string
    startX: number
    startWidth: number
  } | null>(null)
  columnSizingRef.current = columnSizing

  const updateColumnResize = (pointerId: number, clientX: number) => {
    const resize = columnResizeRef.current
    if (!resize || resize.pointerId !== pointerId) return
    const width = Math.max(MIN_COLUMN_WIDTH, resize.startWidth + clientX - resize.startX)
    const next = { ...columnSizingRef.current, [resize.columnId]: width }
    columnSizingRef.current = next
    setColumnSizing(next)
  }

  const finishColumnResize = (pointerId: number) => {
    if (columnResizeRef.current?.pointerId !== pointerId) return
    columnResizeRef.current = null
    onColumnWidthsChange?.(columnSizingRef.current)
  }

  // Reset transient UI state when active file/type changes.
  useEffect(() => {
    setSorting([])
    setGlobalFilter('')
    const next = columnWidths ?? {}
    columnSizingRef.current = next
    setColumnSizing(next)
  }, [data.file_path, activeType, columnWidths])

  // Sync search from parent global search bar.
  useEffect(() => {
    setGlobalFilter(searchQuery ?? '')
  }, [searchQuery])

  const filtered = useMemo(
    () => data.records.filter(r => recordActualType(r) === activeType),
    [data.records, activeType]
  )

  // Build a (recordKey, topLevelFieldName) → severity index for this file so
  // table cells can light up red/yellow without recomputing on every render.
  const cellDiagIndex = useMemo(() => {
    const m = new Map<string, 'error' | 'warning' | 'info'>()
    if (!diagnostics) return m
    for (const d of diagnostics) {
      if (d.file_path !== data.file_path || !d.record_key) continue
      // Take the first path segment as the column we'll mark.
      const top = d.field_path
        ? d.field_path.split(/[.[]/, 1)[0]
        : null
      const coordinates = d.actual_type === null
        ? data.records
            .filter(r => r.coordinate.key === d.record_key)
            .map(r => r.coordinate)
        : [{ actual_type: d.actual_type, key: d.record_key }]
      const rank = (s: 'error' | 'warning' | 'info') => s === 'error' ? 3 : s === 'warning' ? 2 : 1
      const severity = diagnosticSeverity(d.severity)
      for (const coordinate of coordinates) {
        const coordKey = coordinateId(coordinate)
        const key = top ? `${coordKey}::${top}` : `${coordKey}::*`
        const cur = m.get(key)
        if (!cur || rank(severity) > rank(cur)) m.set(key, severity)
      }
    }
    return m
  }, [diagnostics, data.file_path, data.records])
  const recordSeverity = (coordinate: RecordCoordinate): 'error' | 'warning' | null =>
    severityForCoordinate(diagnostics, data.file_path, coordinate)

  const allFieldNames = useMemo(
    () => data.columns
      .filter(column => column.type_names.includes(activeType))
      .map(column => column.name),
    [data.columns, activeType]
  )

  // Ref to the latest `data` so structural memos below (pill columns,
  // declared-type map, column widths) can read the current annotations
  // without listing `data.records` in their deps — that would rebuild the
  // memo on every edit and cascade into a react-table column-defs replay.
  const dataForCellsRef = useRef(data)
  dataForCellsRef.current = data

  // Which columns render as pill cells (ref/enum). Freezing this once per
  // column set means the `pill-cell` class on the td stays stable across
  // writes; without it, a briefly-mounted wrapper span (e.g. diagnostics)
  // would flip a :has() rule and shift the whole column's padding.
  const pillColumns = useMemo(() => {
    const set = new Set<string>()
    for (const column of data.columns) {
      if (!column.type_names.includes(activeType)) continue
      if (columnDropdownKind(data, column.name, activeType) !== null) {
        set.add(column.name)
      }
    }
    return set
  }, [data.file_path, activeType, columnKeySignature(data)])

  // Declared schema type per column, sampled from the first record that
  // carries an annotation for this field. Different records normally
  // agree on the declared type for a shared field name; if they don't
  // (e.g. a rename mid-migration) we show whatever we saw first.
  // Depending on `data.records` here would rebuild the map — and via the
  // columns memo, every columnDef — on every edit, causing react-table to
  // replay column sizes and briefly flash the row layout. Freezing on the
  // structural signature (file + active type + column set) keeps the
  // memo stable across writes.
  const columnDeclaredTypes = useMemo(() => {
    const map: Record<string, string> = {}
    const snapshot = dataForCellsRef.current
    for (const name of allFieldNames) {
      for (const record of snapshot.records) {
        if (recordActualType(record) !== activeType) continue
        const cell = fieldCell(record, name)
        const declared = cell?.annotation?.declared_type ?? inferredCellType(cell)
        if (declared) { map[name] = declared; break }
      }
    }
    return map
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data.file_path, activeType, columnKeySignature(data)])

  // Compute column widths once per (file, activeType, column set) and freeze
  // them for the session. Recomputing on every `data.records` update meant a
  // ref-cell edit could change the column's max-summary length by a few px
  // and snap the whole column to a new width — visible as jitter. Editing a
  // value shouldn't reflow the layout; the user can drag-resize if a newly
  // added cell needs more room.
  const columnSizeHints = useMemo(() => {
    const snapshot = dataForCellsRef.current
    const measure = (text: string) => estimateTextWidth(text, false)
    const measureMono = (text: string) => estimateTextWidth(text, true)
    // Chrome around the content per cell type.
    const PILL_CHROME = 46
    const REF_PREFIX = 14
    const PLAIN_CHROME = 24
    const BADGE_ROOM = 16 // reserve space for the corner diag badge
    const MIN = 90
    // Only value columns are capped, so a rogue 5000-char string doesn't
    // stretch its column across the whole viewport. The user can still drag
    // wider. Key column is never capped — keys must render in full.
    const VALUE_MAX = 600
    // Key column: measure the longest key so it never truncates unless the
    // user shrinks it manually. No cap.
    let keyWidth = measure('Key') + PLAIN_CHROME + 12 /* sort caret */
    for (const record of snapshot.records) {
      if (recordActualType(record) !== activeType) continue
      keyWidth = Math.max(keyWidth, measureMono(recordKey(record)) + PLAIN_CHROME + BADGE_ROOM)
    }
    const KEY_MAX = 500
    const hints: Record<string, number> = { key: Math.min(KEY_MAX, Math.max(MIN, Math.ceil(keyWidth))) }
    for (const column of snapshot.columns) {
      if (!column.type_names.includes(activeType)) continue
      const kind = columnDropdownKind(snapshot, column.name, activeType)
      const isPill = kind !== null
      const chrome = (isPill ? PILL_CHROME + (kind === 'ref' ? REF_PREFIX : 0) : PLAIN_CHROME) + BADGE_ROOM
      let maxContent = 0
      let hasComplexValue = false
      let declaredForHeader: string | undefined
      for (const record of snapshot.records) {
        if (recordActualType(record) !== activeType) continue
        const cell = fieldCell(record, column.name)
        if (!cell) continue
        if (isComplexValue(cell.value)) hasComplexValue = true
        const w = measure(valueSummary(cell.value))
        if (w > maxContent) maxContent = w
        if (!declaredForHeader) declaredForHeader = cell.annotation?.declared_type ?? undefined
      }
      const summaryWidth = maxContent + chrome
      const typeChipWidth = declaredForHeader ? measureMono(declaredForHeader) + 16 : 0
      const headerWidth = measure(column.name) + PLAIN_CHROME + 12 /* sort caret */ + typeChipWidth
      const minimumWidth = hasComplexValue ? 300 : MIN
      hints[column.name] = Math.min(VALUE_MAX, Math.max(minimumWidth, Math.ceil(Math.max(summaryWidth, headerWidth))))
    }
    return hints
    // Deps intentionally stable: file, active type, column identity/count,
    // and record count. Content edits don't move the layout.
  }, [data.file_path, activeType, columnKeySignature(data), data.records.length])

  const canEdit = !readOnly && !!onWriteField
  const canRename = !readOnly && data.capabilities.can_edit_key && !!onRenameRecord

  // Cell renderers read these through refs so writes (which change
  // diagnostics + record identity every time) don't rebuild the `columns`
  // memo. A rebuilt memo → new columnDef identities → react-table replays
  // its column-size default from the memo's `size`, and if the user hasn't
  // dragged the column that briefly re-snaps to the new hint, showing up
  // as jitter when the ref column's summary width just changed.
  const diagnosticsRef = useRef(diagnostics)
  diagnosticsRef.current = diagnostics
  const cellDiagIndexRef = useRef(cellDiagIndex)
  cellDiagIndexRef.current = cellDiagIndex
  const onWriteFieldRef = useRef(onWriteField)
  onWriteFieldRef.current = onWriteField
  const onRenameRecordRef = useRef(onRenameRecord)
  onRenameRecordRef.current = onRenameRecord
  const onDiagnosticBadgeClickRef = useRef(onDiagnosticBadgeClick)
  onDiagnosticBadgeClickRef.current = onDiagnosticBadgeClick
  const syntaxEditRef = useRef(syntaxEdit)
  syntaxEditRef.current = syntaxEdit
  const onParseCellTextRef = useRef(onParseCellText)
  onParseCellTextRef.current = onParseCellText
  const columns = useMemo(() => {
    const helper = createColumnHelper<RecordRow>()
    return [
      helper.accessor(row => recordKey(row), {
        id: 'key',
        header: () => (
          <span className="th-label">
            <span className="th-label-name">Key</span>
            <span className="th-label-type th-label-type-key">key</span>
          </span>
        ),
        cell: info => {
          const filePath = dataForCellsRef.current.file_path
          const rowSev = severityForCoordinate(diagnosticsRef.current, filePath, info.row.original.coordinate)
          const badgeClick = onDiagnosticBadgeClickRef.current
          const renameFn = onRenameRecordRef.current
          return (
            <span className={`cell-key-wrap${rowSev ? ' has-diag' : ''}`}>
              <Icon name="grip" size={13} className="record-drag-handle" aria-hidden />
              <EditableKeyCell
                value={info.getValue()}
                editable={canRename}
                onCommit={canRename && renameFn ? next => renameFn(info.row.original.coordinate, next) : undefined}
              />
              {(rowSev === 'error' || rowSev === 'warning') && (
                <DiagBadge
                  severity={rowSev}
                  onClick={badgeClick
                    ? () => badgeClick(info.row.original.coordinate, null)
                    : undefined}
                />
              )}
            </span>
          )
        },
        size: columnWidths?.key ?? columnSizeHints.key ?? 140,
        sortDescFirst: false,
      }),
      ...allFieldNames.map(name => {
        const declared = columnDeclaredTypes[name]
        return helper.display({
          id: name,
          header: () => (
            <span
              className="th-label"
              style={{ '--field-color': fieldTypeColor(declared ?? name) } as React.CSSProperties}
            >
              <span className="th-label-name">{name}</span>
              {declared && (
                <span className="th-label-type" title={`类型：${declared}`}>{declared}</span>
              )}
            </span>
          ),
          size: columnWidths?.[name] ?? columnSizeHints[name] ?? 120,
          enableSorting: false,
          cell: ({ row }) => {
            const filePath = dataForCellsRef.current.file_path
            const f = fieldCell(row.original, name)
            const sev = cellDiagIndexRef.current.get(`${coordinateId(row.original.coordinate)}::${name}`)
            const badgeClick = onDiagnosticBadgeClickRef.current
            const writeFn = onWriteFieldRef.current
            const cellBadge = (sev === 'error' || sev === 'warning') ? (
              <DiagBadge
                severity={sev}
                onClick={badgeClick
                  ? () => badgeClick(row.original.coordinate, name)
                  : undefined}
              />
            ) : null
            if (!f) {
              return (
                <span className={`dc-null-wrap${sev ? ' dc-cell-diag dc-cell-diag-' + sev : ''}`}>
                  <span className="dc-null">—</span>
                  {cellBadge}
                </span>
              )
            }
            const editKey = tableCellKey(row.original.coordinate, name)
            const syntaxRequest = syntaxEditRef.current?.key === editKey
              ? syntaxEditRef.current
              : null
            if (syntaxRequest) {
              return (
                <CellSyntaxEditor
                  initialText={syntaxRequest.initialText}
                  onCancel={() => setSyntaxEdit(current => current?.key === editKey ? null : current)}
                  onCommit={async text => {
                    const parse = onParseCellTextRef.current
                    const write = onWriteFieldRef.current
                    if (!parse || !write) return
                    try {
                      const path = [fieldPathField(name)]
                      const next = await parse(row.original.coordinate, path, text)
                      await write(row.original.coordinate, path, next)
                      setSyntaxEdit(current => current?.key === editKey ? null : current)
                      setCellNotice(null)
                      requestAnimationFrame(() => tableScrollRef.current?.focus({ preventScroll: true }))
                    } catch (error) {
                      setCellNotice(`输入格式不正确：${errorMessage(error)}`)
                      setSyntaxEdit(current => current?.key === editKey ? null : current)
                      requestAnimationFrame(() => tableScrollRef.current?.focus({ preventScroll: true }))
                    }
                  }}
                />
              )
            }
            const readOnlyFromSchema = cellReadOnly(f)
            const cellEditable = canEdit && !readOnlyFromSchema
            const title = readOnlyFromSchema
              ? '由源记录决定，不可编辑'
              : sev ? findDiagMessage(diagnosticsRef.current, filePath, row.original.coordinate, name) : undefined
            return (
              <span className={sev ? `dc-cell-diag dc-cell-diag-${sev}` : undefined} title={title}>
                <EditableCell
                  value={f.value}
                  label={name}
                  editable={cellEditable}
                  refTargetType={cellRefTargetType(f)}
                  enumType={cellEnumType(f)}
                  nullable={cellNullable(f)}
                  declaredType={cellDeclaredType(f)}
                  onCommit={cellEditable && writeFn ? next => writeFn(row.original.coordinate, [fieldPathField(name)], next) : undefined}
                  onEditingFinished={() => tableScrollRef.current?.focus({ preventScroll: true })}
                />
                {cellBadge}
              </span>
            )
          },
        })
      }),
    ]
    // Only structural changes (column set, active type, computed widths,
    // permission flags) rebuild the column defs. Edit-time state
    // (diagnostics, records, callbacks) is read via refs above.
  }, [allFieldNames, columnSizeHints, columnDeclaredTypes, columnWidths, canEdit, canRename])

  // Global filter: match key or any scalar field value (via summaryOf).
  const globalFilterFn = useMemo(
    () => (row: { original: RecordRow }, _columnId: string, filterValue: string) => {
      return recordMatchesSearch(row.original, filterValue)
    },
  [],
  )

  const table = useReactTable({
    data: filtered,
    columns,
    defaultColumn: {
      minSize: MIN_COLUMN_WIDTH,
      maxSize: Number.MAX_SAFE_INTEGER,
    },
    state: { sorting, columnSizing, globalFilter },
    onSortingChange: setSorting,
    onColumnSizingChange: updater => {
      const next = typeof updater === 'function' ? updater(columnSizingRef.current) : updater
      columnSizingRef.current = next
      setColumnSizing(next)
    },
    onGlobalFilterChange: setGlobalFilter,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    getRowId: row => coordinateId(row.coordinate),
    globalFilterFn,
    enableSortingRemoval: true,
    enableMultiSort: false,
    enableColumnResizing: true,
  })

  const rows = table.getRowModel().rows
  const displayItems = useMemo<TableDisplayItem[]>(() => {
    const rowsById = new Map(rows.map(row => [coordinateId(row.original.coordinate), row]))
    const organized = organizeRecordRows(rows.map(row => row.original), recordGroups ?? [])
    if (organized.groups.length === 0) return rows.map(row => ({ kind: 'row', row }))

    const items: TableDisplayItem[] = []
    for (const view of organized.groups) {
      items.push({ kind: 'group', view })
      if (!collapsedGroupKeys?.has(view.group.id)) {
        items.push(...view.records.map(record => ({
          kind: 'row' as const,
          row: rowsById.get(coordinateId(record.coordinate))!,
          group: view.group,
        })))
      }
    }
    items.push({ kind: 'ungrouped', records: organized.ungrouped })
    items.push(...organized.ungrouped.map(record => ({
        kind: 'row' as const,
        row: rowsById.get(coordinateId(record.coordinate))!,
    })))
    return items
  }, [rows, recordGroups, collapsedGroupKeys])
  const visibleRows = useMemo(
    () => displayItems.flatMap(item => item.kind === 'row' ? [item.row] : []),
    [displayItems],
  )
  const visibleCoordinates = useMemo(
    () => visibleRows.map(row => row.original.coordinate),
    [visibleRows],
  )
  const recordPointerDrag = useRecordPointerDrag({
    rootRef: tableScrollRef,
    records: data.records,
    selectedCoordinates: selection?.filePath === data.file_path
      ? recordSelectionCoordinates(selection)
      : [],
    onSelectDragSource: coordinate => onSelectRecord?.(coordinate, 'replace', visibleCoordinates),
    onDropRecordOntoRecord,
    onDropRecordIntoGroup,
    onDropRecordIntoUngrouped,
  })
  const rowVirtualizer = useVirtualizer({
    count: displayItems.length,
    getScrollElement: () => tableScrollRef.current,
    estimateSize: index => (displayItems[index]?.kind === 'row' ? ROW_H : GROUP_ROW_H) * tableZoom,
    getItemKey: index => {
      const item = displayItems[index]
      if (item?.kind === 'group') return `group:${item.view.group.id}`
      if (item?.kind === 'ungrouped') return 'group:ungrouped'
      return item?.row.id ?? index
    },
    overscan: 12,
  })
  useEffect(() => rowVirtualizer.measure(), [rowVirtualizer, tableZoom])
  const virtualRows = rowVirtualizer.getVirtualItems()
  const totalHeight = rowVirtualizer.getTotalSize()
  const padBefore = virtualRows.length > 0 ? virtualRows[0].start : 0
  const padAfter = virtualRows.length > 0 ? totalHeight - virtualRows[virtualRows.length - 1].end : 0

  const revealTableSelection = (target: EditorSelection) => {
    if (target.filePath !== data.file_path) return
    const rowIndex = displayItems.findIndex(item => (
      item.kind === 'row' && sameCoordinate(item.row.original.coordinate, target.coordinate)
    ))
    if (rowIndex < 0) return
    rowVirtualizer.scrollToIndex(rowIndex, { align: 'auto' })
    const columnId = target.kind === 'record'
      ? 'key'
      : selectedTopLevelField(target.fieldPath)
    if (!columnId) return
    const key = tableCellKey(target.coordinate, columnId)
    let attempts = 0
    const reveal = () => {
      const scroller = tableScrollRef.current
      const cell = scroller?.querySelector<HTMLElement>(
        `[data-table-cell-key="${CSS.escape(key)}"]`,
      )
      if (!cell && attempts < 4) {
        attempts += 1
        requestAnimationFrame(reveal)
        return
      }
      if (!cell || !scroller) return
      // Vertical: fall back to browser nearest logic.
      cell.scrollIntoView({ block: 'nearest', inline: 'nearest' })
      if (columnId === 'key') return
      // Horizontal: ensure the WHOLE column is visible, respecting the sticky
      // Key column that overlays the left edge. Only scroll if either edge is
      // clipped; center if the cell is wider than the visible area.
      const scrollerRect = scroller.getBoundingClientRect()
      const cellRect = cell.getBoundingClientRect()
      const keyCol = scroller.querySelector<HTMLElement>('thead .sticky-key-column')
      const leftOccluded = keyCol ? keyCol.getBoundingClientRect().right : scrollerRect.left
      const visibleLeft = Math.max(scrollerRect.left, leftOccluded)
      const visibleRight = scrollerRect.right
      if (cellRect.right > visibleRight) {
        scroller.scrollLeft += cellRect.right - visibleRight + 4
      } else if (cellRect.left < visibleLeft) {
        scroller.scrollLeft -= visibleLeft - cellRect.left + 4
      }
    }
    reveal()
  }

  useEffect(() => {
    if (!focusRequest || !selection) return
    tableScrollRef.current?.focus({ preventScroll: true })
    revealTableSelection(selection)
  }, [focusRequest])

  // Any selection change (click, keyboard nav, inspector-driven) should
  // reveal the selected cell so its full column stays visible under the
  // sticky Key column.
  const selectionKeyForReveal = selection
    ? `${selection.filePath}::${coordinateId(selection.coordinate)}::${selection.kind === 'value' ? JSON.stringify(selection.fieldPath) : '__record__'}`
    : null
  useEffect(() => {
    if (!selection) return
    revealTableSelection(selection)
  }, [selectionKeyForReveal])

  useEffect(() => {
    if (!firstRecordFocusRequest) return
    tableScrollRef.current?.focus({ preventScroll: true })
    const first = visibleRows[0]?.original.coordinate
    if (first) {
      onSelectRecord?.(first, 'replace', visibleCoordinates)
      revealTableSelection(recordSelection(data.file_path, first))
    }
    onFirstRecordFocusConsumed?.(firstRecordFocusRequest)
  }, [firstRecordFocusRequest])

  useEffect(() => {
    if (selection) revealTableSelection(selection)
  // Row/column identity and selection are the only inputs that can move the target.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selection, displayItems.length, allFieldNames.join('\u001f')])

  // Close context menu on Escape.
  useEffect(() => {
    if (!contextMenu) return
    const h = (e: KeyboardEvent) => { if (e.key === 'Escape') setContextMenu(null) }
    window.addEventListener('keydown', h)
    return () => window.removeEventListener('keydown', h)
  }, [contextMenu])

  const contextMenuCanAddToGroup = contextMenu !== null && (
    ((recordGroups?.length ?? 0) > 0 && !!onDropRecordIntoGroup)
    || (contextMenu.records.length > 1 && !!onCreateGroup)
  )

  useLayoutEffect(() => {
    if (!contextMenu) return
    const fitMenu = () => {
      const menu = contextMenuRef.current
      if (!menu) return
      const rect = menu.getBoundingClientRect()
      const next = fitViewportPosition(
        { x: contextMenu.anchorX, y: contextMenu.anchorY },
        { width: rect.width, height: rect.height },
        { width: window.innerWidth, height: window.innerHeight },
      )
      setContextMenu(current => {
        if (!current || current.anchorX !== contextMenu.anchorX || current.anchorY !== contextMenu.anchorY) {
          return current
        }
        return current.x === next.x && current.y === next.y
          ? current
          : { ...current, x: next.x, y: next.y }
      })
    }
    fitMenu()
    window.addEventListener('resize', fitMenu)
    return () => window.removeEventListener('resize', fitMenu)
  }, [
    contextMenu?.anchorX,
    contextMenu?.anchorY,
    contextMenu?.showGroupTargets,
    contextMenuCanAddToGroup,
    recordGroups?.length,
  ])

  return (
    <div
      className="table-view"
      onClick={e => {
        setContextMenu(null)
        // Clicks that didn't land on a row deselect the current row, which
        // also closes the shared right-side inspector.
        const target = e.target as HTMLElement
        if (!target.closest('.table-row')
          && !target.closest('.record-group-header')
          && !target.closest('.record-ungrouped-header')
          && !target.closest('.context-menu')) {
          onClearSelection?.()
        }
      }}
    >
      <div className="table-main">
        <div
          className="table-scroll"
          ref={tableScrollRef}
          onWheel={event => {
            if (!event.ctrlKey) return
            event.preventDefault()
            setTableZoom(current => Math.max(0.7, Math.min(1.6,
              Math.round((current + (event.deltaY < 0 ? 0.1 : -0.1)) * 10) / 10,
            )))
          }}
          tabIndex={0}
          onPointerDown={recordPointerDrag.onPointerDown}
          onClickCapture={recordPointerDrag.onClickCapture}
          onFocus={e => {
            if (e.target !== e.currentTarget || selection || visibleRows.length === 0) return
            onSelectRecord?.(visibleRows[0].original.coordinate, 'replace', visibleCoordinates)
          }}
          onKeyDown={async e => {
            if (isNativeEditingTarget(e.target)) return
            if (!selection || selection.filePath !== data.file_path) return

            if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a' && visibleCoordinates.length > 0) {
              e.preventDefault()
              const first = visibleCoordinates[0]
              const last = visibleCoordinates[visibleCoordinates.length - 1]
              onSelectRecord?.(first, 'replace', visibleCoordinates)
              onSelectRecord?.(last, 'range', visibleCoordinates)
              return
            }
            if (e.key === 'ArrowLeft' || e.key === 'ArrowRight' || e.key === 'ArrowUp' || e.key === 'ArrowDown') {
              e.preventDefault()
              if (
                selection.kind === 'record'
                && e.shiftKey
                && (e.key === 'ArrowUp' || e.key === 'ArrowDown')
              ) {
                const currentIndex = visibleCoordinates.findIndex(item => sameCoordinate(item, selection.coordinate))
                const nextIndex = Math.max(0, Math.min(
                  visibleCoordinates.length - 1,
                  currentIndex + (e.key === 'ArrowDown' ? 1 : -1),
                ))
                const nextCoordinate = visibleCoordinates[nextIndex]
                if (currentIndex >= 0 && nextCoordinate) {
                  onSelectRecord?.(nextCoordinate, 'range', visibleCoordinates)
                  revealTableSelection(recordSelection(data.file_path, nextCoordinate))
                }
                return
              }
              const next = moveTableSelection(
                selection,
                e.key as TableDirection,
                visibleCoordinates,
                allFieldNames,
              )
              if (next !== selection) {
                setSyntaxEdit(null)
                if (next.kind === 'record') onSelectRecord?.(next.coordinate, 'replace', visibleCoordinates)
                else onSelectValue?.(next.coordinate, next.fieldPath)
                revealTableSelection(next)
              } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp' || e.key === 'ArrowRight') {
                onNavigationBoundary?.(e.key)
              }
              return
            }

            if (e.key === 'Enter' && onEnterInspector) {
              if (selection.kind === 'value') {
                const field = selectedTopLevelField(selection.fieldPath)
                const selectedRow = visibleRows.find(row => coordinateId(row.original.coordinate) === coordinateId(selection.coordinate))
                const selectedCell = field && selectedRow ? fieldCell(selectedRow.original, field) : undefined
                if (selectedCell?.value.kind === 'bool' && canEdit && !cellReadOnly(selectedCell)) {
                  e.preventDefault()
                  try {
                    await onWriteField?.(
                      selection.coordinate,
                      selection.fieldPath,
                      { kind: 'bool', value: !selectedCell.value.value },
                    )
                    setCellNotice(null)
                  } catch (error) {
                    setCellNotice(`无法编辑：${errorMessage(error)}`)
                  }
                  return
                }
              }
              e.preventDefault()
              onEnterInspector()
              return
            }

            if (selection.kind !== 'value') return
            const field = selectedTopLevelField(selection.fieldPath)
            if (!field) return
            const selectedRow = visibleRows.find(row => coordinateId(row.original.coordinate) === coordinateId(selection.coordinate))
            const selectedCell = selectedRow ? fieldCell(selectedRow.original, field) : undefined
            const editable = !!selectedCell && canEdit && !cellReadOnly(selectedCell)
            const directlyEditable = editable && !isComplexValue(selectedCell?.value)
            const modified = e.ctrlKey || e.metaKey || e.altKey
            const lower = e.key.toLowerCase()

            if (e.key === 'Enter') {
              const dropdown = tableScrollRef.current?.querySelector<HTMLInputElement>(
                `[data-table-cell-key="${CSS.escape(tableCellKey(selection.coordinate, field))}"] input.searchable-select`,
              )
              if (dropdown) {
                e.preventDefault()
                dropdown.focus({ preventScroll: true })
                return
              }
            }

            if ((e.ctrlKey || e.metaKey) && lower === 'c' && onRenderCellText) {
              e.preventDefault()
              try {
                const text = await onRenderCellText(selection.coordinate, selection.fieldPath)
                await navigator.clipboard.writeText(text)
                setCellNotice(null)
              } catch (error) {
                setCellNotice(`复制失败：${errorMessage(error)}`)
              }
              return
            }
            if ((e.ctrlKey || e.metaKey) && lower === 'v' && directlyEditable && onParseCellText && onWriteField) {
              e.preventDefault()
              try {
                const text = await navigator.clipboard.readText()
                const next = await onParseCellText(selection.coordinate, selection.fieldPath, text)
                await onWriteField(selection.coordinate, selection.fieldPath, next)
                setCellNotice(null)
              } catch (error) {
                setCellNotice(`粘贴格式不正确：${errorMessage(error)}`)
              }
              return
            }

            const intent = directlyEditable && selectedCell
              ? selectionEditIntentForKey(e.key, modified, selectedCell.value.kind)
              : null
            if (!intent) return
            e.preventDefault()
            if (intent.kind === 'clear' || intent.kind === 'toggle-bool') {
              try {
                const next = intent.kind === 'clear'
                  ? nullValue()
                  : { kind: 'bool' as const, value: !(selectedCell?.value.kind === 'bool' && selectedCell.value.value) }
                await onWriteField?.(selection.coordinate, selection.fieldPath, next)
                setCellNotice(null)
              } catch (error) {
                setCellNotice(`无法编辑：${errorMessage(error)}`)
              }
              return
            }
            try {
              const initialText = intent.kind === 'replace'
                ? intent.text
                : await onRenderCellText?.(selection.coordinate, selection.fieldPath)
              if (initialText !== undefined) {
                setSyntaxEdit({ key: tableCellKey(selection.coordinate, field), initialText })
                setCellNotice(null)
              }
            } catch (error) {
              setCellNotice(`无法编辑：${errorMessage(error)}`)
            }
          }}
        >
          <table className="data-table" style={{ width: table.getTotalSize(), zoom: tableZoom }}>
            <thead>
              {table.getHeaderGroups().map(hg => (
                <tr key={hg.id}>
                  {hg.headers.map(h => {
                    const sort = h.column.getIsSorted()
                    return (
                      <th
                        key={h.id}
                        className={h.column.id === 'key' ? 'sticky-key-column' : undefined}
                        style={{ width: h.getSize() }}
                        aria-sort={sort === 'asc' ? 'ascending' : sort === 'desc' ? 'descending' : 'none'}
                      >
                        <button
                          type="button"
                          className="th-sort-btn"
                          onClick={h.column.getToggleSortingHandler()}
                          disabled={!h.column.getCanSort()}
                          title={h.column.getCanSort()
                            ? sort === 'asc'
                              ? '当前主键升序；点击切换为降序'
                              : sort === 'desc'
                                ? '当前主键降序；点击取消排序'
                                : '点击按主键升序'
                            : undefined}
                        >
                          {flexRender(h.column.columnDef.header, h.getContext())}
                          {sort === 'asc' && <Icon name="chevron-up" size={11} className="th-sort-icon asc" aria-hidden />}
                          {sort === 'desc' && <Icon name="chevron-down" size={11} className="th-sort-icon desc" aria-hidden />}
                        </button>
                        {h.column.getCanResize() && (
                          <div
                            className="th-resizer"
                            onPointerDown={event => {
                              if (event.button !== 0) return
                              event.preventDefault()
                              event.currentTarget.setPointerCapture(event.pointerId)
                              columnResizeRef.current = {
                                pointerId: event.pointerId,
                                columnId: h.column.id,
                                startX: event.clientX,
                                startWidth: h.getSize(),
                              }
                            }}
                            onPointerMove={event => {
                              if (columnResizeRef.current?.pointerId !== event.pointerId) return
                              if (event.buttons === 0) {
                                if (event.currentTarget.hasPointerCapture(event.pointerId)) {
                                  event.currentTarget.releasePointerCapture(event.pointerId)
                                }
                                finishColumnResize(event.pointerId)
                                return
                              }
                              updateColumnResize(event.pointerId, event.clientX)
                            }}
                            onPointerUp={event => {
                              updateColumnResize(event.pointerId, event.clientX)
                              if (event.currentTarget.hasPointerCapture(event.pointerId)) {
                                event.currentTarget.releasePointerCapture(event.pointerId)
                              }
                              finishColumnResize(event.pointerId)
                            }}
                            onPointerCancel={event => finishColumnResize(event.pointerId)}
                            onLostPointerCapture={event => finishColumnResize(event.pointerId)}
                            onClick={e => e.stopPropagation()}
                            aria-hidden
                          />
                        )}
                      </th>
                    )
                  })}
                </tr>
              ))}
            </thead>
            <tbody>
              {padBefore > 0 && (
                <tr style={{ height: padBefore }}>
                  <td colSpan={columns.length} aria-hidden />
                </tr>
              )}
              {virtualRows.map(vr => {
                const item = displayItems[vr.index]
                if (item.kind === 'group') {
                  const collapsed = collapsedGroupKeys?.has(item.view.group.id) ?? false
                  const containsSelection = item.view.records.some(record => (
                    selectionOwnsRow(selection, data.file_path, record.coordinate)
                  ))
                  return (
                    <tr key={`group:${item.view.group.id}`} className="table-group-row">
                      <td colSpan={columns.length}>
                        <RecordGroupHeader
                          name={item.view.group.name}
                          groupId={item.view.group.id}
                          count={item.view.records.length}
                          collapsed={collapsed}
                          color={item.view.group.color}
                          className={`table-record-group-header${containsSelection ? ' contains-selection' : ''}`}
                          onToggle={() => onToggleGroup?.(item.view.group.id)}
                          onRename={name => onRenameGroup?.(item.view.group.id, name)}
                          onColorChange={color => onColorGroup?.(item.view.group.id, color)}
                        />
                      </td>
                    </tr>
                  )
                }
                if (item.kind === 'ungrouped') {
                  const containsSelection = item.records.some(record => (
                    selectionMatchesRecord(selection ?? null, data.file_path, record.coordinate)
                  ))
                  return (
                    <tr key="group:ungrouped" className="table-group-row">
                      <td colSpan={columns.length}>
                        <RecordUngroupedHeader
                          count={item.records.length}
                          className={`table-record-group-header${containsSelection ? ' contains-selection' : ''}`}
                        />
                      </td>
                    </tr>
                  )
                }
                const row = item.row
                const rowSev = recordSeverity(row.original.coordinate)
                return (
                  <tr
                    key={row.id}
                    data-index={vr.index}
                    data-coordinate-id={coordinateId(row.original.coordinate)}
                    data-record-draggable="true"
                    data-record-drop-kind="record"
                    data-record-label={recordKey(row.original)}
                    ref={rowVirtualizer.measureElement}
                    className={`table-row${selectionMatchesRecord(selection ?? null, data.file_path, row.original.coordinate) ? ' selected' : ''}${item.group?.color ? ' has-group-color' : ''}${rowSev ? ' table-row-' + rowSev : ''}`}
                    data-contains-selection={selectionOwnsRow(selection, data.file_path, row.original.coordinate) || undefined}
                    style={recordGroupColorStyle(item.group?.color)}
                    onContextMenu={e => {
                      e.preventDefault()
                      const clickedIsSelected = selection?.kind === 'record'
                        && selection.filePath === data.file_path
                        && selection.coordinates.some(coordinate => sameCoordinate(
                          coordinate,
                          row.original.coordinate,
                        ))
                      const records = clickedIsSelected
                        ? [...selection.coordinates]
                        : [row.original.coordinate]
                      if (!clickedIsSelected) {
                        onSelectRecord?.(row.original.coordinate, 'replace', visibleCoordinates)
                      }
                      setContextMenu({
                        anchorX: e.clientX,
                        anchorY: e.clientY,
                        x: e.clientX,
                        y: e.clientY,
                        row: row.original,
                        records,
                        showGroupTargets: false,
                      })
                    }}
                  >
                    {row.getVisibleCells().map(cell => {
                      const fieldPath = cell.column.id === 'key'
                        ? null
                        : [fieldPathField(cell.column.id)]
                      const selected = fieldPath !== null && selectionMatchesValue(
                        selection ?? null,
                        data.file_path,
                        row.original.coordinate,
                        fieldPath,
                      )
                      const classes = [
                        pillColumns.has(cell.column.id) ? 'pill-cell' : '',
                        fieldPath && isComplexValue(fieldCell(row.original, cell.column.id)?.value) ? 'complex-cell' : '',
                        selected ? 'selected-cell' : '',
                      ].filter(Boolean).join(' ')
                      return (
                        <td
                          key={cell.id}
                          className={[classes, cell.column.id === 'key' ? 'sticky-key-column' : ''].filter(Boolean).join(' ') || undefined}
                          data-table-cell-key={tableCellKey(row.original.coordinate, cell.column.id)}
                          style={{ width: cell.column.getSize() }}
                          aria-selected={selected || undefined}
                          onMouseDown={e => {
                            if (e.button !== 0) return
                            // Runs before native selects open, so the inspector
                            // follows the cell even when its editor consumes click.
                            e.stopPropagation()
                            if (!isNativeEditingTarget(e.target)) {
                              tableScrollRef.current?.focus({ preventScroll: true })
                            }
                            if (fieldPath) onSelectValue?.(row.original.coordinate, fieldPath)
                            else {
                              const mode: RecordSelectionMode = e.shiftKey
                                ? 'range'
                                : (e.ctrlKey || e.metaKey ? 'toggle' : 'replace')
                              onSelectRecord?.(row.original.coordinate, mode, visibleCoordinates)
                            }
                          }}
                        >
                          {flexRender(cell.column.columnDef.cell, cell.getContext())}
                        </td>
                      )
                    })}
                  </tr>
                )
              })}
              {padAfter > 0 && (
                <tr style={{ height: padAfter }}>
                  <td colSpan={columns.length} aria-hidden />
                </tr>
              )}
            </tbody>
          </table>
          {filtered.length === 0 && (
            <div className="empty-hint">暂无 {activeType} 类型的记录</div>
          )}
          {filtered.length > 0 && rows.length === 0 && (
            <div className="empty-hint">无匹配 "{globalFilter}" 的记录</div>
          )}
        </div>

        <div className="table-footer">
          {readOnly ? (
            <span className="table-footer-readonly">该文件为只读</span>
          ) : !data.capabilities.can_insert_record || !onInsertRecord || !onCreateRecordDraft ? (
            <span className="table-footer-readonly">该来源不支持新建记录</span>
          ) : (
            <button className="btn btn-outlined" onClick={() => setShowNewRecord(true)}>
              <Icon name="plus" size={13} />
              新建记录
            </button>
          )}
          {cellNotice && <span className="table-cell-notice" role="status">{cellNotice}</span>}
        </div>
      </div>

      {showNewRecord && onInsertRecord && onCreateRecordDraft && (
        <CreateRecordDialog
          actualType={activeType || data.type_names[0] || ''}
          existingKeys={data.records.map(r => r.coordinate.key)}
          onCreateRecordDraft={onCreateRecordDraft}
          onInsertRecord={onInsertRecord}
          onClose={() => setShowNewRecord(false)}
        />
      )}

      {transferRow && onTransferRecord && (
        <TransferRecordDialog
          recordKey={transferRow.coordinate.key}
          targets={transferTargets}
          onConfirm={(destinationFile, targetIndex) => (
            onTransferRecord(transferRow.coordinate, destinationFile, targetIndex)
          )}
          onClose={() => setTransferRow(null)}
        />
      )}

      {contextMenu && createPortal(
        <div
          ref={contextMenuRef}
          className="context-menu table-context-menu"
          style={{ left: contextMenu.x, top: contextMenu.y }}
          onClick={e => e.stopPropagation()}
          role="menu"
        >
          <div className="ctx-item" role="menuitem" onClick={() => { onOpenRecord(contextMenu.row.coordinate); setContextMenu(null) }}>
            <Icon name="record" size={13} aria-hidden />
            跳转到记录视图
          </div>
          {contextMenuCanAddToGroup && (<>
            <div className="ctx-sep" />
            <button
              type="button"
              className="ctx-item"
              role="menuitem"
              aria-expanded={contextMenu.showGroupTargets}
              onClick={() => setContextMenu(current => current
                ? { ...current, showGroupTargets: !current.showGroupTargets }
                : null)}
            >
              <Icon name="plus" size={13} aria-hidden />
              添加到分组
              <span className="ctx-item-tail">
                {contextMenu.records.length > 1 && <span>{contextMenu.records.length} 条</span>}
                <Icon name={contextMenu.showGroupTargets ? 'chevron-down' : 'chevron-right'} size={12} aria-hidden />
              </span>
            </button>
            {contextMenu.showGroupTargets && (
              <div className="ctx-group-targets" role="group" aria-label="选择分组">
                {contextMenu.records.length > 1 && onCreateGroup && (
                  <button
                    type="button"
                    className="ctx-item ctx-group-target"
                    role="menuitem"
                    onClick={() => {
                      const records = contextMenu.records
                      setContextMenu(null)
                      onCreateGroup(records)
                    }}
                  >
                    <Icon name="plus" size={13} aria-hidden />
                    新建分组
                  </button>
                )}
                {onDropRecordIntoGroup && recordGroups?.map(group => {
                  const alreadyInGroup = contextMenu.records.every(coordinate => (
                    group.records.some(member => sameCoordinate(member, coordinate))
                  ))
                  return (
                    <button
                      key={group.id}
                      type="button"
                      className="ctx-item ctx-group-target"
                      role="menuitem"
                      disabled={alreadyInGroup}
                      title={alreadyInGroup ? '所选记录已在此分组中' : undefined}
                      onClick={() => {
                        const records = contextMenu.records
                        setContextMenu(null)
                        onDropRecordIntoGroup(records, group.id)
                      }}
                    >
                      <span
                        className={`ctx-group-color${group.color ? ' has-color' : ''}`}
                        style={recordGroupColorStyle(group.color)}
                        aria-hidden
                      />
                      <span className="ctx-group-name">{group.name}</span>
                      <span className="ctx-shortcut">{group.records.length}</span>
                    </button>
                  )
                })}
              </div>
            )}
          </>)}
          {contextMenu.records.length === 1 && !readOnly && data.capabilities.can_edit_key && onRenameRecord && (
            <div className="ctx-item" role="menuitem" onClick={async () => {
              const key = recordKey(contextMenu.row)
              const next = window.prompt('重命名 Key', key)?.trim()
              const coordinate = contextMenu.row.coordinate
              setContextMenu(null)
              if (!next || next === key) return
              await onRenameRecord(coordinate, next)
            }}>
              <Icon name="edit" size={13} aria-hidden />
              重命名 Key
            </div>
          )}
          {contextMenu.records.length === 1 && !readOnly && data.capabilities.can_reorder_records && onMoveRecord && sorting.length === 0 && !globalFilter.trim() && contextMenu.row.container_index > 0 && (
            <div className="ctx-item" role="menuitem" onClick={async () => {
              const row = contextMenu.row
              setContextMenu(null)
              await onMoveRecord(row.coordinate, row.container_index - 1)
            }}>
              <Icon name="arrow-up" size={13} aria-hidden />
              上移一位
            </div>
          )}
          {contextMenu.records.length === 1 && !readOnly && data.capabilities.can_reorder_records && onMoveRecord && sorting.length === 0 && !globalFilter.trim() && contextMenu.row.container_index + 1 < contextMenu.row.container_size && (
            <div className="ctx-item" role="menuitem" onClick={async () => {
              const row = contextMenu.row
              setContextMenu(null)
              await onMoveRecord(row.coordinate, row.container_index + 1)
            }}>
              <Icon name="arrow-down" size={13} aria-hidden />
              下移一位
            </div>
          )}
          {contextMenu.records.length === 1 && !readOnly && data.capabilities.can_reorder_records && onMoveRecord && sorting.length === 0 && !globalFilter.trim() && (
            <div className="ctx-item" role="menuitem" onClick={async () => {
              const row = contextMenu.row
              const raw = window.prompt(
                `移动到位置（0-${row.container_size - 1}）`,
                String(row.container_index),
              )
              setContextMenu(null)
              if (raw === null || !/^\d+$/.test(raw.trim())) return
              const target = Number(raw)
              if (target === row.container_index || target >= row.container_size) return
              await onMoveRecord(row.coordinate, target)
            }}>
              <Icon name="record" size={13} aria-hidden />
              移动到位置…
            </div>
          )}
          {contextMenu.records.length === 1 && !readOnly && data.capabilities.can_reorder_records && onSwapRecords && sorting.length === 0 && !globalFilter.trim() && (
            <div className="ctx-item" role="menuitem" onClick={async () => {
              const row = contextMenu.row
              const raw = window.prompt('与记录 Key 交换位置')?.trim()
              setContextMenu(null)
              if (!raw || raw === row.coordinate.key) return
              const other = data.records.find(candidate => (
                candidate.coordinate.actual_type === row.coordinate.actual_type
                && candidate.coordinate.key === raw
              ))
              if (!other) return
              await onSwapRecords(row.coordinate, other.coordinate)
            }}>
              <Icon name="refresh" size={13} aria-hidden />
              交换位置…
            </div>
          )}
          {contextMenu.records.length === 1 && !readOnly && onTransferRecord && transferTargets.length > 0 && sorting.length === 0 && !globalFilter.trim() && (
            <div className="ctx-item" role="menuitem" onClick={() => {
              const row = contextMenu.row
              setContextMenu(null)
              setTransferRow(row)
            }}>
              <Icon name="record" size={13} aria-hidden />
              移动到其他文件…
            </div>
          )}
          {contextMenu.records.length === 1 && !readOnly && data.capabilities.can_delete_record && onDeleteRecord && (
            <div className="ctx-item ctx-danger" role="menuitem" onClick={async () => {
              const key = recordKey(contextMenu.row)
              const coordinate = contextMenu.row.coordinate
              setContextMenu(null)
              if (!window.confirm(`确认删除记录 ${key}？此操作不可撤销。`)) return
              await onDeleteRecord(coordinate)
            }}>
              <Icon name="close" size={13} aria-hidden />
              删除记录
            </div>
          )}
        </div>,
        document.body,
      )}
    </div>
  )
})


/** Approximate visual width in pixels for `text` at 12px UI font. Uses a
 *  codepoint scan (East Asian Wide ≈ 2 ASCII cells) instead of canvas
 *  measurement — deterministic across webview builds and cheap enough to
 *  run over every cell. Not pixel-perfect, but the caller adds chrome
 *  padding and clamps to a max, so approximate is enough. */
function estimateTextWidth(text: string, monospace = false): number {
  const narrow = monospace ? 7.3 : 6.6
  const wide = monospace ? 13.6 : 13.2
  let w = 0
  for (const ch of text) {
    const cp = ch.codePointAt(0)!
    w += isEastAsianWide(cp) ? wide : narrow
  }
  return w
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

function fieldCell(record: RecordRow, fieldName: string) {
  const index = record.field_index[fieldName]
  return typeof index === 'number' ? record.fields[index] : undefined
}

function inferredCellType(cell: RecordRow['fields'][number] | undefined): string | undefined {
  if (!cell) return undefined
  const value = cell.value
  if (value.kind === 'enum') return value.value.enum_name
  if (value.kind === 'ref') return 'ref'
  if (value.kind === 'object') return value.value.actual_type
  if (value.kind === 'array') return 'array'
  if (value.kind === 'dict') return 'dict'
  if (value.kind === 'null') return 'null'
  return value.kind
}

function isComplexValue(value: FieldValue | undefined): boolean {
  return value?.kind === 'object' || value?.kind === 'array' || value?.kind === 'dict'
}



/** Stable identity for the column set: file path + column names joined.
 *  Used as a memo key so column widths only recompute when the schema-
 *  determined column set changes, not when a cell value updates. */
function columnKeySignature(data: FileRecords): string {
  return data.columns.map(c => c.name).join('')
}

function columnDropdownKind(
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

function severityForCoordinate(
  diagnostics: DiagnosticItem[] | undefined,
  filePath: string,
  coordinate: RecordCoordinate,
): 'error' | 'warning' | null {
  if (!diagnostics) return null
  let sev: 'error' | 'warning' | null = null
  for (const d of diagnostics) {
    if (d.file_path !== filePath || !diagnosticMatchesCoordinate(d, coordinate)) continue
    if (d.severity === 'error') return 'error'
    if (d.severity === 'warning') sev = 'warning'
  }
  return sev
}

function findDiagMessage(
  diags: DiagnosticItem[] | undefined,
  filePath: string,
  coordinate: RecordCoordinate,
  topField: string,
): string | undefined {
  if (!diags) return undefined
  const msgs: string[] = []
  for (const d of diags) {
    if (d.file_path !== filePath || !diagnosticMatchesCoordinate(d, coordinate)) continue
    const top = d.field_path ? d.field_path.split(/[.[]/, 1)[0] : null
    if (top !== topField) continue
    msgs.push(d.message)
  }
  return msgs.length ? msgs.join('\n') : undefined
}

function selectedTopLevelField(path: FieldPathSegment[]): string | null {
  return path.length === 1 && path[0].kind === 'field' ? path[0].value : null
}

function tableCellKey(coordinate: RecordCoordinate, fieldName: string): string {
  return `${coordinateId(coordinate)}::${fieldName}`
}

function isNativeEditingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  return target.isContentEditable
    || target.tagName === 'INPUT'
    || target.tagName === 'TEXTAREA'
    || target.tagName === 'SELECT'
}

function CellSyntaxEditor({
  initialText,
  onCommit,
  onCancel,
}: {
  initialText: string
  onCommit: (text: string) => Promise<void>
  onCancel: () => void
}) {
  const [text, setText] = useState(initialText)
  const [busy, setBusy] = useState(false)
  return (
    <input
      className="dc-input dc-input-flat cell-syntax-editor"
      value={text}
      autoFocus
      readOnly={busy}
      onChange={e => setText(e.target.value)}
      onBlur={onCancel}
      onKeyDown={async e => {
        e.stopPropagation()
        if (e.key === 'Escape') onCancel()
        if (e.key === 'Enter' && !busy) {
          e.preventDefault()
          setBusy(true)
          await onCommit(text)
          setBusy(false)
        }
      }}
      aria-label="单元格语法编辑器"
    />
  )
}

function EditableCell({
  value, label, editable, refTargetType, enumType, nullable, declaredType, onCommit, onEditingFinished,
}: {
  value: FieldValue
  label?: string
  editable: boolean
  refTargetType?: string
  enumType?: string
  nullable?: boolean
  declaredType?: string
  onCommit?: (next: FieldValue) => void
  onEditingFinished?: () => void
}) {
  const [editing, setEditing] = useState(false)
  const isScalar = value.kind === 'bool' || value.kind === 'int' || value.kind === 'float'
                || value.kind === 'string' || value.kind === 'enum' || value.kind === 'ref'
  // null cells become editable when the schema tells us they hold an enum/ref/bool
  const isNullDropdown = value.kind === 'null' && !!(enumType || refTargetType)
  const canEdit = editable && (isScalar || isNullDropdown) && !!onCommit
  const commitAndRestoreFocus = (next: FieldValue) => {
    onCommit!(next)
    requestAnimationFrame(() => onEditingFinished?.())
  }

  // Bool: checkbox, always visible
  if (canEdit && value.kind === 'bool') {
    return (
      <div className="cell-edit-wrap">
        <input
          type="checkbox"
          className="dc-checkbox"
          checked={value.value}
          onChange={e => onCommit!({ kind: 'bool', value: e.target.checked })}
        />
      </div>
    )
  }

  // Enum
  if (canEdit && (value.kind === 'enum' || (value.kind === 'null' && enumType))) {
    return (
      <div className="cell-edit-wrap">
        <EnumDirectSelect
          value={value as FieldValue & { kind: 'enum' | 'null' }}
          enumType={enumType}
          nullable={nullable}
          onCommit={commitAndRestoreFocus}
          onExit={onEditingFinished}
        />
      </div>
    )
  }

  // Ref
  if (canEdit && (value.kind === 'ref' || (value.kind === 'null' && refTargetType))) {
    return (
      <div className="cell-edit-wrap">
        <RefDirectSelect
          value={value as FieldValue & { kind: 'ref' | 'null' }}
          onCommit={commitAndRestoreFocus}
          targetType={refTargetType}
          nullable={nullable}
          onExit={onEditingFinished}
        />
      </div>
    )
  }

  // String / int / float: click-to-edit
  if (editing && canEdit) {
    return (
      <div className="cell-edit-wrap" onClick={(e: React.MouseEvent) => e.stopPropagation()}>
        <CellTextEditor
          value={value as FieldValue & { kind: 'int' | 'float' | 'string' }}
          onCommit={next => { onCommit!(next); setEditing(false) }}
          onCancel={() => setEditing(false)}
        />
      </div>
    )
  }
  return (
    <div
      className={`cell-edit-wrap${canEdit ? ' editable' : ''}`}
      onDoubleClick={canEdit ? (e: React.MouseEvent) => {
        e.stopPropagation()
        setEditing(true)
      } : undefined}
      title={canEdit ? '双击编辑' : undefined}
    >
      <DataCardCompact value={value} label={label} declaredType={declaredType} surface="table-cell" />
    </div>
  )
}

function selectionOwnsRow(
  selection: EditorSelection | null | undefined,
  filePath: string,
  coordinate: RecordCoordinate,
): boolean {
  return !!selection
    && selection.filePath === filePath
    && (selection.kind === 'value'
      ? sameCoordinate(selection.coordinate, coordinate)
      : selection.coordinates.some(item => sameCoordinate(item, coordinate)))
}

function CellTextEditor({
  value, onCommit, onCancel,
}: {
  value: FieldValue & { kind: 'int' | 'float' | 'string' }
  onCommit: (next: FieldValue) => void
  onCancel: () => void
}) {
  const [text, setText] = useState(
    value.kind === 'int' || value.kind === 'float' ? String(value.value) : value.value
  )
  function commit() {
    const next = parseFieldValueText(value, text)
    if (next) onCommit(next)
    else onCancel()
  }
  return (
    <input
      className="dc-input dc-input-flat"
      type={value.kind === 'float' ? 'number' : 'text'}
      inputMode={value.kind === 'int' ? 'numeric' : undefined}
      value={text}
      autoFocus
      onChange={e => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={e => {
        if (e.key === 'Enter') (e.target as HTMLInputElement).blur()
        if (e.key === 'Escape') onCancel()
      }}
    />
  )
}

function EditableKeyCell({
  value, editable, onCommit,
}: {
  value: string
  editable: boolean
  onCommit?: (next: string) => void
}) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(value)

  useEffect(() => {
    if (!editing) setDraft(value)
  }, [value, editing])

  const commit = () => {
    const next = draft.trim()
    if (next && next !== value && onCommit) onCommit(next)
    setEditing(false)
  }

  if (editing && editable) {
    return (
      <input
        className="inline-editor key-editor"
        value={draft}
        autoFocus
        onChange={e => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={e => {
          if (e.key === 'Enter') commit()
          if (e.key === 'Escape') {
            setDraft(value)
            setEditing(false)
          }
        }}
        onClick={e => e.stopPropagation()}
        aria-label="重命名记录 Key"
      />
    )
  }

  return (
    <span
      className={`cell-key${editable ? ' editable' : ''}`}
      onDoubleClick={editable ? e => {
        e.stopPropagation()
        setEditing(true)
      } : undefined}
      title={editable ? '双击重命名 Key' : undefined}
    >
      {value}
    </span>
  )
}
