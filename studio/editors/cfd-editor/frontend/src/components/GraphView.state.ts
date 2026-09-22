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

// 框选几何统一用左上/右下表达，屏幕坐标与 flow 坐标都可复用。
export interface GraphRect {
  left: number
  top: number
  right: number
  bottom: number
}

// 选中节点的包围盒；没有选中节点时返回 null，调用方据此决定是否绘制。
export function selectedNodeBounds(
  nodes: readonly { id: string; position: { x: number; y: number }; measured?: { width?: number; height?: number } }[],
  selected: ReadonlySet<string>,
  fallback: { width: number; height: number },
): GraphRect | null {
  let left = Infinity
  let top = Infinity
  let right = -Infinity
  let bottom = -Infinity
  for (const node of nodes) {
    if (!selected.has(node.id)) continue
    const width = node.measured?.width ?? fallback.width
    const height = node.measured?.height ?? fallback.height
    left = Math.min(left, node.position.x)
    top = Math.min(top, node.position.y)
    right = Math.max(right, node.position.x + width)
    bottom = Math.max(bottom, node.position.y + height)
  }
  return Number.isFinite(left) ? { left, top, right, bottom } : null
}

function orientation(ax: number, ay: number, bx: number, by: number, cx: number, cy: number): number {
  return (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}

function onSegment(ax: number, ay: number, bx: number, by: number, px: number, py: number): boolean {
  return Math.min(ax, bx) <= px && px <= Math.max(ax, bx)
    && Math.min(ay, by) <= py && py <= Math.max(ay, by)
}

// 两条线段是否相交，包含共线重叠与端点接触。
export function segmentsIntersect(
  ax: number, ay: number, bx: number, by: number,
  cx: number, cy: number, dx: number, dy: number,
): boolean {
  const d1 = orientation(cx, cy, dx, dy, ax, ay)
  const d2 = orientation(cx, cy, dx, dy, bx, by)
  const d3 = orientation(ax, ay, bx, by, cx, cy)
  const d4 = orientation(ax, ay, bx, by, dx, dy)
  if (((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0))
    && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0))) return true
  if (d1 === 0 && onSegment(cx, cy, dx, dy, ax, ay)) return true
  if (d2 === 0 && onSegment(cx, cy, dx, dy, bx, by)) return true
  if (d3 === 0 && onSegment(ax, ay, bx, by, cx, cy)) return true
  if (d4 === 0 && onSegment(ax, ay, bx, by, dx, dy)) return true
  return false
}

export function pointInRect(x: number, y: number, rect: GraphRect): boolean {
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom
}

export function segmentIntersectsRect(
  x1: number, y1: number, x2: number, y2: number, rect: GraphRect,
): boolean {
  // 先做线段包围盒剪枝，避免每次采样都跑完整相交判断。
  if (Math.max(x1, x2) < rect.left || Math.min(x1, x2) > rect.right
    || Math.max(y1, y2) < rect.top || Math.min(y1, y2) > rect.bottom) return false
  if (pointInRect(x1, y1, rect) || pointInRect(x2, y2, rect)) return true
  return segmentsIntersect(x1, y1, x2, y2, rect.left, rect.top, rect.right, rect.top)
    || segmentsIntersect(x1, y1, x2, y2, rect.right, rect.top, rect.right, rect.bottom)
    || segmentsIntersect(x1, y1, x2, y2, rect.right, rect.bottom, rect.left, rect.bottom)
    || segmentsIntersect(x1, y1, x2, y2, rect.left, rect.bottom, rect.left, rect.top)
}

// 对曲线采样成折线后判断是否与框相交；框选连线时使用。
export function polylineIntersectsRect(
  points: readonly { x: number; y: number }[],
  rect: GraphRect,
): boolean {
  for (let index = 0; index < points.length; index += 1) {
    if (index === 0) {
      if (pointInRect(points[0].x, points[0].y, rect)) return true
      continue
    }
    const previous = points[index - 1]
    const current = points[index]
    if (segmentIntersectsRect(previous.x, previous.y, current.x, current.y, rect)) return true
  }
  return false
}

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
