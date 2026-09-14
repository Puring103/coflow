import type { GraphEdgeView, GraphNodeView } from '../wire'
import { COLUMN_GAP, HEADER_HEIGHT, NODE_WIDTH, ROW_GAP, type Position } from './metrics'

export interface RetainPositionsInput {
  positions: Map<string, Position>
  visibleNodes: GraphNodeView[]
  forwardEdges: GraphEdgeView[]
  backEdges: GraphEdgeView[]
}

/** 增量刷新只安置新增节点，已存在节点（包括用户拖动的位置）保持不变。 */
export function retainGraphPositions(
  layout: RetainPositionsInput,
  retained: ReadonlyMap<string, Position>,
  heightOf: (id: string) => number,
): Map<string, Position> {
  if (retained.size === 0) return new Map(layout.positions)
  const positions = new Map<string, Position>()
  for (const id of layout.positions.keys()) {
    const previous = retained.get(id)
    if (previous) positions.set(id, previous)
  }
  // 高度缓存一次，避免热循环内反复回退估计。
  const heights = new Map<string, number>()
  for (const id of layout.positions.keys()) heights.set(id, heightOf(id))
  const height = (id: string): number => heights.get(id) ?? HEADER_HEIGHT
  for (const [id, suggested] of layout.positions) {
    if (positions.has(id)) continue
    const incoming = [...layout.forwardEdges, ...layout.backEdges].find(edge => edge.target === id && positions.has(edge.source))
    const source = incoming && positions.get(incoming.source)
    const candidate = source ? { x: source.x + NODE_WIDTH + COLUMN_GAP, y: source.y } : { ...suggested }
    // 线性扫描但复用缓存高度，避免每次重建闭包与重复估计。
    let moved = true
    while (moved) {
      moved = false
      for (const [otherId, other] of positions) {
        if (
          Math.abs(other.x - candidate.x) < NODE_WIDTH + ROW_GAP
          && candidate.y < other.y + height(otherId) + ROW_GAP
          && candidate.y + height(id) + ROW_GAP > other.y
        ) {
          candidate.y = other.y + height(otherId) + ROW_GAP
          moved = true
        }
      }
    }
    positions.set(id, candidate)
  }
  return positions
}
