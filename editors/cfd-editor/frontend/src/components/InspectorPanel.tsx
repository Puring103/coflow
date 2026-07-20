import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { FileRecords } from '../bindings/FileRecords'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import type { CollectionEdit } from '../bindings/CollectionEdit'
import type { CfdDictKey } from '../bindings/CfdDictKey'
import {
  recordActualType,
  recordKey,
  sameCoordinate,
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
import type { EditorSelection } from '../state/editorSelection'
import { useRecordItemKeyboard } from '../hooks/useRecordItemKeyboard'
import { BatchRecordEditor } from './BatchRecordEditor'

interface Props {
  open: boolean
  collapsed: boolean
  onToggleCollapse: () => void
  data: FileRecords | null
  selection: EditorSelection | null
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
    newKey: string,
  ) => Promise<RecordRow | void>
  onDiagnosticBadgeClick?: (coordinate: RecordCoordinate, fieldPath: string | null) => void
  focusRequest?: number
  onExitKeyboardNavigation?: () => void
}

const MIN_W = 280
const MAX_W = 720

export function InspectorPanel({
  open,
  collapsed,
  onToggleCollapse,
  data,
  selection,
  readOnly,
  diagnostics,
  width,
  onWidthChange,
  onClose,
  onWriteField,
  onWriteFields,
  onRenderCellText,
  onParseCellText,
  onCollectionEdit,
  onRenameRecord,
  onDiagnosticBadgeClick,
  focusRequest,
  onExitKeyboardNavigation,
}: Props) {
  const [dragging, setDragging] = useState(false)
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

  const record = data && coordinate
    ? data.records.find(r => sameCoordinate(r.coordinate, coordinate))
    : null
  const selectedRecords = data && selection?.kind === 'record'
    ? selection.coordinates.flatMap(selected => {
        const row = data.records.find(item => sameCoordinate(item.coordinate, selected))
        return row ? [row] : []
      })
    : []
  const batchRecords = selectedRecords.length > 1 ? selectedRecords : null

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

  const canRename = !readOnly && data?.capabilities.can_edit_key && !!onRenameRecord
  const selectedTopField = selection?.kind === 'value' && selection.fieldPath[0]?.kind === 'field'
    ? selection.fieldPath[0].value
    : null
  const inspectorFields = selectedTopField && record
    ? record.fields.filter(field => field.name === selectedTopField)
    : record?.fields ?? []
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

  const recordKeyboard = useRecordItemKeyboard({
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
    onParseCellText: data && coordinate && onParseCellText
      ? (path, text) => onParseCellText(data.file_path, coordinate, path, text)
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
    const paths = recursivelyExpandablePaths(inspectorFields)
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
      event.preventDefault()
      event.stopPropagation()
      onExitKeyboardNavigation?.()
      return
    }
    recordKeyboard.onKeyDown(event)
  }

  if (!open) return null

  return (
    <aside
      className={`inspector-panel${collapsed ? ' collapsed' : ''}${dragging ? ' dragging' : ''}`}
      style={collapsed ? undefined : { width }}
      aria-label={inspectingValue ? '单元格详情面板' : '记录详情面板'}
    >
      <div
        className="inspector-splitter"
        onMouseDown={collapsed ? undefined : onSplitterDown}
        role="separator"
        aria-orientation="vertical"
        aria-label="调整记录面板宽度"
        tabIndex={collapsed ? -1 : 0}
        onKeyDown={e => {
          if (e.key === 'ArrowLeft') onWidthChange(Math.min(MAX_W, width + 24))
          if (e.key === 'ArrowRight') onWidthChange(Math.max(MIN_W, width - 24))
        }}
      />
      <div className="inspector-head">
        <button
          className="btn btn-icon inspector-collapse-btn"
          onClick={onToggleCollapse}
          title={collapsed ? '展开面板' : '折叠面板'}
          aria-label={collapsed ? '展开面板' : '折叠面板'}
        >
          <Icon name="chevron-right" size={13} className={collapsed ? '' : 'icon-flip-h'} />
        </button>
        {!collapsed && <span className="inspector-title">{inspectingValue ? '单元格详情' : '记录详情'}</span>}
        {!collapsed && (
          <button
            className="btn btn-icon"
            onClick={onClose}
            title="关闭"
            aria-label="关闭记录面板"
          >
            <Icon name="close" size={13} />
          </button>
        )}
      </div>
      {!collapsed && (
        <div
          className="inspector-body"
          ref={bodyRef}
          tabIndex={-1}
          onKeyDown={onBodyKeyDown}
          onContextMenu={event => {
            if (batchRecords || !record || !data) return
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
          ) : record && data ? (
            <>
              {!inspectingValue && (
                <>
                  <CardHeader
                    recordKey={recordKey(record)}
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
                  expandedPaths={expandedPaths}
                  onRowToggle={expansionOwner
                    ? (path, expanded) => {
                      setExpandedByRecord(current => updateExpandedPath(current, expansionOwner, path, expanded))
                    }
                    : undefined}
                  actualType={recordActualType(record)}
                  onEdit={readOnly || !onWriteField
                    ? undefined
                    : (path, val) => { onWriteField(data.file_path, record.coordinate, path, val) }}
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
        </div>
      )}
      {recordTreeMenu && record && data && (
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
                await navigator.clipboard.writeText(await onRenderCellText(data.file_path, record.coordinate, path))
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
                const next = await onParseCellText(data.file_path, record.coordinate, path, text)
                await onWriteField(data.file_path, record.coordinate, path, next)
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
}

function recursivelyExpandablePaths(fields: RecordRow['fields']): Set<string> {
  const paths = new Set<string>()
  for (const field of fields) collectExpandablePaths(field.value, field.name, paths)
  return paths
}

function collectExpandablePaths(value: FieldValue, path: string, paths: Set<string>) {
  if (value.kind !== 'object' && value.kind !== 'array' && value.kind !== 'dict') return
  paths.add(path)
  if (value.kind === 'object') {
    for (const [name, child] of Object.entries(value.value.fields)) {
      if (child) collectExpandablePaths(child, `${path}.${name}`, paths)
    }
  } else if (value.kind === 'array') {
    value.value.forEach((child, index) => collectExpandablePaths(child, `${path}[${index}]`, paths))
  } else {
    for (const [key, child] of value.value) {
      collectExpandablePaths(child, `${path}[${dictKeyText(key)}]`, paths)
    }
  }
}

function dictKeyText(key: CfdDictKey): string {
  if (key.kind === 'string') return `"${key.value}"`
  if (key.kind === 'int') return String(key.value)
  return key.value.variant ?? String(key.value.value)
}

const EMPTY_EXPANDED_PATHS = new Set<string>()

function parseWireFieldPath(raw: string | undefined): FieldPathSegment[] | null {
  if (!raw) return null
  try {
    const value = JSON.parse(raw)
    return Array.isArray(value) ? value as FieldPathSegment[] : null
  } catch {
    return null
  }
}

function isNativeEditorTarget(target: HTMLElement): boolean {
  return target.isContentEditable
    || target.tagName === 'INPUT'
    || target.tagName === 'TEXTAREA'
    || target.tagName === 'SELECT'
    || target.tagName === 'BUTTON'
}
