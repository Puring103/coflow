import { useEffect, useRef, useState } from 'react'
import type { DimensionFileRecords, DimensionFileRow } from '../api'
import type { DimensionValueState } from '../bindings/DimensionValueState'
import { DataCardCompact } from './DataCard'
import { DimensionCellEditor } from './DimensionTableView'

interface Props {
  data: DimensionFileRecords
  onWrite: (row: DimensionFileRow, variant: string, expected: DimensionValueState, next: DimensionValueState) => Promise<void>
  onExitLeft?: () => void
  onExitUp?: () => void
  focusRequest?: number
  onFocusRequestConsumed?: (request: number) => void
}

/** 单例按变体组织记录；默认值只作为各字段的只读参照。 */
export function DimensionSingletonView({ data, onWrite, onExitLeft, onExitUp, focusRequest = 0, onFocusRequestConsumed }: Props) {
  const [selectedVariant, setSelectedVariant] = useState<string | null>(null)
  const [selectedField, setSelectedField] = useState(0)
  const listRef = useRef<HTMLElement>(null)
  const recordRef = useRef<HTMLElement>(null)
  const variant = selectedVariant && data.variants.includes(selectedVariant) ? selectedVariant : data.variants[0]
  useEffect(() => {
    if (!focusRequest) return
    listRef.current?.querySelector<HTMLElement>('.selected')?.focus({ preventScroll: true })
    onFocusRequestConsumed?.(focusRequest)
  }, [focusRequest, onFocusRequestConsumed])
  const focusEditor = () => recordRef.current?.querySelectorAll<HTMLElement>('.dimension-record-row')[selectedField]
    ?.querySelector<HTMLElement>('.dimension-value-missing, .dimension-cell-editor input, .dimension-cell-editor textarea, .dimension-cell-editor select, .dimension-cell-editor button')?.focus()
  return <div className="dimension-table-view">
    <div className="dimension-table-meta"><strong>{data.display_name}</strong><span>{data.variants.length} 种变体</span></div>
    <div className="dimension-record-layout">
      <aside className="dimension-record-list" aria-label="维度变体" ref={listRef}
        onKeyDown={event => {
          const index = data.variants.indexOf(variant ?? '')
          if (event.key === 'ArrowLeft') { event.preventDefault(); onExitLeft?.() }
          else if (event.key === 'ArrowRight') { event.preventDefault(); recordRef.current?.focus({ preventScroll: true }) }
          else if (event.key === 'ArrowUp' && index === 0) { event.preventDefault(); onExitUp?.() }
          else if (event.key === 'ArrowUp' || event.key === 'ArrowDown') {
            event.preventDefault()
            const next = Math.max(0, Math.min(data.variants.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)))
            setSelectedVariant(data.variants[next] ?? null)
            requestAnimationFrame(() => listRef.current?.querySelectorAll<HTMLElement>('button')[next]?.focus({ preventScroll: true }))
          }
        }}>
        {data.variants.map(value => <button type="button" key={value}
          className={value === variant ? 'selected' : ''}
          onClick={() => { setSelectedVariant(value); setSelectedField(0) }}>{value}</button>)}
      </aside>
      <main className="dimension-record-main" ref={recordRef} tabIndex={0}
        onKeyDown={event => {
          if (event.target !== recordRef.current) return
          if (event.key === 'ArrowLeft') { event.preventDefault(); listRef.current?.querySelector<HTMLElement>('.selected')?.focus({ preventScroll: true }) }
          else if (event.key === 'ArrowUp' || event.key === 'ArrowDown') {
            event.preventDefault()
            if (data.rows.length) setSelectedField(current => Math.max(0, Math.min(data.rows.length - 1, current + (event.key === 'ArrowDown' ? 1 : -1))))
          } else if (event.key === 'Enter' || event.key === 'ArrowRight') { event.preventDefault(); focusEditor() }
        }}>
        {variant ? <div className="dimension-record-card">
          <header><strong>{variant}</strong></header>
          {data.rows.map((row, index) => <div key={row.field} className={`dimension-record-row${index === selectedField ? ' keyboard-selected' : ''}`}
            style={{ gridTemplateColumns: '150px minmax(0, 1fr)' }} onClick={() => setSelectedField(index)}>
            <span>{row.field}</span>
            <div className="dimension-singleton-value">
              <DimensionCellEditor row={row} variant={variant} onWrite={onWrite} />
              <div className="dimension-singleton-default"><span>default</span><DataCardCompact value={row.default_value} formattedPreviews={row.default_previews} /></div>
            </div>
          </div>)}
        </div> : <div className="empty-hint">尚无变体</div>}
      </main>
    </div>
  </div>
}
