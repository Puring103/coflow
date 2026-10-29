import type { Edge, Node } from '@xyflow/react'
import type { GraphRowGeometry } from './rowGeometry'

interface NodeGeometry { x: number; y: number; geometry: GraphRowGeometry; version: number }
interface EdgeGeometry { edge: Edge; points: { x: number; y: number }[] }

/** 端口窗口切换仅影响框架发布数据；完整连线坐标只在逻辑几何或节点位置变化时重建。 */
export class GraphEdgeGeometry {
  private nodes = new Map<string, NodeGeometry>()
  private edges: Edge[] = []
  private result: EdgeGeometry[] = []

  project(nodes: Node[], edges: Edge[]): EdgeGeometry[] {
    let changed = nodes.length !== this.nodes.size || edges !== this.edges
    const next = new Map<string, NodeGeometry>()
    for (const node of nodes) {
      const geometry = node.data.geometry as GraphRowGeometry
      const before = this.nodes.get(node.id)
      const same = before && before.x === node.position.x && before.y === node.position.y
        && before.geometry === geometry && before.version === geometry.snapshot()
      changed ||= !same
      next.set(node.id, same ? before : { x: node.position.x, y: node.position.y, geometry, version: geometry.snapshot() })
    }
    if (!changed) return this.result
    this.nodes = next
    this.edges = edges
    this.result = edges.map(edge => {
      const source = next.get(edge.source), target = next.get(edge.target)
      if (!source || !target) return { edge, points: [] }
      const from = { x: source.x + 280, y: source.y + source.geometry.center(edge.sourceHandle?.slice(5) ?? '') }
      const to = { x: target.x, y: target.y + target.geometry.borderTop + target.geometry.headerHeight / 2 }
      return { edge, points: [from, { x: from.x + 28, y: from.y }, { x: to.x - 28, y: to.y }, to] }
    })
    return this.result
  }
}
