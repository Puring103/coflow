import { createContext, useContext, type ReactNode } from 'react'
import type { GraphRowGeometry } from './rowGeometry'

export interface GraphRowWindow {
  geometry: GraphRowGeometry
  first: number
  end: number
  pinned: ReadonlySet<number>
}
export const GraphRowWindowContext = createContext<GraphRowWindow | null>(null)

/** 保留原有字段组件与 DOM 层级，只以等高空块替代未挂载的兄弟区间。 */
export function WindowedItems({ parent, count, render }: { parent: string; count: number; render: (index: number) => ReactNode }) {
  const window = useContext(GraphRowWindowContext)
  if (!window) return Array.from({ length: count }, (_, index) => render(index))
  return window.geometry.slices(parent, window.first, window.end, window.pinned).map(slice => slice.visible
    ? render(slice.first)
    : <div key={`gap:${slice.first}`} aria-hidden className="graph-row-gap" style={{ height: slice.height, flex: 'none' }} />)
}

export function WindowedAuxRow({ rowKey, children }: { rowKey: string; children: ReactNode }) {
  const window = useContext(GraphRowWindowContext)
  if (!window) return children
  const row = window.geometry.byKey.get(rowKey)
  return row !== undefined && ((row >= window.first && row < window.end) || window.pinned.has(row))
    ? children : <div aria-hidden className="graph-row-gap" style={{ height: window.geometry.rowHeight(rowKey), flex: 'none' }} />
}
