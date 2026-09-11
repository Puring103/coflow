import type { Node } from '@xyflow/react'
import type { GraphNode } from '../bindings/GraphNode'
import { graphNodeView, type GraphNodeView } from '../wire'

// 图数据是包含 bigint 的不可变 wire 对象；按内容复用，避免后端回包换引用导致整图重绘。
export function sameGraphValue(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true
  if (!left || !right || typeof left !== 'object' || typeof right !== 'object') return false
  if (left instanceof Set || right instanceof Set) {
    return left instanceof Set && right instanceof Set && left.size === right.size
      && [...left].every(value => right.has(value))
  }
  if (Array.isArray(left) !== Array.isArray(right)) return false
  const a = left as Record<string, unknown>
  const b = right as Record<string, unknown>
  const keys = Object.keys(a)
  return keys.length === Object.keys(b).length
    && keys.every(key => Object.prototype.hasOwnProperty.call(b, key) && sameGraphValue(a[key], b[key]))
}

export function reconcileGraphViews(previous: readonly GraphNodeView[], nodes: readonly GraphNode[]): GraphNodeView[] {
  const byId = new Map(previous.map(node => [node.id, node]))
  const next = nodes.map(node => {
    const view = graphNodeView(node)
    const old = byId.get(view.id)
    return old && sameGraphValue(old, view) ? old : view
  })
  return next.length === previous.length && next.every((node, index) => node === previous[index])
    ? previous as GraphNodeView[] : next
}

export function reconcileFlowNodes(previous: Node[], incoming: Node[]): Node[] {
  const byId = new Map(previous.map(node => [node.id, node]))
  const next = incoming.map(node => {
    const old = byId.get(node.id)
    if (!old) return node
    const data = sameGraphValue(old.data, node.data) ? old.data : node.data
    const position = old.position.x === node.position.x && old.position.y === node.position.y
      ? old.position : node.position
    if (data === old.data && position === old.position) return old
    // 拖动和内容回写都保留 React Flow 已测量的尺寸，避免反复进入未测量状态。
    return { ...old, ...node, data, position, measured: old.measured, handles: old.measured ? old.handles : node.handles }
  })
  return next.length === previous.length && next.every((node, index) => node === previous[index]) ? previous : next
}

export type GraphFocus = { kind: 'node' | 'edge'; id: string } | null

export function graphHighlights(
  edges: readonly { id: string; source: string; target: string }[],
  selected: GraphFocus,
  hovered: GraphFocus,
): { nodes: Set<string>; edges: Set<string> } {
  const nodes = new Set<string>()
  const highlightedEdges = new Set<string>()
  for (const focus of [selected, hovered]) {
    if (!focus) continue
    if (focus.kind === 'node') nodes.add(focus.id)
    for (const edge of edges) {
      if (focus.kind === 'edge' ? edge.id === focus.id : edge.source === focus.id || edge.target === focus.id) {
        nodes.add(edge.source)
        nodes.add(edge.target)
        highlightedEdges.add(edge.id)
      }
    }
  }
  return { nodes, edges: highlightedEdges }
}
