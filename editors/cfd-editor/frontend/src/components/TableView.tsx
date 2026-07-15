import { useState, useEffect, useMemo, useRef, memo } from 'react'
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
import { useVirtualizer } from '@tanstack/react-virtual'
import type { FileRecords } from '../bindings/FileRecords'
import type { CreateRecordDraft } from '../bindings/CreateRecordDraft'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import {
  coordinateId,
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
import { DiagBadge } from './DiagBadge'
import { Icon } from './Icon'
import {
  selectionMatchesRecord,
  selectionMatchesValue,
  type EditorSelection,
} from '../state/editorSelection'
import {
  moveTableSelection,
  type TableDirection,
} from '../state/tableCellNavigation'
import { selectionEditIntentForKey } from '../state/selectionKeyboard'

interface Props {
  data: FileRecords
  activeType: string
  readOnly?: boolean
  diagnostics?: DiagnosticItem[]
  /** Pre-populate the search filter from the parent global search bar. */
  searchQuery?: string
  /** Current record/value selection, lifted so it can drive the inspector. */
  selection?: EditorSelection | null
  /** Click on the Key cell: select the record. */
  onSelectRecord?: (coordinate: RecordCoordinate) => void
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
  /** Click on a corner badge on a row or cell. `fieldPath` is null for
   *  record-level (the Key column badge), otherwise the column name. */
  onDiagnosticBadgeClick?: (coordinate: RecordCoordinate, fieldPath: string | null) => void
}

const ROW_H = 30

export const TableView = memo(function TableView({ data, activeType, readOnly, diagnostics, searchQuery, selection, onSelectRecord, onSelectValue, onRenderCellText, onParseCellText, onClearSelection, onOpenRecord, onWriteField, onRenameRecord, onInsertRecord, onCreateRecordDraft, onDeleteRecord, onDiagnosticBadgeClick }: Props) {
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; row: RecordRow } | null>(null)
  const [showNewRecord, setShowNewRecord] = useState(false)
  const [syntaxEdit, setSyntaxEdit] = useState<{ key: string; initialText: string } | null>(null)
  const [cellNotice, setCellNotice] = useState<string | null>(null)
  const [sorting, setSorting] = useState<SortingState>([])
  const [columnSizing, setColumnSizing] = useState<ColumnSizingState>({})
  const [globalFilter, setGlobalFilter] = useState(searchQuery ?? '')

  const tableScrollRef = useRef<HTMLDivElement>(null)

  // Reset transient UI state when active file/type changes.
  useEffect(() => {
    setSorting([])
    setGlobalFilter('')
    setColumnSizing({})
  }, [data.file_path, activeType])

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
        const declared = cell?.annotation?.declared_type
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
      let declaredForHeader: string | undefined
      for (const record of snapshot.records) {
        if (recordActualType(record) !== activeType) continue
        const cell = fieldCell(record, column.name)
        if (!cell) continue
        const w = measure(valueSummary(cell.value))
        if (w > maxContent) maxContent = w
        if (!declaredForHeader) declaredForHeader = cell.annotation?.declared_type ?? undefined
      }
      const summaryWidth = maxContent + chrome
      const typeChipWidth = declaredForHeader ? measureMono(declaredForHeader) + 16 : 0
      const headerWidth = measure(column.name) + PLAIN_CHROME + 12 /* sort caret */ + typeChipWidth
      hints[column.name] = Math.min(VALUE_MAX, Math.max(MIN, Math.ceil(Math.max(summaryWidth, headerWidth))))
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
        header: 'Key',
        cell: info => {
          const filePath = dataForCellsRef.current.file_path
          const rowSev = severityForCoordinate(diagnosticsRef.current, filePath, info.row.original.coordinate)
          const badgeClick = onDiagnosticBadgeClickRef.current
          const renameFn = onRenameRecordRef.current
          return (
            <span className={`cell-key-wrap${rowSev ? ' has-diag' : ''}`}>
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
        size: columnSizeHints.key ?? 140,
      }),
      ...allFieldNames.map(name => {
        const declared = columnDeclaredTypes[name]
        return helper.display({
          id: name,
          header: () => (
            <span className="th-label">
              <span className="th-label-name">{name}</span>
              {declared && (
                <span className="th-label-type" title={`类型：${declared}`}>{declared}</span>
              )}
            </span>
          ),
          size: columnSizeHints[name] ?? 120,
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
                  editable={cellEditable}
                  refTargetType={cellRefTargetType(f)}
                  enumType={cellEnumType(f)}
                  nullable={cellNullable(f)}
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
  }, [allFieldNames, columnSizeHints, columnDeclaredTypes, canEdit, canRename])

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
    state: { sorting, columnSizing, globalFilter },
    onSortingChange: setSorting,
    onColumnSizingChange: setColumnSizing,
    onGlobalFilterChange: setGlobalFilter,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    getRowId: row => coordinateId(row.coordinate),
    globalFilterFn,
    columnResizeMode: 'onEnd',
    enableColumnResizing: true,
  })

  const rows = table.getRowModel().rows
  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => tableScrollRef.current,
    estimateSize: () => ROW_H,
    overscan: 12,
  })
  const virtualRows = rowVirtualizer.getVirtualItems()
  const totalHeight = rowVirtualizer.getTotalSize()
  const padBefore = virtualRows.length > 0 ? virtualRows[0].start : 0
  const padAfter = virtualRows.length > 0 ? totalHeight - virtualRows[virtualRows.length - 1].end : 0

  const revealTableSelection = (target: EditorSelection, openDropdown: boolean) => {
    if (target.filePath !== data.file_path) return
    const rowIndex = rows.findIndex(row => sameCoordinate(row.original.coordinate, target.coordinate))
    if (rowIndex < 0) return
    rowVirtualizer.scrollToIndex(rowIndex, { align: 'auto' })
    const columnId = target.kind === 'record'
      ? 'key'
      : selectedTopLevelField(target.fieldPath)
    if (!columnId) return
    const key = tableCellKey(target.coordinate, columnId)
    let attempts = 0
    const reveal = () => {
      const cell = tableScrollRef.current?.querySelector<HTMLElement>(
        `[data-table-cell-key="${CSS.escape(key)}"]`,
      )
      if (!cell && attempts < 4) {
        attempts += 1
        requestAnimationFrame(reveal)
        return
      }
      cell?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
      if (!openDropdown) return
      const select = cell?.querySelector<HTMLSelectElement>('select.dc-pill-select')
      if (!select) return
      select.focus({ preventScroll: true })
      try {
        select.showPicker()
      } catch {
        // Some WebViews allow focus but not programmatic native picker opening.
      }
    }
    // Try in the keyboard event's activation window so native showPicker()
    // is permitted. Virtualized rows fall back to the animation-frame retry.
    reveal()
  }

  useEffect(() => {
    if (selection) revealTableSelection(selection, false)
  // Row/column identity and selection are the only inputs that can move the target.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selection, rows.length, allFieldNames.join('\u001f')])

  // Close context menu on Escape.
  useEffect(() => {
    if (!contextMenu) return
    const h = (e: KeyboardEvent) => { if (e.key === 'Escape') setContextMenu(null) }
    window.addEventListener('keydown', h)
    return () => window.removeEventListener('keydown', h)
  }, [contextMenu])

  return (
    <div
      className="table-view"
      onClick={e => {
        setContextMenu(null)
        // Clicks that didn't land on a row deselect the current row, which
        // also closes the shared right-side inspector.
        const target = e.target as HTMLElement
        if (!target.closest('.table-row') && !target.closest('.context-menu')) {
          onClearSelection?.()
        }
      }}
    >
      <div className="table-main">
        <div
          className="table-scroll"
          ref={tableScrollRef}
          tabIndex={0}
          onKeyDown={async e => {
            if (isNativeEditingTarget(e.target)) return
            if (!selection || selection.filePath !== data.file_path) return

            const visibleCoordinates = rows.map(row => row.original.coordinate)
            if (e.key === 'ArrowLeft' || e.key === 'ArrowRight' || e.key === 'ArrowUp' || e.key === 'ArrowDown') {
              e.preventDefault()
              const next = moveTableSelection(
                selection,
                e.key as TableDirection,
                visibleCoordinates,
                allFieldNames,
              )
              if (next !== selection) {
                setSyntaxEdit(null)
                if (next.kind === 'record') onSelectRecord?.(next.coordinate)
                else onSelectValue?.(next.coordinate, next.fieldPath)
                revealTableSelection(next, true)
              }
              return
            }

            if (selection.kind !== 'value') return
            const field = selectedTopLevelField(selection.fieldPath)
            if (!field) return
            const selectedRow = rows.find(row => coordinateId(row.original.coordinate) === coordinateId(selection.coordinate))
            const selectedCell = selectedRow ? fieldCell(selectedRow.original, field) : undefined
            const editable = !!selectedCell && canEdit && !cellReadOnly(selectedCell)
            const modified = e.ctrlKey || e.metaKey || e.altKey
            const lower = e.key.toLowerCase()

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
            if ((e.ctrlKey || e.metaKey) && lower === 'v' && editable && onParseCellText && onWriteField) {
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

            const intent = editable && selectedCell
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
          <table className="data-table" style={{ width: table.getTotalSize() }}>
            <thead>
              {table.getHeaderGroups().map(hg => (
                <tr key={hg.id}>
                  {hg.headers.map(h => {
                    const sort = h.column.getIsSorted()
                    return (
                      <th
                        key={h.id}
                        style={{ width: h.getSize() }}
                        aria-sort={sort === 'asc' ? 'ascending' : sort === 'desc' ? 'descending' : 'none'}
                      >
                        <button
                          type="button"
                          className="th-sort-btn"
                          onClick={h.column.getToggleSortingHandler()}
                          disabled={!h.column.getCanSort()}
                          title={h.column.getCanSort() ? '点击排序' : undefined}
                        >
                          {flexRender(h.column.columnDef.header, h.getContext())}
                          {sort === 'asc' && <Icon name="chevron-down" size={10} className="th-sort-icon asc" aria-hidden />}
                          {sort === 'desc' && <Icon name="chevron-right" size={10} className="th-sort-icon desc" aria-hidden />}
                        </button>
                        {h.column.getCanResize() && (
                          <div
                            className="th-resizer"
                            onMouseDown={h.getResizeHandler()}
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
                const row = rows[vr.index]
                const rowSev = recordSeverity(row.original.coordinate)
                return (
                  <tr
                    key={row.id}
                    className={`table-row${selectionMatchesRecord(selection ?? null, data.file_path, row.original.coordinate) ? ' selected' : ''}${rowSev ? ' table-row-' + rowSev : ''}`}
                    onContextMenu={e => {
                      e.preventDefault()
                      setContextMenu({ x: e.clientX, y: e.clientY, row: row.original })
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
                        selected ? 'selected-cell' : '',
                      ].filter(Boolean).join(' ')
                      return (
                        <td
                          key={cell.id}
                          className={classes || undefined}
                          data-table-cell-key={tableCellKey(row.original.coordinate, cell.column.id)}
                          style={{ width: cell.column.getSize() }}
                          aria-selected={selected || undefined}
                          onMouseDown={e => {
                            // Runs before native selects open, so the inspector
                            // follows the cell even when its editor consumes click.
                            e.stopPropagation()
                            if (!isNativeEditingTarget(e.target)) {
                              tableScrollRef.current?.focus({ preventScroll: true })
                            }
                            if (fieldPath) onSelectValue?.(row.original.coordinate, fieldPath)
                            else onSelectRecord?.(row.original.coordinate)
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

      {contextMenu && (
        <div
          className="context-menu"
          style={{ left: contextMenu.x, top: contextMenu.y }}
          onClick={e => e.stopPropagation()}
          role="menu"
        >
          <div className="ctx-item" role="menuitem" onClick={() => { onOpenRecord(contextMenu.row.coordinate); setContextMenu(null) }}>
            <Icon name="record" size={13} aria-hidden />
            跳转到记录视图
          </div>
          {!readOnly && data.capabilities.can_edit_key && onRenameRecord && (
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
          {!readOnly && data.capabilities.can_delete_record && onDeleteRecord && (
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
        </div>
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
  value, editable, refTargetType, enumType, nullable, onCommit, onEditingFinished,
}: {
  value: FieldValue
  editable: boolean
  refTargetType?: string
  enumType?: string
  nullable?: boolean
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
      <DataCardCompact value={value} />
    </div>
  )
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
      onClick={editable ? e => {
        e.stopPropagation()
        setEditing(true)
      } : undefined}
      title={editable ? '点击重命名 Key' : undefined}
    >
      {value}
    </span>
  )
}
