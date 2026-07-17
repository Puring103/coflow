import { useEffect, useRef, useState } from 'react'
import type { DimensionFileRecords, DimensionFileRow } from '../api'
import type { DimensionValueState } from '../bindings/DimensionValueState'
import type { FieldValue } from '../wire'
import { DataCardCompact, DirectEditor, InlineEditor } from './DataCard'
import { Icon } from './Icon'

interface Props {
  data: DimensionFileRecords
  mode: 'table' | 'record'
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
  onWrite,
  onExitLeft,
  onExitUp,
  focusRequest = 0,
  onFocusRequestConsumed,
}: Props) {
  const [selectedKey, setSelectedKey] = useState(data.rows[0]?.coordinate.key ?? '')
  const [recordField, setRecordField] = useState(0)
  const tableRef = useRef<HTMLDivElement>(null)
  const listRef = useRef<HTMLElement>(null)
  const recordRef = useRef<HTMLElement>(null)
  useEffect(() => {
    if (!data.rows.some(row => row.coordinate.key === selectedKey)) {
      setSelectedKey(data.rows[0]?.coordinate.key ?? '')
    }
  }, [data, selectedKey])
  const selected = data.rows.find(row => row.coordinate.key === selectedKey) ?? data.rows[0]

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
              const index = Math.max(0, data.rows.findIndex(row => row.coordinate.key === selectedKey))
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
              const next = Math.max(0, Math.min(data.rows.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)))
              const row = data.rows[next]
              if (!row) return
              setSelectedKey(row.coordinate.key)
              requestAnimationFrame(() => listRef.current?.querySelector<HTMLElement>(
                `[data-record-index="${next}"]`,
              )?.focus({ preventScroll: true }))
            }}
          >
            {data.rows.map(row => (
              <button
                key={`${row.coordinate.actual_type}\u001f${row.coordinate.key}`}
                type="button"
                className={row.coordinate.key === selected?.coordinate.key ? 'selected' : ''}
                data-record-index={data.rows.indexOf(row)}
                onClick={() => { setSelectedKey(row.coordinate.key); setRecordField(0) }}
              >
                {row.coordinate.key}
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

function DimensionGrid({ data, onWrite, rootRef, onExitLeft, onExitUp }: Pick<Props, 'data' | 'onWrite' | 'onExitLeft' | 'onExitUp'> & {
  rootRef: React.RefObject<HTMLDivElement | null>
}) {
  const [selection, setSelection] = useState<DimensionCellSelection>({ row: 0, column: 0 })
  const columnCount = data.variants.length + 2
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
        select(moveDimensionCell(selection, event.key, data.rows.length, columnCount))
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
          {data.rows.map(row => (
            <tr key={`${row.coordinate.actual_type}\u001f${row.coordinate.key}`}>
              <th scope="row" {...cellProps(data.rows.indexOf(row), 0, selection, select)}>{row.coordinate.key}</th>
              <td {...cellProps(data.rows.indexOf(row), 1, selection, select)}><DataCardCompact value={row.default_value} /></td>
              {data.variants.map((variant, variantIndex) => (
                <td key={variant} {...cellProps(data.rows.indexOf(row), variantIndex + 2, selection, select)}>
                  <DimensionCellEditor row={row} variant={variant} onWrite={onWrite} />
                </td>
              ))}
            </tr>
          ))}
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
        <DataCardCompact value={row.default_value} />
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
