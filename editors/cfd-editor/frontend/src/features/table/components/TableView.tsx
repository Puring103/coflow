import { CellSyntaxEditor, TablePreviewRows, TableCellContent, EditableCell } from './TableCell'
import { useTableSelectionDismiss } from '../hooks/useTableSelectionDismiss'
import { useCellMotion } from '../../../shared/motion/useCellMotion'
import { motionRecordKey } from '../../record-editing/hooks/useMutationFeedback'
import { cellMotionOrigin, emphasize, type MotionOrigin } from '../../../shared/motion/motion'
import { TableExpansionContext, TableExpansionStore, previewRowCount, previewWindow } from './tableExpansion'
import { DataFillContext } from '../../record-editing/state/dataFillContext'

import { sameFieldValue } from '../../record-editing/state/fieldProjection'
import { getSharedFlatRowModel } from '../state/flatRowModel'
import { readClipboardText, writeClipboardText } from '../../../api/clipboard'
import { finishOffscreenDataEdit } from '../../record-editing/state/editSession'
import { recordMatchesKeySearch } from '../../record-editing/value/fieldValue'
import { useState, useEffect, useMemo, useCallback, useRef, memo } from 'react'

import { ShortNameColumnMenu } from '../../record-editing/components/ShortNameContext'
import { shortNameCandidate } from '../../record-editing/state/shortNames'
import { useReactTable, getSortedRowModel, getFilteredRowModel, flexRender, createColumnHelper, type SortingState, type ColumnSizingState } from '@tanstack/react-table'
import type { Row as TanStackRow } from '@tanstack/react-table'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { FileRecords } from '../../../bindings/FileRecords'
import type { CreateRecordDraft } from '../../../bindings/CreateRecordDraft'
import type { RecordCoordinate } from '../../../bindings/RecordCoordinate'
import type { RecordRow } from '../../../bindings/RecordRow'
import type { BatchWriteFieldInput } from '../../../bindings/BatchWriteFieldInput'
import type { EditorRecordGroup } from '../../../bindings/EditorRecordGroup'
import { coordinateId, cellDeclaredType, cellEnumType, cellEnumIsFlag, cellNullable, cellReadOnly, cellRefTargetType, diagnosticDisplayMessage, diagnosticRecordTarget, diagnosticSeverity, errorMessage, fieldPathField, isComplexValue, presentationValue, recordActualType, recordId, sameCoordinate, replacePresentationValue, type DiagnosticItem, type FieldPathSegment, type FieldValue } from '../../../wire'
import { MissingValueRepair } from '../../record-editing/components/DataCard'
import { highlightSearchText } from '../../record-editing/components/searchHighlight'
import { recordMatchesFullTextSearch, recordMatchesSearch } from '../../record-editing/value/fieldValue'
import { CreateRecordDialog } from '../../record-editing/components/CreateRecordDialog'
import { ConfirmDialog, TextInputDialog } from '../../../shared/ui/ActionDialog'
import { DiagBadge } from '../../diagnostics/components/DiagBadge'
import { Icon } from '../../../shared/ui/Icon'

import { visibilityScrollDelta } from '../state/scrollVisibility'
import { fieldMetadataTitle } from '../../../shared/lib/fieldMetadata'
import { usePluginRegistry } from '../../plugins/runtime'

import { recordSelection, recordSelectionCoordinates, selectionMatchesRecord, valueSelectionRange, type SelectedCells, type EditorSelection, type RecordSelectionMode, type ValueSelectionMode } from '../../workspace/state/editorSelection'
import { moveTableSelection, type TableDirection } from '../state/tableCellNavigation'
import { defaultValueForClear, selectionEditIntentForKey } from '../../workspace/state/selectionKeyboard'
import { canReleaseResizeScrollFloor, resizedColumnWidth, resizeScrollWidthFloor } from '../state/tableColumnResize'
import { fieldTypeColor } from '../../../shared/lib/typeColor'
import { organizeRecordRows, type RecordGroupView } from '../../workspace/state/manualRecordGroups'
import { useRecordPointerDrag } from '../../workspace/hooks/useRecordPointerDrag'
import { useTableCellRangeDrag } from '../hooks/useTableCellRangeDrag'
import { parseCfdClipboard, planPaste, serializeCfdCellMatrix, serializeRecordRefsAsCfd, shouldExpandSinglePasteTarget } from '../../record-editing/state/clipboard'
import { RecordGroupHeader, RecordUngroupedHeader, recordGroupColorStyle } from '../../record-editing/components/RecordGroupHeader'
import { RecordContextMenu } from '../../record-editing/components/RecordContextMenu'

import { isNativeEditorTarget, replaceInputText } from '../../../shared/lib/dom'
import { boundedPasteMatrix, elementBounds, estimateTextWidth, fieldCell, fitsAxis, pasteCellFor, selectedTopLevelField, selectionCellMatrix, tableCellKey } from './TableView.support'

// 按不可变数据快照缓存列准备结果。基础视图、HEAD 与工作区投影互不串用注解，
// 同一查询对象跨组件挂载仍可复用，过期快照由 WeakMap 自动回收。
const COLUMN_PREP_CACHE = new WeakMap<FileRecords, Map<string, unknown>>()

function columnPrepKey(kind: string, activeType: string, extra = ''): string {
  return [kind, activeType, extra].join('\u001f')
}

function cachedColumnPrep<T>(data: FileRecords, key: string, compute: () => T): T {
  let cache = COLUMN_PREP_CACHE.get(data)
  if (!cache) { cache = new Map(); COLUMN_PREP_CACHE.set(data, cache) }
  const cached = cache.get(key)
  if (cached !== undefined) return cached as T
  const value = compute()
  cache.set(key, value)
  return value
}

interface Props {
  rowHeights?: Record<string, number | undefined>
  onRowHeightsChange?: (heights: Record<string, number>) => void
  onExpandCell?: (coordinate: RecordCoordinate, path: FieldPathSegment[] | null, origin: MotionOrigin) => void
  data: FileRecords
  activeType: string
  readOnly?: boolean
  diagnostics?: DiagnosticItem[]
  /** Pre-populate the search filter from the parent global search bar. */
  keyOnlySearch?: boolean
  searchQuery?: string
  /** Search nested field values as well as the standard record summary. */
  fullTextSearch?: boolean
  recordGroups?: readonly EditorRecordGroup[]
  collapsedGroupKeys?: ReadonlySet<string>
  onToggleGroup?: (groupKey: string) => void
  onDropRecordOntoRecord?: (sources: readonly RecordCoordinate[], target: RecordCoordinate) => void
  onDropRecordAfterRecord?: (sources: readonly RecordCoordinate[], target: RecordCoordinate) => void
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
  onSelectValue?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], mode?: ValueSelectionMode) => void
  /** Reports the exact visible rectangle so the inspector shares table range semantics. */
  onValueSelectionCellsChange?: (cells: SelectedCells) => void
  onRenderCellText?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[]) => Promise<string>
  onParseCellText?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], text: string) => Promise<FieldValue>
  /** Click on blank space inside the table view: deselect / close inspector. */
  onClearSelection?: () => void
  /** Dbl-click / context-menu jump to the dedicated record view. */
  onOpenRecord: (coordinate: RecordCoordinate) => void
  onWriteField?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], newValue: FieldValue) => Promise<RecordRow | void>
  onWriteFieldBatch?: (writes: readonly BatchWriteFieldInput[]) => Promise<void>
  onRenameRecord?: (coordinate: RecordCoordinate, newId: string) => Promise<RecordRow | void>
  /** Create a new record. Resolves once the back-end has persisted and the
   *  parent has refreshed `data` for this file. */
  onInsertRecord?: (recordId: string, actualType: string, fields: FieldValue) => Promise<void>
  onCreateRecordDraft?: (actualType: string) => Promise<CreateRecordDraft>
  /** Delete the records selected by the row context menu. */
  onDeleteRecords?: (coordinates: readonly RecordCoordinate[]) => Promise<void>
  onMoveRecord?: (coordinate: RecordCoordinate, targetIndex: number) => Promise<void>
  /** Click on a corner badge on a row or cell. `fieldPath` is null for
   *  record-level (the Key column badge), otherwise the column name. */
  onDiagnosticBadgeClick?: (coordinate: RecordCoordinate, fieldPath: string | null) => void
  columnWidths?: ColumnSizingState
  onColumnWidthsChange?: (widths: ColumnSizingState) => void
  /** Custom table view: ordered visible columns. undefined => all columns. */
  visibleColumns?: readonly string[]
  onEnterInspector?: () => void
  focusRequest?: number
  firstRecordFocusRequest?: number
  onFirstRecordFocusConsumed?: (request: number) => void
  onNavigationBoundary?: (direction: TableDirection) => void
  rowPresentation?: (row: RecordRow, index: number) => TableRowPresentation | undefined
}

export interface TableRowPresentation {
  id: string
  /** 对比行有独立展示身份，业务记录身份用于标题和插件上下文。 */
  coordinate?: RecordCoordinate
  version: 'HEAD' | '当前'
  change: 'added' | 'deleted' | 'modified'
  changedFields: ReadonlySet<string>
  expandedPaths?: ReadonlySet<string>
  changedPaths?: ReadonlySet<string>
  onToggleExpandedPath?: (path: string, expanded: boolean) => void
  /** Diff 分组首行（每条记录一组的首行），用于渲染组间分隔线。 */
  groupStart?: boolean
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
  row: RecordRow
  records: RecordCoordinate[]
}

export const TableView = memo(function TableView({ rowHeights, onRowHeightsChange, onExpandCell, data, activeType, readOnly, diagnostics, searchQuery, keyOnlySearch = false, fullTextSearch = false, recordGroups, collapsedGroupKeys, onToggleGroup, onDropRecordOntoRecord, onDropRecordAfterRecord, onCreateGroup, onDropRecordIntoGroup, onDropRecordIntoUngrouped, onRenameGroup, onColorGroup, selection, onSelectRecord, onSelectValue, onValueSelectionCellsChange, onRenderCellText, onParseCellText, onClearSelection, onOpenRecord, onWriteField, onWriteFieldBatch, onRenameRecord, onInsertRecord, onCreateRecordDraft, onDeleteRecords, onMoveRecord, onDiagnosticBadgeClick, columnWidths, onColumnWidthsChange, visibleColumns, onEnterInspector, focusRequest, firstRecordFocusRequest, onFirstRecordFocusConsumed, onNavigationBoundary, rowPresentation }: Props) {
  const expansionStore = useMemo(() => new TableExpansionStore(), [data.file_path, activeType])
  const [pendingRecordReveal, setPendingRecordReveal] = useState<{ filePath: string; coordinate: RecordCoordinate } | null>(null)
  const [pendingRemoval, setPendingRemoval] = useState<{ filePath: string; deleted: Set<string>; next?: RecordCoordinate } | null>(null)
  const pendingEmphasis = useRef<string | null>(null)
  useEffect(() => { setPendingRecordReveal(null); setPendingRemoval(null); pendingEmphasis.current = null }, [data.file_path, activeType])
  const [localRowHeights, setLocalRowHeights] = useState<Record<string, number>>({})
  useEffect(() => { setLocalRowHeights(Object.fromEntries(Object.entries(rowHeights ?? {}).filter((entry): entry is [string, number] => entry[1] !== undefined))) }, [rowHeights])
  const rowHeightsRef = useRef(localRowHeights)
  rowHeightsRef.current = localRowHeights
  const onExpandCellRef = useRef(onExpandCell)
  onExpandCellRef.current = onExpandCell
  // 拖动时只更新本地尺寸，结束后再持久化，避免逐帧请求后端。
  const resizeRecordRow = (id: string, height: number | null, save: boolean) => {
    const next = { ...rowHeightsRef.current }
    if (height === null) delete next[id]
    else next[id] = Math.max(32, Math.round(height))
    rowHeightsRef.current = next
    setLocalRowHeights(next)
    if (save) onRowHeightsChange?.(next)
  }
  const [contextMenu, setContextMenu] = useState<TableContextMenu | null>(null)
  const [showNewRecord, setShowNewRecord] = useState(false)
  const [shortNameMenu, setShortNameMenu] = useState<{ x: number; y: number; field: string } | null>(null)
  useEffect(() => { setShortNameMenu(null) }, [data.file_path, activeType])
  const [insertAfterRow, setInsertAfterRow] = useState<RecordRow | null>(null)
  const [syntaxEdit, setSyntaxEdit] = useState<{ key: string; initialText: string } | null>(null)
  const [cellNotice, setCellNotice] = useState<string | null>(null)
  const [recordAction, setRecordAction] = useState<
    | { kind: 'rename'; coordinate: RecordCoordinate; id: string }
    | { kind: 'delete'; records: RecordCoordinate[]; message: string }
    | null
  >(null)
  const [sorting, setSorting] = useState<SortingState>([])
  const [columnSizing, setColumnSizing] = useState<ColumnSizingState>(() => columnWidths ?? {})
  const [globalFilter, setGlobalFilter] = useState(searchQuery ?? '')
  const [tableZoom, setTableZoom] = useState(1)
  const [scrollWidthFloor, setScrollWidthFloor] = useState<number | null>(null)

  const tableScrollRef = useRef<HTMLDivElement>(null)
  useCellMotion(tableScrollRef)
  useTableSelectionDismiss(tableScrollRef, selection?.kind ?? null, () => onClearSelection?.())
  const columnSizingRef = useRef(columnSizing)
  const columnResizeRef = useRef<{
    pointerId: number
    columnId: string
    startX: number
    startWidth: number
  } | null>(null)
  const clipboardBusyRef = useRef(false)
  const pendingCut = useRef<{ text: string; targets: ReturnType<typeof pasteCellFor>[] } | null>(null)
  columnSizingRef.current = columnSizing

  const updateColumnResize = (pointerId: number, clientX: number) => {
    const resize = columnResizeRef.current
    if (!resize || resize.pointerId !== pointerId) return
    const width = resizedColumnWidth(
      resize.startWidth,
      resize.startX,
      clientX,
      tableZoom,
      MIN_COLUMN_WIDTH,
    )
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
    setScrollWidthFloor(null)
  }, [data.file_path, activeType, columnWidths])

  // Sync search from parent global search bar.
  useEffect(() => {
    setGlobalFilter(searchQuery ?? '')
  }, [searchQuery])

  const filtered = useMemo(
    () => data.records.filter(r => recordActualType(r) === activeType),
    [data.records, activeType]
  )

  // 一次遍历诊断，同时建立单元格严重级别、整行严重级别和单元格消息索引，
  // 避免渲染时对每个可见单元格反复扫描整个诊断列表。
  const diagIndex = useMemo(() => {
    const cells = new Map<string, 'error' | 'warning' | 'info'>()
    const rows = new Map<string, 'error' | 'warning'>()
    const messages = new Map<string, string[]>()
    if (!diagnostics) return { cells, rows, messages }
    const rank = (s: 'error' | 'warning' | 'info') => s === 'error' ? 3 : s === 'warning' ? 2 : 1
    for (const d of diagnostics) {
      const target = diagnosticRecordTarget(d)
      if (!target || target.file_path !== data.file_path) continue
      // Take the first path segment as the column we'll mark.
      const top = target.kind === 'table_field'
        ? target.field_path.split(/[.[]/, 1)[0]
        : null
      const coordKey = coordinateId(target.coordinate)
      const key = top ? `${coordKey}::${top}` : `${coordKey}::*`
      const severity = diagnosticSeverity(d.severity)
      const cur = cells.get(key)
      if (!cur || rank(severity) > rank(cur)) cells.set(key, severity)
      if (severity === 'error' || severity === 'warning') {
        const rowCur = rows.get(coordKey)
        if (rowCur !== 'error') rows.set(coordKey, severity === 'error' ? 'error' : (rowCur ?? 'warning'))
        if (top) {
          const list = messages.get(key)
          if (list) list.push(diagnosticDisplayMessage(d))
          else messages.set(key, [diagnosticDisplayMessage(d)])
        }
      }
    }
    return { cells, rows, messages }
  }, [diagnostics, data.file_path])
  const recordSeverity = (coordinate: RecordCoordinate): 'error' | 'warning' | null =>
    diagIndex.rows.get(coordinateId(coordinate)) ?? null

  const allFieldNames = useMemo(
    () => {
      const available = data.columns
        .filter(column => column.type_names.includes(activeType))
        .map(column => column.name)
      if (!visibleColumns) return available
      // Custom view: restrict to selected columns, in the selected order,
      // dropping any that no longer exist on the type.
      const availableSet = new Set(available)
      return visibleColumns.filter(name => availableSet.has(name))
    },
    [data.columns, activeType, visibleColumns]
  )

  // 渲染闭包读取最新快照，列定义身份不随单条记录写入改变。
  const dataForCellsRef = useRef(data)
  dataForCellsRef.current = data
  const rowPresentationRef = useRef(rowPresentation)
  rowPresentationRef.current = rowPresentation

  // 列的下拉/胶囊类型（ref/enum/bool）只依赖 schema；同一列集合内计算一次，
  // 供 pill-class 与列宽估算共用，避免两处重复扫描记录。
  const columnKinds = useMemo(
    () => {
      const kinds: Record<string, 'ref' | 'enum' | 'bool' | null> = {}
      for (const column of data.columns) {
        if (!column.type_names.includes(activeType)) continue
        const kind = column.content_metrics[activeType]?.dropdown_kind
        kinds[column.name] = kind === 'ref' || kind === 'enum' || kind === 'bool' ? kind : null
      }
      return kinds
    },
    [data.columns, activeType],
  )

  // Which columns render as pill cells (ref/enum). Freezing this once per
  // column set means the `pill-cell` class on the td stays stable across
  // writes; without it, a briefly-mounted wrapper span (e.g. diagnostics)
  // would flip a :has() rule and shift the whole column's padding.
  const pillColumns = useMemo(() => {
    const set = new Set<string>()
    for (const [name, kind] of Object.entries(columnKinds)) {
      if (kind !== null) set.add(name)
    }
    return set
  }, [columnKinds])

  // 类型和显示信息由后端按实际类型聚合；列准备不再遍历记录。
  const columnDeclaredTypes = useMemo(() => Object.fromEntries(data.columns
    .filter(column => column.type_names.includes(activeType))
    .flatMap(column => {
      const declared = column.content_metrics[activeType]?.declared_type
      return declared ? [[column.name, declared]] : []
    })), [data.columns, activeType])

  const columnDisplay = useMemo(() => Object.fromEntries(data.columns
    .filter(column => column.type_names.includes(activeType))
    .map(column => {
      const metadata = column.content_metrics[activeType]
      return [column.name, { label: metadata?.label ?? undefined, description: metadata?.description ?? undefined }]
    })), [data.columns, activeType])

  // Compute column widths once per (file, activeType, column set) and freeze
  // them for the session. Recomputing on every `data.records` update meant a
  // ref-cell edit could change the column's max-summary length by a few px
  // and snap the whole column to a new width — visible as jitter. Editing a
  // value shouldn't reflow the layout; the user can drag-resize if a newly
  // added cell needs more room.
  const columnStructureKey = useMemo(() => JSON.stringify([allFieldNames, columnKinds, columnDeclaredTypes, columnDisplay]), [allFieldNames, columnKinds, columnDeclaredTypes, columnDisplay])
  const columnSizeHints = useMemo(
    () => cachedColumnPrep(data, columnPrepKey('sizeHints', activeType, allFieldNames.join(',')), () => {
      const snapshot = dataForCellsRef.current
      const measure = (text: string) => estimateTextWidth(text, false, 600)
      const measureMono = (text: string) => estimateTextWidth(text, true, 500)
      // Chrome around the content per cell type.
      const PILL_CHROME = 46
      const REF_PREFIX = 14
      const PLAIN_CHROME = 24
      const BADGE_ROOM = 16 // reserve space for the corner diag badge
      const MIN = 90
      // Only value columns are capped, so a rogue 5000-char string doesn't
      // stretch its column across the whole viewport. The user can still drag
      // wider. Key columns reserve room for dragging and diagnostics.
      const VALUE_MAX = 600
      // Key column: measure the longest key so it never truncates unless the
      // user shrinks it manually, up to the automatic width limit.
      let keyWidth = measure('Key') + PLAIN_CHROME + 12 /* sort caret */
      for (const record of snapshot.records) {
        if (recordActualType(record) !== activeType) continue
        keyWidth = Math.max(keyWidth, measureMono(rowPresentationRef.current?.(record, record.container_index)?.coordinate?.id ?? recordId(record)) + PLAIN_CHROME + BADGE_ROOM + 18 /* 拖动手柄与间距 */)
        // 已达到自动列宽上限，后续记录不再影响布局。
        if (keyWidth >= 500) break
      }
      const KEY_MAX = 500
      const hints: Record<string, number> = { id: Math.min(KEY_MAX, Math.max(MIN, Math.ceil(keyWidth))) }
      for (const column of snapshot.columns) {
        if (!column.type_names.includes(activeType)) continue
        const kind = columnKinds[column.name] ?? null
        const isPill = kind !== null
        const chrome = (isPill ? PILL_CHROME + (kind === 'ref' ? REF_PREFIX : 0) : PLAIN_CHROME) + BADGE_ROOM
        const metrics = column.content_metrics[activeType]
        const maxContent = metrics?.max_width ?? 0
        const hasComplexValue = metrics?.has_complex_value ?? false
        const summaryWidth = maxContent + chrome
        const declaredForHeader = columnDeclaredTypes[column.name]
        const typeChipWidth = declaredForHeader ? measureMono(declaredForHeader) + 16 : 0
        const headerLabel = columnDisplay[column.name]?.label ?? column.name
        const headerWidth = measure(headerLabel) + PLAIN_CHROME + 12 /* sort caret */ + typeChipWidth
        const minimumWidth = hasComplexValue ? 300 : MIN
        hints[column.name] = Math.min(VALUE_MAX, Math.max(minimumWidth, Math.ceil(Math.max(summaryWidth, headerWidth))))
      }
      return hints
    }),
    // 只在文件、类型或字段结构改变时重新估宽，保存数据不移动列边界。
    [data.file_path, activeType, columnStructureKey],
  )

  const canEdit = !readOnly && !!onWriteField
  const canRename = !readOnly && data.capabilities.can_edit_id && !!onRenameRecord

  // Cell renderers read these through refs so writes (which change
  // diagnostics + record identity every time) don't rebuild the `columns`
  // memo. A rebuilt memo → new columnDef identities → react-table replays
  // its column-size default from the memo's `size`, and if the user hasn't
  // dragged the column that briefly re-snaps to the new hint, showing up
  // as jitter when the ref column's summary width just changed.
  const diagnosticsRef = useRef(diagnostics)
  diagnosticsRef.current = diagnostics
  const diagIndexRef = useRef(diagIndex)
  diagIndexRef.current = diagIndex
  const onWriteFieldRef = useRef(onWriteField)
  onWriteFieldRef.current = onWriteField
  const onDiagnosticBadgeClickRef = useRef(onDiagnosticBadgeClick)
  onDiagnosticBadgeClickRef.current = onDiagnosticBadgeClick
  const syntaxEditRef = useRef(syntaxEdit)
  syntaxEditRef.current = syntaxEdit
  const highlightQueryRef = useRef(fullTextSearch ? globalFilter : '')
  highlightQueryRef.current = fullTextSearch ? globalFilter : ''
  const onParseCellTextRef = useRef(onParseCellText)
  onParseCellTextRef.current = onParseCellText
  const columns = useMemo(() => {
    const helper = createColumnHelper<RecordRow>()
    return [
      helper.accessor(row => recordId(row), {
        id: 'id',
        header: () => (
          <span className="th-label">
            <span className="th-label-name">{highlightSearchText('Key', highlightQueryRef.current)}</span>
            <span className="th-label-type th-label-type-key">id</span>
          </span>
        ),
        cell: info => {
          const rowSev = diagIndexRef.current.rows.get(coordinateId(info.row.original.coordinate)) ?? null
          const badgeClick = onDiagnosticBadgeClickRef.current
          return (
            <span className={`cell-key-wrap${rowSev ? ' has-diag' : ''}`}>
              <Icon name="grip" size={13} className="record-drag-handle" aria-hidden />
              <span className="cell-key" title="双击打开记录详情">{highlightSearchText(rowPresentationRef.current?.(info.row.original, info.row.index)?.coordinate?.id ?? info.getValue(), highlightQueryRef.current)}</span>
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
        size: columnWidths?.id ?? columnSizeHints.id ?? 140,
        sortDescFirst: false,
        enableSorting: !rowPresentation,
      }),
      ...(rowPresentation ? [helper.display({
        id: '__comparison_version',
        header: () => <span className="th-label-name">版本</span>,
        cell: info => {
          const presentation = rowPresentationRef.current?.(info.row.original, info.row.index)
          return presentation
            ? <span className={`table-comparison-version ${presentation.change}`}>{presentation.version}</span>
            : null
        },
        size: 72,
        enableSorting: false,
      })] : []),
      ...allFieldNames.map(name => {
        const declared = columnDeclaredTypes[name]
        const display = columnDisplay[name]
        const label = display?.label ?? name
        return helper.display({
          id: name,
          header: () => (
            <span
              className="th-label"
              style={{ '--field-color': fieldTypeColor(declared ?? name) } as React.CSSProperties}
            >
              <span className="th-label-name" title={fieldMetadataTitle(name, display?.description)}>{highlightSearchText(label, highlightQueryRef.current)}</span>
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
            const sev = diagIndexRef.current.cells.get(`${coordinateId(row.original.coordinate)}::${name}`)
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
            const comparison = rowPresentationRef.current?.(row.original, row.index)
            const readOnlyFromSchema = cellReadOnly(f)
            const cellEditable = canEdit && !readOnlyFromSchema
            const title = readOnlyFromSchema
              ? '由源记录决定，不可编辑'
              : sev ? diagIndexRef.current.messages.get(`${coordinateId(row.original.coordinate)}::${name}`)?.join('\n') : undefined
            return (
              <span className={sev ? `dc-cell-diag dc-cell-diag-${sev}` : undefined} title={title}>
                {f.missing ? (
                  <MissingValueRepair
                    value={f.value}
                    onRepair={cellEditable && writeFn
                      ? () => writeFn(row.original.coordinate, [fieldPathField(name)], f.value).then(() => undefined)
                      : undefined}
                  />
                ) : <EditableCell
                  onExpand={onExpandCellRef.current ? origin => onExpandCellRef.current?.(row.original.coordinate, [fieldPathField(name)], origin) : undefined}
                  filePath={filePath}
                  coordinate={comparison?.coordinate ?? row.original.coordinate}
                  fieldPath={[fieldPathField(name)]}
                  value={f.value}
                  editable={cellEditable}
                  expandedPaths={comparison?.expandedPaths}
                  onRowToggle={comparison?.onToggleExpandedPath}
                  diffChangedPaths={comparison?.changedPaths}
                  annotation={f.annotation}
                  refTargetType={cellRefTargetType(f)}
                  enumType={cellEnumType(f)}
                  enumIsFlag={cellEnumIsFlag(f)}
                  nullable={cellNullable(f)}
                  declaredType={cellDeclaredType(f)}
                  highlightQuery={highlightQueryRef.current}
                  onEdit={cellEditable && writeFn ? (path, next) => writeFn(row.original.coordinate, path, next) : undefined}
                  onCommit={cellEditable && writeFn ? next => writeFn(row.original.coordinate, [fieldPathField(name)], next) : undefined}
                  onEditingFinished={() => tableScrollRef.current?.focus({ preventScroll: true })}
                />}
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
  }, [allFieldNames, columnSizeHints, columnDeclaredTypes, columnDisplay, columnWidths, canEdit, canRename, rowPresentation])

  // Global filter optionally traverses nested collections and object fields.
  const globalFilterFn = useMemo(
    () => (row: { original: RecordRow }, _columnId: string, filterValue: string) => {
      return keyOnlySearch ? recordMatchesKeySearch(row.original, filterValue) : fullTextSearch
        ? recordMatchesFullTextSearch(row.original, filterValue)
        : recordMatchesSearch(row.original, filterValue)
    },
  [fullTextSearch, keyOnlySearch],
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
    getCoreRowModel: getSharedFlatRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    getRowId: (row, index) => rowPresentation?.(row, index)?.id ?? coordinateId(row.coordinate),
    globalFilterFn,
    enableSortingRemoval: true,
    enableMultiSort: false,
    enableColumnResizing: true,
  })

  const rows = table.getRowModel().rows
  const displayItems = useMemo<TableDisplayItem[]>(() => {
    // 无分组时不建立全表坐标索引，也不执行分组扫描。
    if (!recordGroups?.length) return rows.map(row => ({ kind: 'row' as const, row }))
    const rowsById = new Map(rows.map(row => [coordinateId(row.original.coordinate), row]))
    const organized = organizeRecordRows(rows.map(row => row.original), recordGroups ?? [])
    if (organized.groups.length === 0) return rows.map(row => ({ kind: 'row', row }))

    const items: TableDisplayItem[] = []
    for (const view of organized.groups) {
      items.push({ kind: 'group', view })
      if (!collapsedGroupKeys?.has(view.group.id)) {
        for (const record of view.records) items.push({
          kind: 'row',
          row: rowsById.get(coordinateId(record.coordinate))!,
          group: view.group,
        })
      }
    }
    items.push({ kind: 'ungrouped', records: organized.ungrouped })
    for (const record of organized.ungrouped) items.push({
      kind: 'row',
      row: rowsById.get(coordinateId(record.coordinate))!,
    })
    return items
  }, [rows, recordGroups, collapsedGroupKeys])
  const { visibleRows, visibleRecordRows, visibleCoordinates, visibleRowIndexes } = useMemo(() => {
    const visibleRows: TanStackRow<RecordRow>[] = []
    const visibleRecordRows: RecordRow[] = []
    const visibleCoordinates: RecordCoordinate[] = []
    const visibleRowIndexes = new Map<string, number>()
    // 一次遍历生成导航数据，避免每行临时数组和多轮全表映射。
    for (const item of displayItems) if (item.kind === 'row') {
      visibleRowIndexes.set(coordinateId(item.row.original.coordinate), visibleRows.length)
      visibleRows.push(item.row)
      visibleRecordRows.push(item.row.original)
      visibleCoordinates.push(item.row.original.coordinate)
    }
    return { visibleRows, visibleRecordRows, visibleCoordinates, visibleRowIndexes }
  }, [displayItems])
  const fieldIndexes = useMemo(() => new Map(allFieldNames.map((field, index) => [field, index])), [allFieldNames])
  const selectionIndexes = useMemo(() => ({ rows: visibleRowIndexes, columns: fieldIndexes }), [visibleRowIndexes, fieldIndexes])
  const selectedValueRange = useMemo(() => selection?.filePath === data.file_path
    ? valueSelectionRange(selection, visibleCoordinates, allFieldNames, selectionIndexes) : null,
  [selection, data.file_path, visibleCoordinates, allFieldNames, selectionIndexes])
  const pointerSelectionRef = useRef(false)
  const pointerCellKeyRef = useRef<string | null>(null)
  const pointerStringEditRef = useRef(false)
  const revealFrameRef = useRef(0)
  useEffect(() => {
    const finishPointer = () => { pointerSelectionRef.current = false }
    document.addEventListener('pointerup', finishPointer)
    document.addEventListener('pointercancel', finishPointer)
    return () => {
      document.removeEventListener('pointerup', finishPointer)
      document.removeEventListener('pointercancel', finishPointer)
      cancelAnimationFrame(revealFrameRef.current)
    }
  }, [])
  const cellRangeDrag = useTableCellRangeDrag({
    rootRef: tableScrollRef,
    onSelectCell: (id, field, mode) => {
      const coordinate = visibleCoordinates[visibleRowIndexes.get(id) ?? -1]
      if (coordinate) {
        pointerCellKeyRef.current = tableCellKey(coordinate, field)
        onSelectValue?.(coordinate, [fieldPathField(field)], mode)
      }
    },
  })
  const recordPointerDrag = useRecordPointerDrag({
    rootRef: tableScrollRef,
    records: data.records,
    selectedCoordinates: selection?.filePath === data.file_path
      ? recordSelectionCoordinates(selection)
      : [],
    onSelectDragSource: coordinate => onSelectRecord?.(coordinate, 'replace', visibleCoordinates),
    onDropRecordOntoRecord,
    onDropRecordAfterRecord: !readOnly && data.capabilities.can_reorder_records && sorting.length === 0 && !globalFilter.trim()
      ? onDropRecordAfterRecord
      : undefined,
    onDropRecordIntoGroup,
    onDropRecordIntoUngrouped,
  })
  // 列宽前缀只在布局改变时重建；滚动仅二分定位，跨越列边界时才更新挂载集合。
  const pluginRegistry = usePluginRegistry()
  const pluginCellTypes = useMemo(() => new Set(pluginRegistry.presentations.filter(item => item.slot === 'cell').flatMap(item => item.types)), [pluginRegistry])
  const horizontalColumns = table.getVisibleLeafColumns()
  const columnOffsets = useMemo(() => {
    const offsets = [0]
    horizontalColumns.forEach(column => offsets.push(offsets[offsets.length - 1] + column.getSize() * tableZoom))
    return offsets
  }, [horizontalColumns, columnSizing, tableZoom])
  const [horizontalRange, setHorizontalRange] = useState({ start: 0, end: 6 })
  useEffect(() => {
    const element = tableScrollRef.current
    if (!element) return
    const boundary = (position: number) => {
      let low = 0, high = columnOffsets.length
      while (low < high) {
        const middle = (low + high) >>> 1
        if (columnOffsets[middle] <= position) low = middle + 1
        else high = middle
      }
      return low
    }
    const update = () => {
      const start = Math.max(0, boundary(element.scrollLeft - 160) - 1)
      const end = Math.min(horizontalColumns.length, boundary(element.scrollLeft + element.clientWidth + 160))
      setHorizontalRange(current => current.start === start && current.end === end ? current : { start, end })
    }
    update()
    const observer = new ResizeObserver(update)
    observer.observe(element)
    element.addEventListener('scroll', update, { passive: true })
    return () => { observer.disconnect(); element.removeEventListener('scroll', update) }
  }, [columnOffsets, horizontalColumns])
  const mountedColumnIds = new Set<string>(['id'])
  for (let index = horizontalRange.start; index < horizontalRange.end; index++) {
    if (horizontalColumns[index]) mountedColumnIds.add(horizontalColumns[index].id)
  }
  // 活动单元格保持挂载，横向滚动不丢草稿，键盘导航能直接进入目标编辑器。
  if (selection?.kind === 'value') {
    const field = selectedTopLevelField(selection.fieldPath)
    if (field) mountedColumnIds.add(field)
  }
  // 列窗口及占位区间属于布局，不随记录变化；整张表共用一次分段结果。
  const columnRuns: { start: number; end: number; mounted: boolean; width: number }[] = []
  for (let index = 0; index < horizontalColumns.length;) {
    const start = index
    const needsContent = (position: number) => mountedColumnIds.has(horizontalColumns[position].id)
      || !!rowPresentation || pluginCellTypes.has(columnDeclaredTypes[horizontalColumns[position].id] ?? '')
    const mounted = needsContent(index++)
    if (!mounted) while (index < horizontalColumns.length && !needsContent(index)) index++
    columnRuns.push({ start, end: index, mounted, width: (columnOffsets[index] - columnOffsets[start]) / tableZoom })
  }
  // virtual-core 将 getItemKey 身份纳入测量缓存键；滚动重渲染不能使十万行测量全部失效。
  // 排序、过滤、分组改变 displayItems 时才更新函数，继续正确失效旧布局。
  const virtualRowKey = useCallback((index: number) => {
      const item = displayItems[index]
      if (item?.kind === 'group') return `group:${item.view.group.id}`
      if (item?.kind === 'ungrouped') return 'group:ungrouped'
      return item?.row.id ?? index
  }, [displayItems])
  const rowVirtualizer = useVirtualizer({
    count: displayItems.length,
    getScrollElement: () => tableScrollRef.current,
    estimateSize: index => (displayItems[index]?.kind === 'row' ? ROW_H : GROUP_ROW_H) * tableZoom,
    getItemKey: virtualRowKey,
    overscan: 12,
  })
  useEffect(() => rowVirtualizer.measure(), [rowVirtualizer, tableZoom])
  const virtualRows = rowVirtualizer.getVirtualItems()
  const totalHeight = rowVirtualizer.getTotalSize()
  const padBefore = virtualRows.length > 0 ? virtualRows[0].start : 0
  const padAfter = virtualRows.length > 0 ? totalHeight - virtualRows[virtualRows.length - 1].end : 0

  const revealTableSelection = (target: EditorSelection) => {
    cancelAnimationFrame(revealFrameRef.current)
    if (target.filePath !== data.file_path) return
    const rowIndex = displayItems.findIndex(item => (
      item.kind === 'row' && sameCoordinate(item.row.original.coordinate, target.coordinate)
    ))
    if (rowIndex < 0) return
    const columnId = target.kind === 'record'
      ? 'id'
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
        if (attempts === 1) rowVirtualizer.scrollToIndex(rowIndex, { align: 'auto' })
        revealFrameRef.current = requestAnimationFrame(reveal)
        return
      }
      if (!cell || !scroller) return
      const scrollerRect = scroller.getBoundingClientRect()
      const cellRect = cell.getBoundingClientRect()
      const selectedCells = target.kind === 'value'
        ? Array.from(scroller.querySelectorAll<HTMLElement>('td.selected-cell'))
        : []
      const selectedBounds = selectedCells.length > 0
        ? elementBounds(selectedCells)
        : elementBounds([cell])
      const stickyHeaders = Array.from(scroller.querySelectorAll<HTMLElement>('thead th'))
      const visibleTop = Math.max(
        scrollerRect.top,
        ...stickyHeaders.map(header => header.getBoundingClientRect().bottom),
      )
      const verticalTarget = fitsAxis(
        selectedBounds.top,
        selectedBounds.bottom,
        visibleTop,
        scrollerRect.bottom,
      ) ? { start: selectedBounds.top, end: selectedBounds.bottom } : { start: cellRect.top, end: cellRect.bottom }
      scroller.scrollTop += visibilityScrollDelta(
        { start: visibleTop, end: scrollerRect.bottom },
        verticalTarget,
      )

      const emphasizeArrival = () => {
        if (pendingEmphasis.current !== key) return
        pendingEmphasis.current = null
        emphasize(cell)
      }
      if (columnId === 'id') { emphasizeArrival(); return }
      // Respect the sticky key column and reveal the whole selected range when
      // it fits. Otherwise keep the focused cell visible.
      const keyCol = scroller.querySelector<HTMLElement>('thead .sticky-key-column')
      const leftOccluded = keyCol ? keyCol.getBoundingClientRect().right : scrollerRect.left
      const visibleLeft = Math.max(scrollerRect.left, leftOccluded)
      const visibleRight = scrollerRect.right
      const horizontalTarget = fitsAxis(
        selectedBounds.left,
        selectedBounds.right,
        visibleLeft,
        visibleRight,
      ) ? { start: selectedBounds.left, end: selectedBounds.right } : { start: cellRect.left, end: cellRect.right }
      scroller.scrollLeft += visibilityScrollDelta(
        { start: visibleLeft, end: visibleRight },
        horizontalTarget,
      )
      emphasizeArrival()
    }
    revealFrameRef.current = requestAnimationFrame(reveal)
  }

  useEffect(() => {
    if (!focusRequest || !selection) return
    pointerSelectionRef.current = false
    pointerCellKeyRef.current = null
    tableScrollRef.current?.focus({ preventScroll: true })
    revealTableSelection(selection)
  }, [focusRequest])

  useEffect(() => {
    if (!pendingRecordReveal) return
    if (pendingRecordReveal.filePath !== data.file_path) { setPendingRecordReveal(null); return }
    const target = pendingRecordReveal.coordinate
    if (!data.records.some(row => sameCoordinate(row.coordinate, target))) return
    setPendingRecordReveal(null)
    if (!visibleRowIndexes.has(coordinateId(target))) {
      setCellNotice('记录已创建，当前筛选或分组隐藏了该记录')
      return
    }
    // 等待真实记录进入视图后再选中和定位，不等待或播放虚拟行高度动画。
    pendingEmphasis.current = tableCellKey(target, 'id')
    pointerCellKeyRef.current = null
    onSelectRecord?.(target, 'replace', visibleCoordinates)
    tableScrollRef.current?.focus({ preventScroll: true })
    revealTableSelection(recordSelection(data.file_path, target))
  }, [pendingRecordReveal, data, visibleRowIndexes])

  useEffect(() => {
    if (!pendingRemoval) return
    if (pendingRemoval.filePath !== data.file_path) { setPendingRemoval(null); return }
    if (data.records.some(row => pendingRemoval.deleted.has(coordinateId(row.coordinate)))) return
    setPendingRemoval(null)
    // 只有刷新后的数据证实删除成功，才把焦点交给相邻记录。
    const next = pendingRemoval.next
    if (next && visibleRowIndexes.has(coordinateId(next))) {
      onSelectRecord?.(next, 'replace', visibleCoordinates)
      revealTableSelection(recordSelection(data.file_path, next))
    } else onClearSelection?.()
    tableScrollRef.current?.focus({ preventScroll: true })
  }, [pendingRemoval, data, visibleRowIndexes])

  // 鼠标选择保持点击位置稳定；仅键盘或外部定位负责滚动，拖选由边缘滚动负责。
  const selectionKeyForReveal = selection
    ? `${selection.filePath}::${coordinateId(selection.coordinate)}::${selection.kind === 'value' ? JSON.stringify(selection.fieldPath) : '__record__'}`
    : null
  useEffect(() => {
    if (!selection) return
    const field = selection.kind === 'value' ? selectedTopLevelField(selection.fieldPath) : 'id'
    if (field && pointerCellKeyRef.current === tableCellKey(selection.coordinate, field)) return
    revealTableSelection(selection)
  }, [selectionKeyForReveal])

  useEffect(() => {
    if (selection?.kind !== 'value' || selection.filePath !== data.file_path) return
    if (valueSelectionRange(selection, visibleCoordinates, allFieldNames, selectionIndexes)) return
    const focusVisible = visibleRowIndexes.has(coordinateId(selection.coordinate))
      && !!selectedTopLevelField(selection.fieldPath)
      && allFieldNames.includes(selectedTopLevelField(selection.fieldPath)!)
    const anchorVisible = visibleRowIndexes.has(coordinateId(selection.rangeAnchor.coordinate))
      && !!selectedTopLevelField(selection.rangeAnchor.fieldPath)
      && allFieldNames.includes(selectedTopLevelField(selection.rangeAnchor.fieldPath)!)
    if (focusVisible) onSelectValue?.(selection.coordinate, selection.fieldPath, 'replace')
    else if (anchorVisible) onSelectValue?.(
      selection.rangeAnchor.coordinate,
      selection.rangeAnchor.fieldPath,
      'replace',
    )
    else onClearSelection?.()
  }, [selection, visibleCoordinates, allFieldNames, selectionIndexes, visibleRowIndexes, data.file_path, onSelectValue, onClearSelection])

  useEffect(() => {
    onValueSelectionCellsChange?.(
      selection?.kind === 'value' && selection.filePath === data.file_path
        ? (selectedValueRange ? { rows: visibleCoordinates, columns: allFieldNames, range: selectedValueRange } : [{ coordinate: selection.coordinate, fieldPath: selection.fieldPath }])
        : [],
    )
  }, [selection, selectedValueRange, visibleCoordinates, allFieldNames, data.file_path, onValueSelectionCellsChange])

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

  const contextMenuCanAddToGroup = contextMenu !== null && (
    ((recordGroups?.length ?? 0) > 0 && !!onDropRecordIntoGroup)
    || (contextMenu.records.length > 1 && !!onCreateGroup)
  )

  const fillSelection = async (value: FieldValue) => {
    if (selection?.kind !== 'value' || !onWriteFieldBatch) throw new Error('当前视图不支持批量填充')
    const targets = selectionCellMatrix(selection, visibleCoordinates, allFieldNames).flat().map(anchor => pasteCellFor(anchor, visibleRecordRows, canEdit))
    if (targets.some(cell => !cell.writable || cell.annotation?.read_only)) throw new Error('选区包含只读字段，未写入任何数据')
    await onWriteFieldBatch(targets.map(cell => ({ coordinate: cell.coordinate, field_path: cell.fieldPath, new_value: replacePresentationValue(cell.value, value) })))
  }

  return (
    <DataFillContext.Provider value={fillSelection}><div
      className={`table-view${rowPresentation ? ' table-view-comparison' : ''}`}
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
          onScrollCapture={event => { if (event.target === event.currentTarget) finishOffscreenDataEdit(event.currentTarget) }}
          onScroll={event => {
            if (columnResizeRef.current || scrollWidthFloor === null) return
            const tableWidth = table.getTotalSize() * tableZoom
            if (canReleaseResizeScrollFloor(
              tableWidth,
              event.currentTarget.scrollLeft,
              event.currentTarget.clientWidth,
            )) {
              setScrollWidthFloor(null)
            }
          }}
          onWheel={event => {
            if (!event.ctrlKey) return
            event.preventDefault()
            setScrollWidthFloor(null)
            setTableZoom(current => Math.max(0.7, Math.min(1.6,
              Math.round((current + (event.deltaY < 0 ? 0.1 : -0.1)) * 10) / 10,
            )))
          }}
          tabIndex={0}
          onPointerDown={event => {
            pointerSelectionRef.current = true
            cancelAnimationFrame(revealFrameRef.current)
            cellRangeDrag.onPointerDown(event)
            recordPointerDrag.onPointerDown(event)
          }}
          onClickCapture={event => {
            cellRangeDrag.onClickCapture(event)
            if (!event.isPropagationStopped()) recordPointerDrag.onClickCapture(event)
          }}
          onFocus={e => {
            if (e.target !== e.currentTarget || pointerSelectionRef.current || selection || visibleRows.length === 0) return
            onSelectRecord?.(visibleRows[0].original.coordinate, 'replace', visibleCoordinates)
          }}
          onKeyDown={async e => {
            pointerSelectionRef.current = false
            pointerCellKeyRef.current = null
            if (isNativeEditorTarget(e.target, false)) return
            if (e.key === 'Escape' && pendingCut.current) { pendingCut.current = null; setCellNotice(null); e.preventDefault(); return }
            if (!selection || selection.filePath !== data.file_path) return
            if (e.nativeEvent.isComposing || e.keyCode === 229) return

            if (e.key === 'Enter' && !e.ctrlKey && !e.metaKey && onEnterInspector && selectedValueRange
              && (selectedValueRange.rowStart !== selectedValueRange.rowEnd || selectedValueRange.columnStart !== selectedValueRange.columnEnd)) {
              e.preventDefault()
              onEnterInspector()
              return
            }

            if (e.key === 'Tab' && selection.kind === 'value') {
              const row = (visibleRowIndexes.get(coordinateId(selection.coordinate)) ?? -1)
              const column = allFieldNames.indexOf(selectedTopLevelField(selection.fieldPath) ?? '')
              const next = row * allFieldNames.length + column + (e.shiftKey ? -1 : 1)
              if (next >= 0 && next < visibleCoordinates.length * allFieldNames.length) {
                e.preventDefault()
                const coordinate = visibleCoordinates[Math.floor(next / allFieldNames.length)]
                const path = [fieldPathField(allFieldNames[next % allFieldNames.length])]
                onSelectValue?.(coordinate, path, 'replace')
                revealTableSelection({ ...selection, coordinate, fieldPath: path })
              }
              return
            }

            if (selection.kind === 'value' && !e.ctrlKey && !e.metaKey && !e.altKey
              && (e.key === 'Enter' || e.key === 'F2' || e.key.length === 1)) {
              const field = selectedTopLevelField(selection.fieldPath)
              if (field) {
                const input = tableScrollRef.current?.querySelector<HTMLTextAreaElement>(
                  `[data-table-cell-key="${CSS.escape(tableCellKey(selection.coordinate, field))}"] .cell-string-textarea`,
                )
                const record = visibleRows[visibleRowIndexes.get(coordinateId(selection.coordinate)) ?? -1]
                if (input && record && !isComplexValue(fieldCell(record.original, field)?.value)) {
                  e.preventDefault()
                  input.focus({ preventScroll: true })
                  if (e.key.length === 1) replaceInputText(input, e.key)
                  return
                }
              }
            }

            if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a' && visibleCoordinates.length > 0) {
              e.preventDefault()
              if (selection.kind === 'value' && allFieldNames.length > 0) {
                onSelectValue?.(visibleCoordinates[0], [fieldPathField(allFieldNames[0])], 'replace')
                onSelectValue?.(
                  visibleCoordinates[visibleCoordinates.length - 1],
                  [fieldPathField(allFieldNames[allFieldNames.length - 1])],
                  'range',
                )
              } else {
                const first = visibleCoordinates[0]
                const last = visibleCoordinates[visibleCoordinates.length - 1]
                onSelectRecord?.(first, 'replace', visibleCoordinates)
                onSelectRecord?.(last, 'range', visibleCoordinates)
              }
              return
            }
            if (e.key === 'ArrowLeft' || e.key === 'ArrowRight' || e.key === 'ArrowUp' || e.key === 'ArrowDown') {
              e.preventDefault()
              if (
                selection.kind === 'record'
                && e.shiftKey
                && (e.key === 'ArrowUp' || e.key === 'ArrowDown')
              ) {
                const currentIndex = (visibleRowIndexes.get(coordinateId(selection.coordinate)) ?? -1)
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
                e.shiftKey && selection.kind === 'value',
              )
              if (next !== selection) {
                setSyntaxEdit(null)
                if (next.kind === 'record') onSelectRecord?.(next.coordinate, 'replace', visibleCoordinates)
                else onSelectValue?.(next.coordinate, next.fieldPath, e.shiftKey ? 'range' : 'replace')
                revealTableSelection(next)
              } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp' || e.key === 'ArrowRight') {
                onNavigationBoundary?.(e.key)
              }
              return
            }

            if ((e.key === 'Enter' || e.key === 'F2') && selection.kind === 'value') {
              const field = selectedTopLevelField(selection.fieldPath)
              const dropdown = field ? tableScrollRef.current?.querySelector<HTMLInputElement>(`[data-table-cell-key="${CSS.escape(tableCellKey(selection.coordinate, field))}"] input.searchable-select`) : null
              const record = visibleRows[visibleRowIndexes.get(coordinateId(selection.coordinate)) ?? -1]
              if (dropdown && field && record && !isComplexValue(fieldCell(record.original, field)?.value)) { e.preventDefault(); dropdown.focus({ preventScroll: true }); return }
            }
            if (e.key === 'Enter' && onEnterInspector) {
              if (selection.kind === 'value') {
                const field = selectedTopLevelField(selection.fieldPath)
                const selectedRow = visibleRows[visibleRowIndexes.get(coordinateId(selection.coordinate)) ?? -1]
                const selectedCell = field && selectedRow ? fieldCell(selectedRow.original, field) : undefined
                if (selectedCell && !isComplexValue(selectedCell.value) && selectedCell.value.kind !== 'bool') {
                  e.preventDefault()
                  const input = tableScrollRef.current?.querySelector<HTMLElement>(`[data-table-cell-key="${CSS.escape(tableCellKey(selection.coordinate, field!))}"] textarea, [data-table-cell-key="${CSS.escape(tableCellKey(selection.coordinate, field!))}"] input`)
                  input?.focus({ preventScroll: true })
                  return
                }
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

            const shortcut = (e.ctrlKey || e.metaKey) ? e.key.toLowerCase() : ''
            if ((shortcut === 'c' || shortcut === 'x') && onRenderCellText) {
              e.preventDefault()
              if (clipboardBusyRef.current) return
              clipboardBusyRef.current = true
              try {
                if (selection.kind === 'record') {
                  if (shortcut === 'x') throw new Error('不支持剪切整条记录')
                  const coordinates = recordSelectionCoordinates(selection).map(coordinate => {
                    if (!rowPresentation) return coordinate
                    const row = data.records.find(record => sameCoordinate(record.coordinate, coordinate))
                    return row ? rowPresentation(row, row.container_index)?.coordinate ?? coordinate : coordinate
                  })
                  await writeClipboardText(serializeRecordRefsAsCfd(coordinates))
                } else {
                  const matrix = selectionCellMatrix(selection, visibleCoordinates, allFieldNames)
                  const text = await serializeCfdCellMatrix(matrix, onRenderCellText)
                  await writeClipboardText(text)
                  if (shortcut === 'x') {
                    if (!onWriteFieldBatch) throw new Error('当前来源不支持批量写入')
                    const targets = matrix.flat().map(anchor => pasteCellFor(
                      anchor,
                      visibleRecordRows,
                      canEdit,
                    ))
                    if (targets.some(target => !target.writable || target.annotation?.read_only || (!target.annotation?.nullable && !['string', 'formatted_string', 'array', 'dict'].includes(target.value.kind)))) {
                      throw new Error('剪切区域包含只读或不可清空的单元格')
                    }
                    pendingCut.current = { text, targets }

                  }
                }
                if (shortcut === 'c') pendingCut.current = null
                setCellNotice(shortcut === 'x' ? '已标记剪切，粘贴成功后移除来源；Esc 取消' : null)
              } catch (error) {
                setCellNotice(`${shortcut === 'x' ? '剪切' : '复制'}失败：${errorMessage(error)}`)
              } finally {
                clipboardBusyRef.current = false
              }
              return
            }
            if (shortcut === 'v') {
              e.preventDefault()
              if (clipboardBusyRef.current) return
              if (selection.kind !== 'value') {
                setCellNotice('记录选择模式不支持粘贴')
                return
              }
              if (!onParseCellText || !onWriteFieldBatch) {
                setCellNotice('当前来源不支持批量粘贴')
                return
              }
              clipboardBusyRef.current = true
              try {
                const clipboardText = await readClipboardText()
                if (pendingCut.current?.text !== clipboardText) pendingCut.current = null
                const source = parseCfdClipboard(clipboardText)
                const records = visibleRecordRows
                let anchors = selectionCellMatrix(selection, visibleCoordinates, allFieldNames)
                const selectedTarget = anchors.length === 1 && anchors[0].length === 1
                  ? pasteCellFor(anchors[0][0], records, canEdit)
                  : null
                if (
                  selectedTarget
                  && shouldExpandSinglePasteTarget(source, selectedTarget)
                ) {
                  anchors = boundedPasteMatrix(
                    anchors[0][0],
                    source.length,
                    source[0].length,
                    visibleCoordinates,
                    allFieldNames,
                  )
                }
                const targets = anchors.map(row => row.map(anchor => pasteCellFor(anchor, records, canEdit)))
                const result = await planPaste(source, targets, {
                  parse: onParseCellText,
                  mode: e.shiftKey ? 'append' : 'replace',
                })
                if (!result.ok) {
                  throw new Error(result.errors.map(error => error.message).join('; '))
                }
                let writes = result.writes
                if (pendingCut.current) {
                  const { targets: originals } = pendingCut.current
                  for (const original of originals) {
                    const latest = pasteCellFor({ coordinate: original.coordinate, fieldPath: original.fieldPath }, data.records, canEdit)
                    if (!sameFieldValue(latest.value, original.value)) { pendingCut.current = null; throw new Error('剪切来源已改变，已取消移动') }
                  }
                  const destinations = new Set(targets.flat().map(target => `${coordinateId(target.coordinate)}:${JSON.stringify(target.fieldPath)}`))
                  const clears = originals.filter(source => !destinations.has(`${coordinateId(source.coordinate)}:${JSON.stringify(source.fieldPath)}`)).map(source => ({ coordinate: source.coordinate, field_path: source.fieldPath, new_value: defaultValueForClear(source.annotation, source.value.kind) }))
                  writes = [...clears, ...writes]
                }
                if (writes.length > 0) await onWriteFieldBatch(writes)
                pendingCut.current = null
                setCellNotice(null)
              } catch (error) {
                setCellNotice(`粘贴失败：${errorMessage(error)}`)
              } finally {
                clipboardBusyRef.current = false
              }
              return
            }

            if (selection.kind !== 'value') return
            const modified = e.ctrlKey || e.metaKey || e.altKey

            // Delete key with multi-cell range selection: batch-clear all cells.
            if (e.key === 'Delete' && !modified && canEdit && onWriteFieldBatch) {
              const range = valueSelectionRange(selection, visibleCoordinates, allFieldNames, selectionIndexes)
              const isRange = range && (range.rowEnd > range.rowStart || range.columnEnd > range.columnStart)
              if (isRange) {
                e.preventDefault()
                const cells = selectionCellMatrix(selection, visibleCoordinates, allFieldNames).flat()
                const records = visibleRecordRows
                try {
                  const writes = cells.map(anchor => {
                    const f = selectedTopLevelField(anchor.fieldPath)
                    const row = records[visibleRowIndexes.get(coordinateId(anchor.coordinate)) ?? -1]
                    const cell = f && row ? fieldCell(row, f) : undefined
                    if (!cell || cellReadOnly(cell)) throw new Error('选区包含只读字段，未写入任何数据')
                    const next = defaultValueForClear(cell.annotation, cell.value.kind)
                    return { coordinate: anchor.coordinate, field_path: anchor.fieldPath, new_value: next }
                  })
                  if (writes.length > 0) await onWriteFieldBatch(writes)
                  setCellNotice(null)
                } catch (error) {
                  setCellNotice(`无法编辑：${errorMessage(error)}`)
                }
                return
              }
            }

            const field = selectedTopLevelField(selection.fieldPath)
            if (!field) return
            const selectedRow = visibleRows[visibleRowIndexes.get(coordinateId(selection.coordinate)) ?? -1]
            const selectedCell = selectedRow ? fieldCell(selectedRow.original, field) : undefined
            const editable = !!selectedCell && canEdit && !cellReadOnly(selectedCell)
            const directlyEditable = editable && !isComplexValue(selectedCell?.value)
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

            // Delete on a single cell: works for both simple and collection types.
            if (e.key === 'Delete' && !modified && editable && selectedCell) {
              e.preventDefault()
              try {
                await onWriteField?.(
                  selection.coordinate,
                  selection.fieldPath,
                  defaultValueForClear(selectedCell.annotation, selectedCell.value.kind),
                )
                setCellNotice(null)
              } catch (error) {
                setCellNotice(`无法编辑：${errorMessage(error)}`)
              }
              return
            }

            const intent = directlyEditable && selectedCell
              ? selectionEditIntentForKey(e.key, modified, selectedCell.value.kind)
              : null
            if (!intent) return
            e.preventDefault()
            if (intent.kind === 'toggle-bool') {
              try {
                await onWriteField?.(
                  selection.coordinate,
                  selection.fieldPath,
                  { kind: 'bool', value: !(selectedCell?.value.kind === 'bool' && selectedCell.value.value) },
                )
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
                        onContextMenu={event => {
                          const column = data.columns.find(column => column.name === h.column.id)
                          const eligible = filtered.length > 0
                            ? shortNameCandidate(filtered[0].fields, [fieldPathField(h.column.id)]) !== undefined
                            : column?.type_names.length === 1 && column.type_names[0] === 'string'
                          if (!activeType || !column || !eligible) return
                          event.preventDefault()
                          event.stopPropagation()
                          setShortNameMenu({ x: event.clientX, y: event.clientY, field: column.name })
                        }}
                        className={h.column.id === 'id' ? 'sticky-key-column' : undefined}
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
                              const scroller = tableScrollRef.current
                              if (scroller) {
                                setScrollWidthFloor(resizeScrollWidthFloor(
                                  scroller.scrollWidth,
                                  scroller.scrollLeft,
                                  scroller.clientWidth,
                                ))
                              }
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
                const comparison = rowPresentationRef.current?.(row.original, row.index)
                return (
                  <tr
                    key={row.id}
                    data-index={vr.index}
                    data-coordinate-id={coordinateId(row.original.coordinate)}
                    data-motion-record={motionRecordKey(data.file_path, row.original.coordinate)}
                    data-record-drop-kind="record"
                    ref={rowVirtualizer.measureElement}
                    className={`table-row${selectionMatchesRecord(selection ?? null, data.file_path, row.original.coordinate) ? ' selected' : ''}${item.group?.color ? ' has-group-color' : ''}${rowSev ? ' table-row-' + rowSev : ''}${comparison ? ` table-diff-row table-diff-${comparison.change}${comparison.groupStart ? ' table-diff-group-start' : ''}` : ''}`}
                    data-contains-selection={selectionOwnsRow(selection, data.file_path, row.original.coordinate) || undefined}
                    style={{ ...recordGroupColorStyle(item.group?.color), height: localRowHeights[coordinateId(row.original.coordinate)] !== undefined ? localRowHeights[coordinateId(row.original.coordinate)] * tableZoom : undefined, '--table-row-preview-height': `${localRowHeights[coordinateId(row.original.coordinate)] ?? 132}px` } as React.CSSProperties}
                    onContextMenu={e => {
                      if (readOnly) return
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
                        row: row.original,
                        records,
                      })
                    }}
                  >
                    {columnRuns.map(run => {
                      const cells = row.getVisibleCells(), cell = cells[run.start]
                      // 连续屏外列合并成一个占位单元格，保留列宽并降低 DOM 与布局成本。
                      if (!run.mounted) {
                        let height = 0
                        const id = coordinateId(row.original.coordinate)
                        const previewHeight = (localRowHeights[id] ?? 132) - 12
                        for (let index = run.start; index < run.end && height < previewHeight; index++) {
                          const candidate = cells[index]
                          // 高度达到预览上限后无需再读取其他屏外字段。
                          const field = fieldCell(row.original, candidate.column.id)
                          if (field && isComplexValue(presentationValue(field.value))) {
                            const rows = previewWindow(field.value, field.annotation, expansionStore.get(id), candidate.column.id, Math.ceil(previewHeight / 28)).rows
                            height = Math.max(height, Math.min(previewHeight, rows * 28))
                          }
                        }
                        // 区间宽度复用列前缀，不在每条记录内重复累加列宽。
                        return <td key={`spacer:${cell.id}`} colSpan={run.end - run.start} style={{ width: run.width }} aria-hidden><div style={{ height }} /></td>
                      }
                      const fieldPath = cell.column.id === 'id'
                        ? null
                        : [fieldPathField(cell.column.id)]
                      // 每次选择只计算一次矩形；可见单元格直接比较索引，不展开整个选区。
                      const selectedRowIndex = visibleRowIndexes.get(coordinateId(row.original.coordinate)) ?? -1
                      const selectedColumnIndex = fieldIndexes.get(cell.column.id) ?? -1
                      const selected = fieldPath !== null && selectedValueRange !== null
                        && selectedRowIndex >= selectedValueRange.rowStart && selectedRowIndex <= selectedValueRange.rowEnd
                        && selectedColumnIndex >= selectedValueRange.columnStart && selectedColumnIndex <= selectedValueRange.columnEnd
                      const classes = [
                        pillColumns.has(cell.column.id) ? 'pill-cell' : '',
                        fieldPath && isComplexValue(fieldCell(row.original, cell.column.id)?.value) ? 'complex-cell' : '',
                        selected ? 'selected-cell' : '',
                        selected && selectedValueRange?.rowStart === selectedRowIndex ? 'range-top' : '',
                        selected && selectedValueRange?.rowEnd === selectedRowIndex ? 'range-bottom' : '',
                        selected && selectedValueRange?.columnStart === selectedColumnIndex ? 'range-left' : '',
                        selected && selectedValueRange?.columnEnd === selectedColumnIndex ? 'range-right' : '',
                        comparison?.changedFields.has(cell.column.id) ? 'table-diff-cell-modified' : '',
                      ].filter(Boolean).join(' ')
                      return (
                        <td
                          key={cell.id}
                          className={[classes, cell.column.id === 'id' ? 'sticky-key-column' : ''].filter(Boolean).join(' ') || undefined}
                          data-table-cell-key={tableCellKey(row.original.coordinate, cell.column.id)}
                          data-table-value-cell={fieldPath ? 'true' : undefined}
                          data-coordinate-id={coordinateId(row.original.coordinate)}
                          data-field={fieldPath ? cell.column.id : undefined}
                          data-record-draggable={!fieldPath ? 'true' : undefined}
                          data-record-label={!fieldPath ? recordId(row.original) : undefined}
                          style={{ width: cell.column.getSize() }}
                          aria-selected={selected || undefined}
                          onDoubleClick={event => {
                            if (event.shiftKey || event.ctrlKey || event.metaKey) return
                            if ((event.target as HTMLElement).closest('button, input, textarea, select, a, .table-row-resizer, .dc-row-foldout, .dc-row-field, [contenteditable="true"]')) return
                            if (cell.column.id !== 'id' && !event.currentTarget.classList.contains('complex-cell')) return
                            event.stopPropagation()
                            onExpandCellRef.current?.(row.original.coordinate, cell.column.id === 'id' ? null : [fieldPathField(cell.column.id)], cellMotionOrigin(event.currentTarget))
                          }}
                          onPointerDownCapture={event => {
                            if (event.button !== 0) return
                            // 在拖选控制器更新选择之前记录状态：首次点击只选中，再次点击才编辑。
                            pointerStringEditRef.current = selected
                              && selectedValueRange?.rowStart === selectedValueRange?.rowEnd
                              && selectedValueRange?.columnStart === selectedValueRange?.columnEnd
                              && !event.shiftKey && !event.ctrlKey && !event.metaKey
                            if (!pointerStringEditRef.current
                              && (event.target as HTMLElement).closest('.cell-string-textarea')) {
                              event.preventDefault()
                              tableScrollRef.current?.focus({ preventScroll: true })
                            }
                          }}
                          onClick={event => {
                            if (!fieldPath || event.shiftKey || event.ctrlKey || event.metaKey) return
                            if (!pointerStringEditRef.current) return
                            const target = event.target as HTMLElement
                            if (target.closest('button, a, input, textarea, select, [contenteditable="true"]')) return
                            // 复杂单元格的点击由字段树处理，不能把展开操作转成首个叶子的编辑。
                            if (event.currentTarget.classList.contains('complex-cell')) return
                            // 已选中的字符串单元格再次点击留白进入编辑；拖选结束的 click 已被拦截。
                            event.currentTarget.querySelector<HTMLTextAreaElement>('.cell-string-textarea')?.focus({ preventScroll: true })
                          }}
                          onFocusCapture={e => {
                            if (fieldPath && !selected && isNativeEditorTarget(e.target, false)) {
                              onSelectValue?.(row.original.coordinate, fieldPath, 'replace')
                            }
                          }}
                          onMouseDown={e => {
                            if (e.button !== 0) return
                            // Runs before native selects open, so the inspector
                            // follows the cell even when its editor consumes click.
                            e.stopPropagation()
                            const containsEditor = e.currentTarget.contains(document.activeElement)
                            if (!isNativeEditorTarget(e.target, false)) {
                              if (containsEditor) e.preventDefault()
                              else {
                                // 聚焦滚动容器不能再触发默认首行选择，覆盖 pointerdown 的目标。
                                e.preventDefault()
                                tableScrollRef.current?.focus({ preventScroll: true })
                              }
                            }
                            if (!fieldPath && !containsEditor) {
                              pointerCellKeyRef.current = tableCellKey(row.original.coordinate, 'id')
                              const mode: RecordSelectionMode = e.shiftKey
                                ? 'range'
                                : (e.ctrlKey || e.metaKey ? 'toggle' : 'replace')
                              onSelectRecord?.(row.original.coordinate, mode, visibleCoordinates)
                            }
                          }}
                        >
                          <TableExpansionContext.Provider value={expansionStore}><TablePreviewRows.Provider value={selected ? undefined : Math.max(1, Math.ceil(((localRowHeights[coordinateId(row.original.coordinate)] ?? 132) - 12) / 28) + 1)}>
                            <TableCellContent cell={cell} diagnostics={diagIndex} query={highlightQueryRef.current} syntaxEdit={syntaxEdit} rowPresentation={rowPresentation} writeField={onWriteField} renameRecord={onRenameRecord} diagnosticClick={onDiagnosticBadgeClick} expandCell={onExpandCell} />
                          </TablePreviewRows.Provider></TableExpansionContext.Provider>
                          <div className="table-row-resizer" role="separator" aria-orientation="horizontal" aria-label="调整行高" tabIndex={0}
                            onMouseDown={event => event.stopPropagation()}
                            onDoubleClick={event => {
                              event.stopPropagation()
                              const id = coordinateId(row.original.coordinate)
                              // 仅用户请求适配时遍历完整展开树；包含当前屏外的可见列。
                              let contentHeight = 20
                              for (const column of horizontalColumns) {
                                const field = fieldCell(row.original, column.id)
                                if (field && isComplexValue(presentationValue(field.value))) contentHeight = Math.max(contentHeight,
                                  previewRowCount(field.value, field.annotation, expansionStore.get(id), column.id) * 28)
                              }
                              for (const element of event.currentTarget.closest('tr')!.querySelectorAll<HTMLElement>('.cell-complex-content, .cell-string-preview')) {
                                if (!element.classList.contains('cell-string-preview')) {
                                  // 自定义展示保留真实布局测量；内置树由完整展开行数计算。
                                  if (element.querySelector('.plugin-cell-presentation')) contentHeight = Math.max(contentHeight, element.scrollHeight)
                                } else {
                                  const range = document.createRange(); range.selectNodeContents(element)
                                  contentHeight = Math.max(contentHeight, range.getBoundingClientRect().height / tableZoom)
                                }
                              }
                              resizeRecordRow(id, contentHeight + 12, true)
                            }}
                            onKeyDown={event => { if (event.key === 'Home') { event.preventDefault(); resizeRecordRow(coordinateId(row.original.coordinate), null, true) } else if (event.key === 'ArrowUp' || event.key === 'ArrowDown') { event.preventDefault(); resizeRecordRow(coordinateId(row.original.coordinate), (localRowHeights[coordinateId(row.original.coordinate)] ?? 132) + (event.key === 'ArrowDown' ? 20 : -20), true) } }}
                            onContextMenu={event => { event.preventDefault(); event.stopPropagation(); resizeRecordRow(coordinateId(row.original.coordinate), null, true) }}
                            title="拖动调整行高；双击适配内容；右键恢复默认"
                            onPointerDown={event => {
                              if (event.button !== 0) return
                              event.preventDefault(); event.stopPropagation()
                              const handle = event.currentTarget, start = event.clientY
                              const height = handle.closest('tr')!.getBoundingClientRect().height / tableZoom
                              const id = coordinateId(row.original.coordinate)
                              handle.setPointerCapture(event.pointerId)
                              const move = (pointer: PointerEvent) => resizeRecordRow(id, height + (pointer.clientY - start) / tableZoom, false)
                              const finish = () => { handle.removeEventListener('pointermove', move); handle.removeEventListener('pointerup', finish); handle.removeEventListener('pointercancel', finish); handle.removeEventListener('lostpointercapture', finish); onRowHeightsChange?.(rowHeightsRef.current) }
                              handle.addEventListener('pointermove', move); handle.addEventListener('pointerup', finish); handle.addEventListener('pointercancel', finish); handle.addEventListener('lostpointercapture', finish)
                            }} />
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
          {scrollWidthFloor !== null && (
            <div className="table-scroll-width-floor" style={{ width: scrollWidthFloor }} aria-hidden />
          )}
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
            <button className="btn btn-outlined" onClick={() => { setInsertAfterRow(null); setShowNewRecord(true) }}>
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
          typeOptions={data.type_names}
          existingIds={data.records.map(r => r.coordinate.id)}
          onCreateRecordDraft={onCreateRecordDraft}
          onInsertRecord={async (key, type, fields) => {
            await onInsertRecord(key, type, fields)
            if (insertAfterRow && onMoveRecord) {
              await onMoveRecord({ actual_type: type, id: key}, insertAfterRow.container_index + 1)
            }
            setPendingRecordReveal({ filePath: data.file_path, coordinate: { actual_type: type, id: key } })
          }}
          onClose={() => setShowNewRecord(false)}
        />
      )}

      {shortNameMenu && activeType && <ShortNameColumnMenu key={`${data.file_path}:${activeType}`}
        actualType={activeType} field={shortNameMenu.field} x={shortNameMenu.x} y={shortNameMenu.y}
        onClose={() => setShortNameMenu(null)} />}
      {contextMenu && (
        <RecordContextMenu
          request={{
            anchorX: contextMenu.anchorX,
            anchorY: contextMenu.anchorY,
            filePath: data.file_path,
            coordinates: contextMenu.records,
            primaryId: recordId(contextMenu.row),
          }}
          groups={recordGroups}
          showOpenRecord={!!onOpenRecord}
          canRename={contextMenu.records.length === 1 && !readOnly
            && data.capabilities.can_edit_id && !!onRenameRecord}
          canInsertBelow={contextMenu.records.length === 1 && !readOnly
            && data.capabilities.can_insert_record && data.capabilities.can_reorder_records
            && !!onInsertRecord && !!onCreateRecordDraft && !!onMoveRecord}
          canDelete={!readOnly && data.capabilities.can_delete_record && !!onDeleteRecords}
          canAddToGroup={contextMenuCanAddToGroup}
          canCreateGroup={contextMenu.records.length > 1 && !!onCreateGroup}
          onOpenRecord={onOpenRecord ? () => onOpenRecord(contextMenu.row.coordinate) : undefined}
          onRename={() => {
            setRecordAction({
              kind: 'rename',
              coordinate: contextMenu.row.coordinate,
              id: recordId(contextMenu.row),
            })
          }}
          onInsertBelow={() => {
            setInsertAfterRow(contextMenu.row)
            setShowNewRecord(true)
          }}
          onDelete={() => {
            const records = contextMenu.records
            const prompt = records.length === 1
              ? `确认删除记录 ${recordId(contextMenu.row)}？此操作不可撤销。`
              : `确认删除选中的 ${records.length} 条记录？此操作不可撤销。`
            setRecordAction({ kind: 'delete', records, message: prompt })
          }}
          onCreateGroup={onCreateGroup}
          onAddToGroup={onDropRecordIntoGroup}
          onClose={() => setContextMenu(null)}
        />
      )}
      {recordAction?.kind === 'rename' && onRenameRecord && (
        <TextInputDialog
          title="重命名 ID"
          message="输入新的记录 ID"
          initialValue={recordAction.id}
          confirmLabel="重命名"
          onClose={() => setRecordAction(null)}
          onConfirm={async next => {
            if (next !== recordAction.id) await onRenameRecord(recordAction.coordinate, next)
            setRecordAction(null)
          }}
        />
      )}
      {recordAction?.kind === 'delete' && onDeleteRecords && (
        <ConfirmDialog
          title="删除记录"
          message={recordAction.message}
          confirmLabel="删除"
          danger
          onClose={() => setRecordAction(null)}
          onConfirm={async () => {
            const deleted = new Set(recordAction.records.map(coordinateId))
            const visible = visibleCoordinates.filter(coordinate => !deleted.has(coordinateId(coordinate)))
            const index = visibleCoordinates.findIndex(coordinate => deleted.has(coordinateId(coordinate)))
            const next = visible[Math.min(Math.max(index, 0), visible.length - 1)]
            await onDeleteRecords(recordAction.records)
            setRecordAction(null)
            setPendingRemoval({ filePath: data.file_path, deleted, next })
          }}
        />
      )}
    </div></DataFillContext.Provider>
  )
})

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
