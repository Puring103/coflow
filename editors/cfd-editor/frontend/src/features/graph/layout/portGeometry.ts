import { Position, type Edge, type InternalNode, type Node, type NodeHandle } from '@xyflow/react'
import type { GraphRowGeometry } from './rowGeometry'

interface PortNodeData extends Record<string, unknown> {
  geometry: GraphRowGeometry
  outgoingPaths: string[]
}
interface PortCache {
  outgoing: string[]
  outgoingSet: ReadonlySet<string>
  geometry: GraphRowGeometry
  version: number
  paths: string[]
  handles: NodeHandle[]
  input: Node
  node: Node
}

export function geometryHandles(geometry: GraphRowGeometry, paths: readonly string[]): NodeHandle[] {
  return [
    { id: '__in', type: 'target', position: Position.Left, x: -7, y: geometry.borderTop + geometry.headerHeight / 2 - 8, width: 16, height: 16 },
    ...paths.map(path => ({ id: `path-${path}`, type: 'source' as const, position: Position.Right,
      x: 271, y: geometry.center(path) - 8, width: 16, height: 16 })),
  ]
}

/** 同时核对公开尺寸和框架实际端口；DOM 观察丢失屏外端口时仍必须重新发布。 */
export function geometryIsPublished(node: InternalNode | undefined, geometry: GraphRowGeometry, handles: readonly NodeHandle[]): boolean {
  if (!node || node.width !== 280 || node.height !== geometry.height
    || node.measured.width !== 280 || node.measured.height !== geometry.height) return false
  const bounds = node.internals.handleBounds
  const actual = [...(bounds?.source ?? []), ...(bounds?.target ?? [])]
  if (actual.length !== handles.length) return false
  const byId = new Map(actual.map(handle => [handle.id, handle]))
  return handles.every(handle => {
    const bound = byId.get(handle.id)
    return bound?.type === handle.type && bound.position === handle.position
      && (['x', 'y', 'width', 'height'] as const).every(key => {
        const expected = handle[key], measured = bound[key]
        return expected !== undefined && Math.abs(expected - measured) < .05
      })
  })
}

/** 完整端口留在几何索引；框架只接收绘制连线和当前窗口需要的端口，避免逐边扫描全部端口。 */
export class GraphPortWindow {
  private cache = new Map<string, PortCache>()
  private previousNodes: Node[] = []
  private previousPaths = new Map<string, readonly string[]>()

  project(nodes: Node[], edges: Edge[], viewport: { x: number; y: number; zoom: number }, height: number,
    active: { nodeId: string; portId: string } | null = null): { nodes: Node[]; paths: ReadonlyMap<string, readonly string[]> } {
    const required = new Map<string, Set<string>>()
    const add = (id: string, handle: string | null | undefined) => {
      if (!handle?.startsWith('path-')) return
      let paths = required.get(id)
      if (!paths) required.set(id, paths = new Set())
      paths.add(handle.slice(5))
    }
    // 穿屏线的起点可能位于窗口之外，必须同时发布它的真实逻辑端口。
    for (const edge of edges) add(edge.source, edge.sourceHandle)
    if (active) add(active.nodeId, active.portId)
    const pathsByNode = new Map<string, readonly string[]>()
    const next = nodes.map(node => {
      const { geometry, outgoingPaths } = node.data as PortNodeData
      const previous = this.cache.get(node.id)
      const outgoingSet = previous?.outgoing === outgoingPaths ? previous.outgoingSet : new Set(outgoingPaths)
      const paths = required.get(node.id) ?? new Set<string>()
      const top = -viewport.y / viewport.zoom - node.position.y
      const [first, end] = geometry.range(top - 180, top + height / viewport.zoom + 180)
      for (let index = first; index < end; index++) {
        const path = geometry.rows[index].path
        if (outgoingSet.has(path)) paths.add(path)
      }
      const ordered = [...paths].sort()
      const samePaths = previous && ordered.length === previous.paths.length && ordered.every((path, index) => path === previous.paths[index])
      const publishedPaths = samePaths ? previous.paths : ordered
      const handles = previous?.geometry === geometry && previous.version === geometry.snapshot() && samePaths
        ? previous.handles : geometryHandles(geometry, publishedPaths)
      const result = node.handles === handles ? node : previous?.input === node && previous.handles === handles ? previous.node : { ...node, handles }
      this.cache.set(node.id, { outgoing: outgoingPaths, outgoingSet, geometry, version: geometry.snapshot(), paths: publishedPaths, handles, input: node, node: result })
      pathsByNode.set(node.id, publishedPaths)
      return result
    })
    for (const id of this.cache.keys()) if (!pathsByNode.has(id)) this.cache.delete(id)
    if (next.length !== this.previousNodes.length || next.some((node, index) => node !== this.previousNodes[index])) this.previousNodes = next
    if (pathsByNode.size !== this.previousPaths.size || [...pathsByNode].some(([id, paths]) => paths !== this.previousPaths.get(id))) this.previousPaths = pathsByNode
    return { nodes: this.previousNodes, paths: this.previousPaths }
  }
}
