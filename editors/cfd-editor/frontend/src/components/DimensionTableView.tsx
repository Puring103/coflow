import { useEffect, useMemo, useRef, useState } from 'react'
import type { DimensionFileRecords, DimensionFileRow } from '../api'
import type { DimensionValueState } from '../bindings/DimensionValueState'
import type { EditorProjectSettings } from '../bindings/EditorProjectSettings'
import type { EditorRecordGroup } from '../bindings/EditorRecordGroup'
import type { FieldValue } from '../wire'
import { coordinateId, sameCoordinate } from '../wire'
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

function DimensionGrid({ data, onWrite, rootRef, onExitLeft, onExitUp, items, collapsedGroups, onToggleGroup, onRenameGroup, onColorGroup }: Pick<Props, 'data' | 'onWrite' | 'onExitLeft' | 'onExitUp' | 'onRenameGroup' | 'onColorGroup'> & {
  rootRef: React.RefObject<HTMLDivElement | null>
  items: DimensionDisplayItem[]
  collapsedGroups: ReadonlySet<string>
  onToggleGroup: (key: string) => void
}) {
  const [selection, setSelection] = useState<DimensionCellSelection>({ row: 0, column: 0 })
  const columnCount = data.variants.length + 2
  const visibleRows = items.flatMap(item => item.kind === 'row' ? [item.row] : [])
  const select = (next: DimensionCellSelection) => {
    setSelection(next)
    requestAnimationFrame(() => rootRef.current?.querySelector<HTMLElement>(
      `[data-dimension-row="${next.row}"][data-dimension-column="${next.column}"]`,
    )?.scrollIntoView({ block: 'nearest', inline: 'nearest' }))
  }
  return (
    <div
      className="dimension-table-scroll"
      ref={rootRef}
      tabIndex={0}
      onKeyDown={event => {
        if (isNativeEditorTarget(event.target)) return
        if (event.key === 'Enter') {
          event.preventDefault()
          focusDimensionEditor(rootRef.current, selection.column, selection.row)
          return
        }
        if (!isDirection(event.key)) return
        if (event.key === 'ArrowLeft' && selection.column === 0) {
          event.preventDefault()
          onExitLeft?.()
          return
        }
        if (event.key === 'ArrowUp' && selection.row === 0) {
          event.preventDefault()
          onExitUp?.()
          return
        }
        event.preventDefault()
        select(moveDimensionCell(selection, event.key, visibleRows.length, columnCount))
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
                <th scope="row" {...cellProps(rowIndex, 0, selection, select)}>{item.row.coordinate.key}</th>
                <td {...cellProps(rowIndex, 1, selection, select)}><DataCardCompact value={item.row.default_value} label="default" /></td>
                {data.variants.map((variant, variantIndex) => (
                  <td key={variant} {...cellProps(rowIndex, variantIndex + 2, selection, select)}>
                    <DimensionCellEditor row={item.row} variant={variant} onWrite={onWrite} />
                  </td>
                ))}
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
  selection: DimensionCellSelection,
  onSelect: (selection: DimensionCellSelection) => void,
) {
  return {
    'data-dimension-row': row,
    'data-dimension-column': column,
    className: selection.row === row && selection.column === column ? 'keyboard-selected' : undefined,
    onMouseDown: () => onSelect({ row, column }),
  }
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
