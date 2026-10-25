import { createPortal } from 'react-dom'
import { flushDataEdits } from '../state/editSession'
import { recordIndex } from '../state/recordIndex'
import { defaultValueForClear } from '../state/selectionKeyboard'
import { readClipboardText, writeClipboardText } from '../api/clipboard'
import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { FileRecords } from '../bindings/FileRecords'
import { ShortNameMenuItem } from './ShortNameContext'
import { shortNameCandidate } from '../state/shortNames'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import type { BatchWriteFieldInput } from '../bindings/BatchWriteFieldInput'
import type { CollectionEdit } from '../bindings/CollectionEdit'
import {
  recordActualType,
  recordId,
  coordinateId,
  errorMessage,
  type DiagnosticItem,
  type FieldPathSegment,
  type FieldValue,
} from '../wire'
import { CardHeader, DataCardExpanded } from './DataCard'
import { Icon } from './Icon'
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
  selectedCellsIdentity,
  editorSelectionIdentity,
  type SelectedCells,
  type CellSelectionRange,
  iterateSelectedCells,
  selectedCellCount,
  type EditorSelection,
} from '../state/editorSelection'
import { useRecordItemKeyboard } from '../hooks/useRecordItemKeyboard'
import {
  PluginContributionMount,
  currentPluginIdentity,
  usePluginPresentation,
} from '../plugins'
import type { PluginPresentationContext } from '../plugins/types'
import { BatchRecordEditor } from './BatchRecordEditor'
import { BatchCellEditor } from './BatchCellEditor'
import { projectBatchCells } from '../state/batchRecordProjection'
import { parseCfdClipboard, pasteCellAtRecordPath, planPaste } from '../state/clipboard'
import { isNativeEditorTarget, parseWireFieldPath } from '../utils/dom'

interface Props {
  modal?: boolean
  origin?: DOMRect | null
  open: boolean
  embedded?: boolean
  data: FileRecords | null
  selection: EditorSelection | null
  valueCells?: SelectedCells
  readOnly?: boolean
  diagnostics?: DiagnosticItem[]
  width: number
  onWidthChange: (w: number) => void
  onClose: () => void
  onWriteField?: (
    filePath: string,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
    newValue: FieldValue,
  ) => Promise<RecordRow | void>
  onWriteFields?: (
    filePath: string,
    coordinates: readonly RecordCoordinate[],
    fieldPath: FieldPathSegment[],
    newValue: FieldValue,
  ) => Promise<void>
  onWriteFieldBatch?: (writes: readonly BatchWriteFieldInput[]) => Promise<void>
  onRenderCellText?: (
    filePath: string,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
  ) => Promise<string>
  onParseCellText?: (
    filePath: string,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
    text: string,
  ) => Promise<FieldValue>
  onCollectionEdit?: (
    filePath: string,
    coordinate: RecordCoordinate,
    fieldPath: FieldPathSegment[],
    edit: CollectionEdit,
  ) => Promise<RecordRow | void>
  onRenameRecord?: (
    filePath: string,
    coordinate: RecordCoordinate,
    newId: string,
  ) => Promise<RecordRow | void>
  onDiagnosticBadgeClick?: (coordinate: RecordCoordinate, fieldPath: string | null) => void
  focusRequest?: number
  onExitKeyboardNavigation?: () => void
  /** Custom view: fields visible by default. undefined => show all (default
   *  views); a set restricts fields, with a header toggle to reveal the rest. */
  visibleFields?: ReadonlySet<string>
}

const MIN_W = 280
const MAX_W = 720

export function InspectorPanel({
  open,
  embedded = false,
  modal = false,
  origin,
  data,
  selection,
  valueCells = [],
  readOnly,
  diagnostics,
  width,
  onWidthChange,
  onClose,
  onWriteField,
  onWriteFields,
  onWriteFieldBatch,
  onRenderCellText,
  onParseCellText,
  onCollectionEdit,
  onRenameRecord,
  onDiagnosticBadgeClick,
  focusRequest,
  onExitKeyboardNavigation,
  visibleFields,
}: Props) {
  const modalRef = useRef<HTMLElement>(null)
  const returnFocusRef = useRef<HTMLElement | null>(null)
  const backdropRef = useRef<HTMLDivElement>(null)
  const closingRef = useRef(false)
  const closeModal = async () => {
    if (closingRef.current) return
    closingRef.current = true
    // 等待真实写入结果，校验或保存失败时保留弹窗和草稿。
    if (!await flushDataEdits()) { closingRef.current = false; return }
    const element = modalRef.current
    if (element && !matchMedia('(prefers-reduced-motion: reduce)').matches) {
      // 从当前视觉帧接续关闭，快速点击也不会跳回展开后的尺寸。
      const visual = getComputedStyle(element)
      const startFrame = { transform: visual.transform, opacity: visual.opacity }
      element.getAnimations().forEach(animation => animation.cancel())
      const rect = element.getBoundingClientRect()
      // 保留关闭末帧直到 React 卸载，避免动画结束恢复原尺寸造成闪烁。
      backdropRef.current?.animate([{ backgroundColor: '#0005' }, { backgroundColor: '#0000' }], { duration: 140, fill: 'forwards' })
      await element.animate([
        startFrame,
        { transform: origin ? `translate(${origin.x - rect.x}px, ${origin.y - rect.y}px) scale(${origin.width / rect.width}, ${origin.height / rect.height})` : 'scale(.97)', opacity: 0 },
      ], { duration: 140, easing: 'ease-in', fill: 'forwards' }).finished.catch(() => {})
    }
    onClose()
    returnFocusRef.current?.focus({ preventScroll: true })
    closingRef.current = false
  }
  useEffect(() => {
    if (!modal || !open) return
    returnFocusRef.current = document.activeElement as HTMLElement | null
    const element = modalRef.current
    if (!element) return
    element.focus({ preventScroll: true })
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    if (!matchMedia('(prefers-reduced-motion: reduce)').matches) {
      const rect = element.getBoundingClientRect()
      element.animate([
        { transform: origin ? `translate(${origin.x - rect.x}px, ${origin.y - rect.y}px) scale(${origin.width / rect.width}, ${origin.height / rect.height})` : 'scale(.97)', opacity: .45 },
        { transform: 'none', opacity: 1 },
      ], { duration: 200, easing: 'cubic-bezier(.2,.8,.2,1)' })
    }
    return () => { document.body.style.overflow = previousOverflow }
  }, [open, modal, origin])
  const [dragging, setDragging] = useState(false)
  // Session-only toggle: in a custom view, reveal all fields (incl. hidden).
  // Not persisted; resets whenever the visible-field set changes (view switch).
  const [showAllFields, setShowAllFields] = useState(false)
  useEffect(() => { setShowAllFields(false) }, [visibleFields])
  const [expandedByRecord, setExpandedByRecord] = useState<ExpandedPathMap>(() => new Map())
  const widthRef = useRef(width)
  const bodyRef = useRef<HTMLDivElement>(null)
  const [keyboardFieldPath, setKeyboardFieldPath] = useState<FieldPathSegment[] | null>(null)
  const [selectedActionPathWire, setSelectedActionPathWire] = useState<string | null>(null)
  const [keyboardNotice, setKeyboardNotice] = useState<string | null>(null)
  const [recordTreeMenu, setRecordTreeMenu] = useState<{
    x: number
    y: number
    path: FieldPathSegment[]
    editable: boolean
  } | null>(null)
  widthRef.current = width
  const coordinate = selection?.coordinate ?? null

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

  const onSplitterDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setDragging(true)
    const startX = e.clientX
    const startW = widthRef.current
    const onMove = (ev: MouseEvent) => {
      const next = Math.min(MAX_W, Math.max(MIN_W, startW - (ev.clientX - startX)))
      onWidthChange(next)
    }
    const onUp = () => {
      setDragging(false)
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }, [onWidthChange])

  // 大选区按坐标索引记录，避免每个选中单元格都重新线性扫描整张表。
  const rowsByCoordinate = useMemo(() => data ? recordIndex(data.records) : new Map<string, RecordRow>(), [data?.records])
  const record = coordinate ? rowsByCoordinate.get(coordinateId(coordinate)) ?? null : null
  const rootPresentation = usePluginPresentation('inspector', record?.coordinate.actual_type ?? '')
  const pluginIdentity = currentPluginIdentity()
  const rootPresentationContext = useMemo<PluginPresentationContext | null>(() => (
    data && record && pluginIdentity && pluginIdentity.revision === data.revision
      ? {
          identity: pluginIdentity,
          filePath: data.file_path,
          coordinate: { ...record.coordinate },
          actualType: record.coordinate.actual_type,
          fieldPath: null,
          declaredType: record.coordinate.actual_type,
        }
      : null
  ), [data, pluginIdentity?.sessionId, pluginIdentity?.revision, record])
  const selectedRecords = useMemo(() => selection?.kind === 'record'
    ? selection.coordinates.flatMap(selected => {
        const row = rowsByCoordinate.get(coordinateId(selected))
        return row ? [row] : []
      }) : [], [selection, rowsByCoordinate])
  const batchRecords = selectedRecords.length > 1 ? selectedRecords : null
  const batchCells = useMemo(() => ({
    *[Symbol.iterator]() {
      if (!data || selection?.kind !== 'value' || selectedCellCount(valueCells) < 2) return
      for (const anchor of iterateSelectedCells(valueCells)) {
        const name = anchor.fieldPath.length === 1 && anchor.fieldPath[0].kind === 'field' ? anchor.fieldPath[0].value : null
        const row = rowsByCoordinate.get(coordinateId(anchor.coordinate))
        const index = name ? row?.field_index[name] : undefined
        const cell = index !== undefined ? row?.fields[index] : undefined
        if (cell) yield { anchor, cell }
      }
    },
  }), [data, rowsByCoordinate, selection?.kind, valueCells])
  const batchProjection = useMemo(() => projectBatchCells({
    *[Symbol.iterator]() {
      if (Array.isArray(valueCells)) { for (const item of batchCells) yield item.cell; return }
      if (!data || selection?.kind !== 'value' || selectedCellCount(valueCells) < 2) return
      const { rows, columns, range } = valueCells as CellSelectionRange
      // 聚合只读取字段值：每行查找一次，不生成每格坐标、路径和包装对象。
      for (let index = range.rowStart; index <= range.rowEnd; index++) {
        const row = rowsByCoordinate.get(coordinateId(rows[index]))
        if (!row) continue
        for (let column = range.columnStart; column <= range.columnEnd; column++) {
          const fieldIndex = row.field_index[columns[column]]
          const cell = fieldIndex === undefined ? undefined : row.fields[fieldIndex]
          if (cell) yield cell
        }
      }
    },
  }), [batchCells, valueCells, data, selection?.kind, rowsByCoordinate])
  const hasBatchCellEditor = !!batchProjection
  const editTargetIdentity = useMemo(() => {
    if (!selection) return 'none'
    if (!hasBatchCellEditor || !data) return editorSelectionIdentity(selection)
    return selectedCellsIdentity(data.file_path, valueCells)
  }, [selection, hasBatchCellEditor, data?.file_path, valueCells])

  const diagnosticIndex = useMemo(
    () => buildRecordDiagnosticIndex(
      data?.records.map(row => ({ filePath: data.file_path, coordinate: row.coordinate })) ?? [],
      diagnostics,
    ),
    [data, diagnostics],
  )
  const diagnosticProjection = record && data
    ? diagnosticsForRecord(
        diagnosticIndex,
        { filePath: data.file_path, coordinate: record.coordinate },
        {
          fieldDiagnostics: record.field_diagnostics,
          severity: record.diagnostic_severity === 'error' || record.diagnostic_severity === 'warning'
            ? record.diagnostic_severity
            : null,
        },
      )
    : { fieldDiagnostics: [], severity: null }
  const fieldDiags = diagnosticProjection.fieldDiagnostics
  const recordSeverity = diagnosticProjection.severity

  const canRename = !readOnly && data?.capabilities.can_edit_id && !!onRenameRecord
  const selectedTopField = selection?.kind === 'value' && selection.fieldPath[0]?.kind === 'field'
    ? selection.fieldPath[0].value
    : null
  // In a custom view, restrict to the view's fields unless the user toggled
  // "show all". A specific selected top field always wins.
  const restrictFields = !!visibleFields && !showAllFields
  const hiddenFieldCount = visibleFields && record
    ? record.fields.filter(field => !visibleFields.has(field.name)).length
    : 0
  const inspectorFields = selectedTopField && record
    ? record.fields.filter(field => field.name === selectedTopField)
    : record
      ? (restrictFields
          ? record.fields.filter(field => visibleFields!.has(field.name))
          : record.fields)
      : []
  const inspectingValue = selection?.kind === 'value'
  const expansionOwner = data && coordinate
    ? `${data.file_path}:${coordinateId(coordinate)}`
    : null
  const expandedPaths = expansionOwner
    ? expandedPathsFor(expandedByRecord, expansionOwner)
    : undefined
  const selectionKey = selection?.kind === 'value'
    ? `${expansionOwner ?? ''}:${JSON.stringify(selection.fieldPath)}`
    : null

  const pasteIntoRecordPath = useCallback(async (
    path: FieldPathSegment[],
    text: string,
    mode: 'replace' | 'append',
  ) => {
    if (!data || !record || !onParseCellText || !onWriteField) {
      throw new Error('当前来源不支持粘贴')
    }
    const target = pasteCellAtRecordPath(record, path, !readOnly)
    if (!target) throw new Error('找不到粘贴目标')
    const result = await planPaste(parseCfdClipboard(text), [[target]], {
      mode,
      parse: (coordinate, fieldPath, cellText) => (
        onParseCellText(data.file_path, coordinate, fieldPath, cellText)
      ),
    })
    if (!result.ok) throw new Error(result.errors.map(error => error.message).join('; '))
    const write = result.writes[0]
    if (write) await onWriteField(data.file_path, write.coordinate, write.field_path, write.new_value)
  }, [data, onParseCellText, onWriteField, readOnly, record])

  const recordKeyboard = useRecordItemKeyboard({
    clearValue: path => {
      const cell = record ? pasteCellAtRecordPath(record, path, !readOnly) : null
      if (!cell || !cell.writable || cell.annotation?.read_only) throw new Error('目标字段不可编辑')
      return defaultValueForClear(cell.annotation, cell.value.kind)
    },
    rootRef: bodyRef,
    selectedFieldPath: selectedActionPathWire ? null : keyboardFieldPath,
    selectedActionPathWire,
    expandedPaths: expandedPaths ?? EMPTY_EXPANDED_PATHS,
    onSelectValue: path => {
      setSelectedActionPathWire(null)
      setKeyboardFieldPath(path)
    },
    onSelectAction: setSelectedActionPathWire,
    onToggleExpansion: (path, expanded) => {
      if (!expansionOwner) return
      setExpandedByRecord(current => updateExpandedPath(current, expansionOwner, path, expanded))
    },
    onRenderCellText: data && coordinate && onRenderCellText
      ? path => onRenderCellText(data.file_path, coordinate, path)
      : undefined,
    onPaste: data && record && onParseCellText && onWriteField
      ? pasteIntoRecordPath
      : undefined,
    onWriteField: data && coordinate && onWriteField
      ? (path, value) => onWriteField(data.file_path, coordinate, path, value)
      : undefined,
    onNotice: setKeyboardNotice,
    onBoundary: edge => {
      if (edge === 'parent') onExitKeyboardNavigation?.()
    },
  })

  useEffect(() => {
    setSelectedActionPathWire(null)
    setKeyboardNotice(null)
    if (!selectionKey || !expansionOwner || selection?.kind !== 'value') {
      setKeyboardFieldPath(null)
      return
    }
    setKeyboardFieldPath(selection.fieldPath)
    // 单元格详情只展开当前字段本身，嵌套结构由用户按需展开，避免深层集合一次铺满面板。
    const paths = topLevelExpandablePaths(inspectorFields)
    setExpandedByRecord(current => {
      let next = current
      for (const path of paths) next = updateExpandedPath(next, expansionOwner, path, true)
      return next
    })
  }, [selectionKey])

  useEffect(() => {
    if (!focusRequest || !selection) return
    bodyRef.current?.focus({ preventScroll: true })
    requestAnimationFrame(() => {
      recordKeyboard.selectFirstItem()
    })
  }, [focusRequest])

  const onBodyKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === 'Escape') {
      if (modal) { event.preventDefault(); event.stopPropagation(); void closeModal(); return }
      event.preventDefault()
      event.stopPropagation()
      onExitKeyboardNavigation?.()
      return
    }
    recordKeyboard.onKeyDown(event)
  }

  if (!open) return null

  const panel = (
    <aside
      ref={modal ? modalRef : undefined}
      tabIndex={modal ? -1 : undefined}
      role={modal ? 'dialog' : undefined}
      aria-modal={modal || undefined}
      onKeyDown={modal ? event => {
        if (event.key === 'Escape' && !event.defaultPrevented) { event.stopPropagation(); void closeModal() }
        if (event.key === 'Tab') {
          const items = [...(modalRef.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), [tabindex="0"]') ?? [])].filter(item => item.getClientRects().length)
          const first = items[0], last = items[items.length - 1]
          if (event.shiftKey && (document.activeElement === first || document.activeElement === modalRef.current)) { event.preventDefault(); last?.focus() }
          else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus() }
        }
      } : undefined}
      className={`inspector-panel${dragging ? ' dragging' : ''}${modal ? ' inspector-modal' : ''}`}
      style={{ width }}
      aria-label={inspectingValue ? '单元格详情面板' : '记录详情面板'}
    >
      {!embedded && !modal && <div
        className="inspector-splitter"
        onMouseDown={onSplitterDown}
        role="separator"
        aria-orientation="vertical"
        aria-label="调整记录面板宽度"
        tabIndex={0}
        onKeyDown={e => {
          if (e.key === 'ArrowLeft') onWidthChange(Math.min(MAX_W, width + 24))
          if (e.key === 'ArrowRight') onWidthChange(Math.max(MIN_W, width - 24))
        }}
      />}
      {!embedded && <div className={`inspector-head${modal ? ' inspector-head-minimal' : ''}`}>
        {!modal && <span className="inspector-title">{inspectingValue ? selectedTopField : coordinate?.id}</span>}
        {visibleFields && !inspectingValue && !selectedTopField && hiddenFieldCount > 0 && (
          <button
            className={`btn btn-icon inspector-showall-btn${showAllFields ? ' active' : ''}`}
            onClick={() => setShowAllFields(show => !show)}
            title={showAllFields ? '仅显示视图字段' : `显示全部字段（含隐藏的 ${hiddenFieldCount} 个）`}
            aria-label={showAllFields ? '仅显示视图字段' : '显示全部字段'}
            aria-pressed={showAllFields}
          >
            <Icon name="filter" size={13} />
          </button>
        )}
        {(
          <button
            className="btn btn-icon"
            onClick={modal ? () => { void closeModal() } : onClose}
            title={modal ? '关闭编辑弹窗' : '清除选择'}
            aria-label={modal ? '关闭编辑弹窗' : '清除详情选择'}
          >
            <Icon name="close" size={13} />
          </button>
        )}
      </div>}
      {(
        <div
          className="inspector-body"
          ref={bodyRef}
          tabIndex={-1}
          onKeyDown={onBodyKeyDown}
          onContextMenu={event => {
            if (batchRecords || hasBatchCellEditor || !record || !data) return
            const row = (event.target as HTMLElement).closest<HTMLElement>('.dc-row[data-field-path-wire]')
            if (!row || !bodyRef.current?.contains(row)) return
            const path = parseWireFieldPath(row.dataset.fieldPathWire)
            if (!path) return
            event.preventDefault()
            event.stopPropagation()
            setSelectedActionPathWire(null)
            setKeyboardFieldPath(path)
            setRecordTreeMenu({
              x: event.clientX,
              y: event.clientY,
              path,
              editable: row.dataset.keyboardEditable === 'true',
            })
          }}
          onMouseDownCapture={event => {
            const target = event.target as HTMLElement
            if (target.closest('.searchable-select-menu') || isNativeEditorTarget(target)) return
            bodyRef.current?.focus({ preventScroll: true })
          }}
        >
          <Fragment key={editTargetIdentity}>
          {batchRecords && data ? (
            <BatchRecordEditor
              records={batchRecords}
              readOnly={readOnly}
              onWriteFields={!onWriteFields
                ? undefined
                : (path, value) => onWriteFields(
                    data.file_path,
                    batchRecords.map(item => item.coordinate),
                    path,
                    value,
                  )}
            />
          ) : hasBatchCellEditor ? (
            <BatchCellEditor
              cells={batchCells}
              projection={batchProjection!}
              readOnly={readOnly}
              onWriteBatch={onWriteFieldBatch}
            />
          ) : record && data && !inspectingValue && rootPresentation?.slot === 'inspector' && rootPresentationContext ? (
            <PluginContributionMount
              contribution={rootPresentation}
              context={rootPresentationContext}
              className="plugin-record-presentation"
            />
          ) : record && data ? (
            <>
              {!embedded && !inspectingValue && (
                <>
                  <CardHeader
                    recordId={recordId(record)}
                    fields={record.fields}
                    actualType={recordActualType(record)}
                    filePath={data.file_path}
                    onRename={canRename && onRenameRecord
                      ? async (next) => { await onRenameRecord(data.file_path, record.coordinate, next) }
                      : undefined}
                    diagSeverity={recordSeverity}
                    onDiagBadgeClick={onDiagnosticBadgeClick
                      ? () => onDiagnosticBadgeClick(record.coordinate, null)
                      : undefined}
                  />
                </>
              )}
              {!inspectingValue || inspectorFields.length > 0 ? (
                <DataCardExpanded
                  fields={inspectorFields}
                  filePath={data.file_path}
                  coordinate={record.coordinate}
                  expandedPaths={expandedPaths}
                  onRowToggle={expansionOwner
                    ? (path, expanded) => {
                      setExpandedByRecord(current => updateExpandedPath(current, expansionOwner, path, expanded))
                    }
                    : undefined}
                  actualType={recordActualType(record)}
                  onEdit={readOnly || !onWriteField
                    ? undefined
                    : (path, val) => onWriteField(data.file_path, record.coordinate, path, val)}
                  onCollectionEdit={readOnly || !onCollectionEdit
                    ? undefined
                    : (path, edit) => { onCollectionEdit(data.file_path, record.coordinate, path, edit) }}
                  diagnostics={fieldDiags}
                  selectedFieldPath={selectedActionPathWire ? null : keyboardFieldPath}
                  selectedActionPathWire={selectedActionPathWire}
                  flattenSingleComplexField={inspectingValue}
                  onSelectValue={path => {
                    setSelectedActionPathWire(null)
                    setKeyboardFieldPath(path)
                  }}
                  onSelectAction={setSelectedActionPathWire}
                  onEditingFinished={() => bodyRef.current?.focus({ preventScroll: true })}
                  onDiagnosticBadgeClick={onDiagnosticBadgeClick
                    ? (topPath) => onDiagnosticBadgeClick(record.coordinate, topPath)
                    : undefined}
                />
              ) : (
                <div className="empty-hint">选中的单元格不存在</div>
              )}
              {keyboardNotice && <span className="table-cell-notice" role="status">{keyboardNotice}</span>}
            </>
          ) : (
            <div className="empty-hint">未选择记录</div>
          )}
          </Fragment>
        </div>
      )}
      {recordTreeMenu && record && data && (
        <div
          className="context-menu record-tree-context-menu"
          style={{ left: recordTreeMenu.x, top: recordTreeMenu.y }}
          role="menu"
          onPointerDown={event => event.stopPropagation()}
        >
          <ShortNameMenuItem actualType={record.coordinate.actual_type}
            field={shortNameCandidate(record.fields, recordTreeMenu.path)}
            onClose={() => setRecordTreeMenu(null)} />
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
                await writeClipboardText(await onRenderCellText(data.file_path, record.coordinate, path))
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
                const text = await readClipboardText()
                await pasteIntoRecordPath(path, text, 'replace')
                setKeyboardNotice(null)
              } catch (error) {
                setKeyboardNotice(`粘贴格式不正确：${errorMessage(error)}`)
              } finally {
                requestAnimationFrame(() => bodyRef.current?.focus({ preventScroll: true }))
              }
            }}
          >
            <Icon name="paste" size={13} aria-hidden />
            粘贴
            <span className="ctx-shortcut">Ctrl+V</span>
          </button>
        </div>
      )}
    </aside>
  )
  return modal ? createPortal(<div ref={backdropRef} className="inspector-modal-backdrop" onClick={event => {
    // 只响应遮罩自身；详情内部及其他 portal 菜单的点击不关闭窗口。
    if (event.target === event.currentTarget) void closeModal()
  }}>{panel}</div>, document.body) : panel
}

function topLevelExpandablePaths(fields: RecordRow['fields']): Set<string> {
  const paths = new Set<string>()
  for (const field of fields) {
    if (field.value.kind === 'object' || field.value.kind === 'array' || field.value.kind === 'dict') {
      paths.add(field.name)
    }
  }
  return paths
}

const EMPTY_EXPANDED_PATHS = new Set<string>()
