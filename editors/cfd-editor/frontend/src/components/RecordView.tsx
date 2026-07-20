import { useState, useEffect, useMemo, useRef } from 'react'
import type { FileRecords } from '../bindings/FileRecords'
import type { CreateRecordDraft } from '../bindings/CreateRecordDraft'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import type { EditorRecordGroup } from '../bindings/EditorRecordGroup'
import type { CollectionEdit } from '../bindings/CollectionEdit'
import {
  coordinateId,
  errorMessage,
  recordActualType,
  recordKey,
  sameCoordinate,
  type DiagnosticItem,
  type FieldPathSegment,
  type FieldValue,
} from '../wire'
import { DataCardExpanded, CardHeader } from './DataCard'
import { CreateRecordDialog } from './CreateRecordDialog'
import { DiagBadge } from './DiagBadge'
import { Icon } from './Icon'
import { typeColor } from '../utils/typeColor'
import { RECORD_HIGHLIGHT_SENTINEL } from '../App'
import { recordMatchesSearch } from '../value/fieldValue'
import {
  expandedPathsFor,
  updateExpandedPath,
  type ExpandedPathMap,
} from '../state/expandedPaths'
import {
  buildRecordDiagnosticIndex,
  diagnosticsForRecord,
} from '../state/recordDiagnostics'
import {
  recordSelectionCoordinates,
  selectionMatchesRecord,
  type EditorSelection,
  type RecordSelectionMode,
} from '../state/editorSelection'
import { useRecordItemKeyboard } from '../hooks/useRecordItemKeyboard'
import { useRecordPointerDrag } from '../hooks/useRecordPointerDrag'
import { organizeRecordRows } from '../state/manualRecordGroups'
import { RecordGroupHeader, RecordUngroupedHeader, recordGroupColorStyle } from './RecordGroupHeader'
import { BatchRecordEditor } from './BatchRecordEditor'

interface Props {
  data: FileRecords
  coordinate: RecordCoordinate
  typeFilter?: string
  readOnly?: boolean
  diagnostics?: DiagnosticItem[]
  /** Filters the sidebar record list (shared global search). */
  recordSearch?: string
  recordGroups?: readonly EditorRecordGroup[]
  collapsedGroupKeys?: ReadonlySet<string>
  onToggleGroup?: (groupKey: string) => void
  onDropRecordOntoRecord?: (sources: readonly RecordCoordinate[], target: RecordCoordinate) => void
  onDropRecordAfterRecord?: (sources: readonly RecordCoordinate[], target: RecordCoordinate) => void
  onDropRecordIntoGroup?: (sources: readonly RecordCoordinate[], groupId: string) => void
  onDropRecordIntoUngrouped?: (sources: readonly RecordCoordinate[]) => void
  onRenameGroup?: (groupId: string, name: string) => void
  onColorGroup?: (groupId: string, color: string | null) => void
  highlightField?: string | null
  onHighlightConsumed?: () => void
  onOpenRecord: (coordinate: RecordCoordinate) => void
  onSelectRecord?: (
    coordinate: RecordCoordinate,
    mode: RecordSelectionMode,
    visibleCoordinates: readonly RecordCoordinate[],
  ) => void
  selection?: EditorSelection | null
  onSelectValue?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[]) => void
  onRenderCellText?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[]) => Promise<string>
  onParseCellText?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], text: string) => Promise<FieldValue>
  onWriteField?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], newValue: FieldValue) => Promise<RecordRow | void>
  onWriteFields?: (coordinates: readonly RecordCoordinate[], fieldPath: FieldPathSegment[], newValue: FieldValue) => Promise<void>
  onCollectionEdit?: (coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], edit: CollectionEdit) => Promise<RecordRow | void>
  onRenameRecord?: (coordinate: RecordCoordinate, newKey: string) => Promise<RecordRow | void>
  onInsertRecord?: (recordKey: string, actualType: string, fields: FieldValue) => Promise<void>
  onCreateRecordDraft?: (actualType: string) => Promise<CreateRecordDraft>
  /** Click on a corner badge — either the CardHeader (fieldPath = null) or
   *  a field row (top-level fieldPath). Forwarded up to App so the
   *  diagnostics panel can focus the matching item. */
  onDiagnosticBadgeClick?: (coordinate: RecordCoordinate, fieldPath: string | null) => void
  onExitLeft?: () => void
  onExitUp?: () => void
  firstRecordFocusRequest?: number
  onFirstRecordFocusConsumed?: (request: number) => void
}

export function RecordView({ data, coordinate, typeFilter, readOnly, diagnostics, recordSearch, recordGroups, collapsedGroupKeys, onToggleGroup, onDropRecordOntoRecord, onDropRecordAfterRecord, onDropRecordIntoGroup, onDropRecordIntoUngrouped, onRenameGroup, onColorGroup, highlightField, onHighlightConsumed, onOpenRecord, onSelectRecord, selection, onSelectValue, onRenderCellText, onParseCellText, onWriteField, onWriteFields, onCollectionEdit, onRenameRecord, onInsertRecord, onCreateRecordDraft, onDiagnosticBadgeClick, onExitLeft, onExitUp, firstRecordFocusRequest, onFirstRecordFocusConsumed }: Props) {
  const record = data.records.find(r => sameCoordinate(r.coordinate, coordinate))
  const [fieldSearch, setFieldSearch] = useState('')
  const [showNewRecord, setShowNewRecord] = useState(false)
  const [expandedByRecord, setExpandedByRecord] = useState<ExpandedPathMap>(() => new Map())
  const [selectedActionPathWire, setSelectedActionPathWire] = useState<string | null>(null)
  const [keyboardNotice, setKeyboardNotice] = useState<string | null>(null)
  const [recordTreeMenu, setRecordTreeMenu] = useState<{
    x: number
    y: number
    path: FieldPathSegment[]
    editable: boolean
  } | null>(null)
  const fieldSearchRef = useRef<HTMLInputElement>(null)
  const sidebarRef = useRef<HTMLDivElement>(null)
  const mainRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!recordTreeMenu) return
    const close = (event: PointerEvent) => {
      if (event.target instanceof Element && event.target.closest('.record-tree-context-menu')) return
      setRecordTreeMenu(null)
    }
    const closeOnBlur = () => setRecordTreeMenu(null)
    window.addEventListener('pointerdown', close)
    window.addEventListener('blur', closeOnBlur)
    return () => {
      window.removeEventListener('pointerdown', close)
      window.removeEventListener('blur', closeOnBlur)
    }
  }, [recordTreeMenu])

  const activeId = coordinateId(coordinate)
  const expansionOwner = `${data.file_path}:${activeId}`
  const expandedPaths = expandedPathsFor(expandedByRecord, expansionOwner)
  const selectedFieldPath = selection?.kind === 'value'
    && selection.filePath === data.file_path
    && sameCoordinate(selection.coordinate, coordinate)
    ? selection.fieldPath
    : null

  useEffect(() => setSelectedActionPathWire(null), [expansionOwner])

  // Record-level highlight burns off after the header flashes — the child
  // DataCardExpanded only clears the highlight for field-level jumps.
  useEffect(() => {
    if (highlightField !== RECORD_HIGHLIGHT_SENTINEL) return
    const t = setTimeout(() => onHighlightConsumed?.(), 1600)
    return () => clearTimeout(t)
  }, [highlightField, onHighlightConsumed])

  const diagnosticIndex = useMemo(
    () => buildRecordDiagnosticIndex(
      data.records.map(row => ({ filePath: data.file_path, coordinate: row.coordinate })),
      diagnostics,
    ),
    [data.file_path, data.records, diagnostics],
  )

  const allSidebarRecords = typeFilter
    ? data.records.filter(r => recordActualType(r) === typeFilter)
    : data.records

  const sidebarRecords = recordSearch
    ? allSidebarRecords.filter(record => recordMatchesSearch(record, recordSearch))
    : allSidebarRecords
  const organizedRecords = useMemo(
    () => organizeRecordRows(sidebarRecords, recordGroups ?? []),
    [recordGroups, sidebarRecords],
  )
  const visibleSidebarRecords = [
    ...organizedRecords.groups.flatMap(view => collapsedGroupKeys?.has(view.group.id) ? [] : view.records),
    ...organizedRecords.ungrouped,
  ]
  const visibleCoordinates = visibleSidebarRecords.map(row => row.coordinate)
  const selectedRecords = selection?.kind === 'record' && selection.filePath === data.file_path
    ? selection.coordinates.flatMap(selected => {
        const row = data.records.find(item => sameCoordinate(item.coordinate, selected))
        return row ? [row] : []
      })
    : []
  const batchRecords = selectedRecords.length > 1 ? selectedRecords : null
  const recordPointerDrag = useRecordPointerDrag({
    rootRef: sidebarRef,
    records: data.records,
    selectedCoordinates: selection?.filePath === data.file_path
      ? recordSelectionCoordinates(selection)
      : [],
    onSelectDragSource: source => onSelectRecord?.(source, 'replace', visibleCoordinates),
    onDropRecordOntoRecord,
    onDropRecordAfterRecord: !readOnly && data.capabilities.can_reorder_records ? onDropRecordAfterRecord : undefined,
    onDropRecordIntoGroup,
    onDropRecordIntoUngrouped,
  })

  useEffect(() => {
    if (!firstRecordFocusRequest) return
    const first = visibleSidebarRecords[0]
    if (first) {
      onOpenRecord(first.coordinate)
      onSelectRecord?.(first.coordinate, 'replace', visibleCoordinates)
      requestAnimationFrame(() => {
        sidebarRef.current?.querySelector<HTMLElement>(
          `[data-coordinate-id="${cssEscape(coordinateId(first.coordinate))}"]`,
        )?.focus({ preventScroll: true })
      })
    }
    onFirstRecordFocusConsumed?.(firstRecordFocusRequest)
  }, [firstRecordFocusRequest])

  if (!record) {
    return <div className="record-view"><div className="empty-hint">记录 "{coordinate.actual_type}.{coordinate.key}" 未找到</div></div>
  }

  const fields = fieldSearch
    ? record.fields.filter(f => f.name.toLowerCase().includes(fieldSearch.toLowerCase()))
    : record.fields

  const diagnosticProjection = (row: RecordRow) => diagnosticsForRecord(
    diagnosticIndex,
    { filePath: data.file_path, coordinate: row.coordinate },
    {
      fieldDiagnostics: row.field_diagnostics,
      severity: row.diagnostic_severity === 'error' || row.diagnostic_severity === 'warning'
        ? row.diagnostic_severity
        : null,
    },
  )
  const fieldDiags = diagnosticProjection(record).fieldDiagnostics
  const canRename = !readOnly && data.capabilities.can_edit_key && !!onRenameRecord
  const rowSeverity = (row: RecordRow): 'error' | 'warning' | null =>
    diagnosticProjection(row).severity

  const recordKeyboard = useRecordItemKeyboard({
    rootRef: mainRef,
    selectedFieldPath,
    selectedActionPathWire,
    expandedPaths,
    onSelectValue: path => onSelectValue?.(coordinate, path),
    onSelectAction: setSelectedActionPathWire,
    onToggleExpansion: (path, expanded) => {
      setExpandedByRecord(current => updateExpandedPath(current, expansionOwner, path, expanded))
    },
    onRenderCellText: onRenderCellText
      ? path => onRenderCellText(coordinate, path)
      : undefined,
    onParseCellText: onParseCellText
      ? (path, text) => onParseCellText(coordinate, path, text)
      : undefined,
    onWriteField: onWriteField
      ? (path, value) => onWriteField(coordinate, path, value)
      : undefined,
    onNotice: setKeyboardNotice,
    onBoundary: edge => {
      if (edge === 'before') {
        fieldSearchRef.current?.focus({ preventScroll: true })
        fieldSearchRef.current?.select()
      } else {
        sidebarRef.current?.querySelector<HTMLElement>(
          `[data-coordinate-id="${cssEscape(activeId)}"]`,
        )?.focus({ preventScroll: true })
      }
    },
  })

  const onSidebarKeyDown = (e: React.KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a' && visibleCoordinates.length > 0) {
      e.preventDefault()
      onSelectRecord?.(visibleCoordinates[0], 'replace', visibleCoordinates)
      onSelectRecord?.(visibleCoordinates[visibleCoordinates.length - 1], 'range', visibleCoordinates)
      return
    }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp' && e.key !== 'ArrowLeft' && e.key !== 'ArrowRight' && e.key !== 'Enter') return
    const ids = visibleSidebarRecords.map(r => coordinateId(r.coordinate))
    if (ids.length === 0) return
    const cur = document.activeElement as HTMLElement | null
    const idx = Math.max(0, ids.indexOf(activeId))
    if (e.key === 'ArrowLeft') {
      e.preventDefault()
      onExitLeft?.()
    } else if (e.key === 'ArrowRight') {
      e.preventDefault()
      const first = mainRef.current?.querySelector<HTMLElement>('.dc-row[data-field-path-wire]')
      if (first) {
        selectRecordItem(first, record.coordinate, onSelectValue, setSelectedActionPathWire)
        mainRef.current?.focus({ preventScroll: true })
        first.scrollIntoView({ block: 'nearest' })
      }
    } else if (e.key === 'ArrowDown') {
      e.preventDefault()
      const next = ids[Math.min(idx + 1, ids.length - 1)]
      const nextRecord = visibleSidebarRecords.find(r => coordinateId(r.coordinate) === next)
      if (nextRecord && next !== activeId) {
        onOpenRecord(nextRecord.coordinate)
        onSelectRecord?.(nextRecord.coordinate, e.shiftKey ? 'range' : 'replace', visibleCoordinates)
      }
      requestAnimationFrame(() => sidebarRef.current?.querySelector<HTMLElement>(`[data-coordinate-id="${cssEscape(next)}"]`)?.focus())
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      if (idx === 0) {
        onExitUp?.()
        return
      }
      const prev = ids[Math.max(idx - 1, 0)]
      const previousRecord = visibleSidebarRecords.find(r => coordinateId(r.coordinate) === prev)
      if (previousRecord && prev !== activeId) {
        onOpenRecord(previousRecord.coordinate)
        onSelectRecord?.(previousRecord.coordinate, e.shiftKey ? 'range' : 'replace', visibleCoordinates)
      }
      requestAnimationFrame(() => sidebarRef.current?.querySelector<HTMLElement>(`[data-coordinate-id="${cssEscape(prev)}"]`)?.focus())
    } else if (e.key === 'Enter') {
      const id = cur?.dataset.coordinateId
      const next = id ? visibleSidebarRecords.find(r => coordinateId(r.coordinate) === id) : null
      if (next) {
        e.preventDefault()
        onOpenRecord(next.coordinate)
        onSelectRecord?.(next.coordinate, 'replace', visibleCoordinates)
      }
    }
  }

  const newRecordType = typeFilter || recordActualType(record)
  const canCreate = !readOnly
    && data.capabilities.can_insert_record
    && !!onInsertRecord
    && !!onCreateRecordDraft
    && !!newRecordType

  const renderSidebarRecord = (row: RecordRow, group?: EditorRecordGroup) => {
    const sev = rowSeverity(row)
    const id = coordinateId(row.coordinate)
    const selected = selectionMatchesRecord(selection ?? null, data.file_path, row.coordinate)
    return (
      <div
        key={id}
        className={`rv-sidebar-item${selected ? ' selected' : ''}${group?.color ? ' has-group-color' : ''}${sev ? ' rv-sidebar-' + sev : ''}`}
        role="option"
        aria-selected={selected}
        tabIndex={id === activeId ? 0 : -1}
        data-coordinate-id={id}
        data-record-draggable="true"
        data-record-drop-kind="record"
        data-record-label={recordKey(row)}
        style={{
          '--type-color': typeColor(recordActualType(row)),
          ...recordGroupColorStyle(group?.color),
        } as React.CSSProperties}
        onClick={event => {
          const mode: RecordSelectionMode = event.shiftKey
            ? 'range'
            : (event.ctrlKey || event.metaKey ? 'toggle' : 'replace')
          onSelectRecord?.(row.coordinate, mode, visibleCoordinates)
          onOpenRecord(row.coordinate)
        }}
        onKeyDown={event => {
          if (event.key === 'Enter') {
            event.preventDefault()
            event.stopPropagation()
            onSelectRecord?.(row.coordinate, 'replace', visibleCoordinates)
            onOpenRecord(row.coordinate)
          }
        }}
      >
        <Icon name="grip" size={13} className="record-drag-handle rv-record-drag-handle" aria-hidden />
        <span className="rv-item-key">{recordKey(row)}</span>
        {firstScalarSummary(row) && (
          <span className="rv-item-subtitle">{firstScalarSummary(row)}</span>
        )}
        {(sev === 'error' || sev === 'warning') && (
          <DiagBadge
            severity={sev}
            onClick={onDiagnosticBadgeClick
              ? () => { onOpenRecord(row.coordinate); onDiagnosticBadgeClick(row.coordinate, null) }
              : undefined}
          />
        )}
      </div>
    )
  }

  return (
    <div className="record-view">
      <div className="rv-sidebar-wrap">
        <div
          className="rv-sidebar"
          role="listbox"
          aria-multiselectable="true"
          aria-label="记录列表"
          onKeyDown={onSidebarKeyDown}
          onPointerDown={recordPointerDrag.onPointerDown}
          onClickCapture={recordPointerDrag.onClickCapture}
          ref={sidebarRef}
        >
          {organizedRecords.groups.map(view => {
                const collapsed = collapsedGroupKeys?.has(view.group.id) ?? false
                return (
                  <div className="record-group" key={view.group.id} role="group" aria-label={view.group.name}>
                    <RecordGroupHeader
                      name={view.group.name}
                      groupId={view.group.id}
                      count={view.records.length}
                      collapsed={collapsed}
                      color={view.group.color}
                      className="rv-record-group-header"
                      onToggle={() => onToggleGroup?.(view.group.id)}
                      onRename={name => onRenameGroup?.(view.group.id, name)}
                      onColorChange={color => onColorGroup?.(view.group.id, color)}
                    />
                    {!collapsed && view.records.map(row => renderSidebarRecord(row, view.group))}
                  </div>
                )
              })}
          {organizedRecords.groups.length > 0 && (
            <RecordUngroupedHeader
              count={organizedRecords.ungrouped.length}
              className="rv-record-group-header"
            />
          )}
          {organizedRecords.ungrouped.map(row => renderSidebarRecord(row))}
        </div>
        {canCreate && (
          <div className="rv-sidebar-footer">
            <button className="btn btn-outlined rv-sidebar-new" onClick={() => setShowNewRecord(true)}>
              <Icon name="plus" size={13} />
              新建记录
            </button>
          </div>
        )}
      </div>

      <div
        className="rv-main"
        ref={mainRef}
        tabIndex={0}
        onKeyDown={batchRecords ? undefined : recordKeyboard.onKeyDown}
        onContextMenu={event => {
          if (batchRecords) return
          const row = (event.target as HTMLElement).closest<HTMLElement>('.dc-row[data-field-path-wire]')
          if (!row || !mainRef.current?.contains(row)) return
          const path = parseWireFieldPath(row.dataset.fieldPathWire)
          if (!path) return
          event.preventDefault()
          event.stopPropagation()
          setSelectedActionPathWire(null)
          onSelectValue?.(record.coordinate, path)
          setRecordTreeMenu({
            x: event.clientX,
            y: event.clientY,
            path,
            editable: row.dataset.keyboardEditable === 'true',
          })
        }}
        onMouseDownCapture={e => {
          const target = e.target as HTMLElement
          const addRow = target.closest('.dc-row-add[data-add-path-wire]')
          if (addRow) e.preventDefault()
          if (addRow || (target.closest('.dc-row[data-field-path-wire]') && !isNativeEditorTarget(target))) {
            mainRef.current?.focus({ preventScroll: true })
          }
        }}
      >
        {batchRecords ? (
          <BatchRecordEditor
            records={batchRecords}
            readOnly={readOnly}
            onWriteFields={!onWriteFields
              ? undefined
              : (path, value) => onWriteFields(
                  batchRecords.map(item => item.coordinate),
                  path,
                  value,
                )}
          />
        ) : <>
        <CardHeader
          recordKey={recordKey(record)}
          actualType={recordActualType(record)}
          filePath={data.file_path}
          onRename={canRename ? async (next) => { await onRenameRecord!(record.coordinate, next) } : undefined}
          diagSeverity={rowSeverity(record)}
          onDiagBadgeClick={onDiagnosticBadgeClick ? () => onDiagnosticBadgeClick(record.coordinate, null) : undefined}
          highlight={highlightField === RECORD_HIGHLIGHT_SENTINEL}
        />
        <div className="rv-search-bar">
          <Icon name="search" size={13} className="rv-search-icon" aria-hidden />
          <input
            ref={fieldSearchRef}
            placeholder="搜索字段…"
            value={fieldSearch}
            onChange={e => setFieldSearch(e.target.value)}
            onKeyDown={e => {
              if (e.key === 'ArrowUp') {
                e.preventDefault()
                onExitUp?.()
                return
              }
              if (e.key === 'ArrowLeft' && e.currentTarget.selectionStart === 0) {
                e.preventDefault()
                sidebarRef.current?.querySelector<HTMLElement>(
                  `[data-coordinate-id="${cssEscape(activeId)}"]`,
                )?.focus({ preventScroll: true })
                return
              }
              if (e.key !== 'ArrowDown') return
              const first = mainRef.current?.querySelector<HTMLElement>('.dc-row[data-field-path-wire]')
              if (!first) return
              e.preventDefault()
              selectRecordItem(first, record.coordinate, onSelectValue, setSelectedActionPathWire)
              mainRef.current?.focus({ preventScroll: true })
              first.scrollIntoView({ block: 'nearest' })
            }}
            aria-label="搜索字段"
          />
          {fieldSearch && (
            <button className="rv-clear-search" onClick={() => setFieldSearch('')} aria-label="清除搜索">
              <Icon name="close" size={13} aria-hidden />
            </button>
          )}
        </div>
        <DataCardExpanded
          fields={fields}
          expandedPaths={expandedPaths}
          onRowToggle={(path, expanded) => {
            setExpandedByRecord(current => updateExpandedPath(current, expansionOwner, path, expanded))
          }}
          actualType={recordActualType(record)}
          onEdit={readOnly || !onWriteField ? undefined : (path, val) => { onWriteField(record.coordinate, path, val) }}
          onCollectionEdit={readOnly || !onCollectionEdit ? undefined : (path, edit) => { onCollectionEdit(record.coordinate, path, edit) }}
          diagnostics={fieldDiags}
          highlightField={highlightField === RECORD_HIGHLIGHT_SENTINEL ? null : highlightField}
          expandAlongPath={highlightField && highlightField !== RECORD_HIGHLIGHT_SENTINEL ? highlightField : null}
          onHighlightConsumed={onHighlightConsumed}
          selectedFieldPath={selectedActionPathWire ? null : selectedFieldPath}
          selectedActionPathWire={selectedActionPathWire}
          onSelectValue={path => {
            setSelectedActionPathWire(null)
            onSelectValue?.(record.coordinate, path)
          }}
          onSelectAction={setSelectedActionPathWire}
          onEditingFinished={() => mainRef.current?.focus({ preventScroll: true })}
          onDiagnosticBadgeClick={onDiagnosticBadgeClick
            ? (topPath) => onDiagnosticBadgeClick(record.coordinate, topPath)
            : undefined}
        />
        {keyboardNotice && <span className="table-cell-notice" role="status">{keyboardNotice}</span>}
        </>}
      </div>
      {recordTreeMenu && (
        <div
          className="context-menu record-tree-context-menu"
          style={{ left: recordTreeMenu.x, top: recordTreeMenu.y }}
          role="menu"
          onPointerDown={event => event.stopPropagation()}
        >
          <button
            type="button"
            className="ctx-item"
            role="menuitem"
            disabled={!onRenderCellText}
            onClick={async () => {
              const path = recordTreeMenu.path
              setRecordTreeMenu(null)
              try {
                if (!onRenderCellText) return
                await navigator.clipboard.writeText(await onRenderCellText(record.coordinate, path))
                setKeyboardNotice(null)
              } catch (error) {
                setKeyboardNotice(`复制失败：${errorMessage(error)}`)
              }
            }}
          >
            <Icon name="copy" size={13} aria-hidden />
            复制
            <span className="ctx-shortcut">Ctrl+C</span>
          </button>
          <button
            type="button"
            className="ctx-item"
            role="menuitem"
            disabled={!recordTreeMenu.editable || !onParseCellText || !onWriteField}
            onClick={async () => {
              const path = recordTreeMenu.path
              setRecordTreeMenu(null)
              try {
                if (!onParseCellText || !onWriteField) return
                const text = await navigator.clipboard.readText()
                const next = await onParseCellText(record.coordinate, path, text)
                await onWriteField(record.coordinate, path, next)
                setKeyboardNotice(null)
              } catch (error) {
                setKeyboardNotice(`粘贴格式不正确：${errorMessage(error)}`)
              } finally {
                requestAnimationFrame(() => mainRef.current?.focus({ preventScroll: true }))
              }
            }}
          >
            <Icon name="paste" size={13} aria-hidden />
            粘贴
            <span className="ctx-shortcut">Ctrl+V</span>
          </button>
        </div>
      )}
      {showNewRecord && onInsertRecord && onCreateRecordDraft && (
        <CreateRecordDialog
          actualType={newRecordType}
          existingKeys={data.records.map(r => r.coordinate.key)}
          onCreateRecordDraft={onCreateRecordDraft}
          onInsertRecord={onInsertRecord}
          onClose={() => setShowNewRecord(false)}
        />
      )}
    </div>
  )
}

function cssEscape(s: string): string {
  if (typeof CSS !== 'undefined' && typeof CSS.escape === 'function') return CSS.escape(s)
  return s.replace(/["\\]/g, '\\$&')
}

function selectRecordItem(
  element: HTMLElement,
  coordinate: RecordCoordinate,
  onSelectValue: Props['onSelectValue'],
  setSelectedActionPathWire: (value: string | null) => void,
) {
  const actionPath = element.dataset.addPathWire
  if (actionPath) {
    setSelectedActionPathWire(actionPath)
    return
  }
  const path = parseWireFieldPath(element.dataset.fieldPathWire)
  if (!path) return
  setSelectedActionPathWire(null)
  onSelectValue?.(coordinate, path)
}

function parseWireFieldPath(raw: string | undefined): FieldPathSegment[] | null {
  if (!raw) return null
  try {
    const value = JSON.parse(raw)
    return Array.isArray(value) ? value as FieldPathSegment[] : null
  } catch {
    return null
  }
}

function isNativeEditorTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  return target.isContentEditable
    || target.tagName === 'INPUT'
    || target.tagName === 'TEXTAREA'
    || target.tagName === 'SELECT'
    || target.tagName === 'BUTTON'
}

/** First scalar field's summary — used as the sidebar row subtitle so the
 *  user can eyeball each record without opening it. Scalar = bool/int/float/
 *  string/enum/ref; nested collections/objects don't produce a useful preview. */
function firstScalarSummary(row: RecordRow): string | null {
  for (const field of row.fields) {
    const kind = field.value.kind
    if (kind === 'bool' || kind === 'int' || kind === 'float'
        || kind === 'string' || kind === 'enum' || kind === 'ref') {
      const summary = row.field_summaries[field.name]
      if (summary) return summary
    }
  }
  return null
}

