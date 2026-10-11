import type { GraphData } from '../bindings/GraphData'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import { coordinateId } from '../wire'
import { bfsReachable } from '../graph/topology'

export function reachableGraph(
  graph: GraphData,
  isRoot: (coordinate: RecordCoordinate) => boolean,
): GraphData {
  const roots: string[] = []
  for (const node of graph.nodes) {
    const id = coordinateId(node.coordinate)
    if (!isRoot(node.coordinate) || roots.includes(id)) continue
    roots.push(id)
  }

  // 每条可达边最多访问一次，避免旧实现反复扫描完整边集。
  const keep = bfsReachable(
    roots,
    graph.edges.map(edge => ({ source: coordinateId(edge.source), target: coordinateId(edge.target) })),
  )

  return {
    ...graph,
    nodes: graph.nodes.filter(node => keep.has(coordinateId(node.coordinate))),
    edges: graph.edges.filter(edge => (
      keep.has(coordinateId(edge.source)) && keep.has(coordinateId(edge.target))
    )),
  }
}
