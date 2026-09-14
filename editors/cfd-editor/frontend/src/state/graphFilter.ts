import type { GraphData } from '../bindings/GraphData'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import { coordinateId } from '../wire'

export function reachableGraph(
  graph: GraphData,
  isRoot: (coordinate: RecordCoordinate) => boolean,
): GraphData {
  const adjacency = new Map<string, string[]>()
  for (const edge of graph.edges) {
    const source = coordinateId(edge.source)
    const targets = adjacency.get(source)
    if (targets) targets.push(coordinateId(edge.target))
    else adjacency.set(source, [coordinateId(edge.target)])
  }

  const keep = new Set<string>()
  const queue: string[] = []
  for (const node of graph.nodes) {
    const id = coordinateId(node.coordinate)
    if (!isRoot(node.coordinate) || keep.has(id)) continue
    keep.add(id)
    queue.push(id)
  }

  // 每条可达边最多访问一次，避免旧实现反复扫描完整边集。
  for (let cursor = 0; cursor < queue.length; cursor += 1) {
    for (const target of adjacency.get(queue[cursor]) ?? []) {
      if (keep.has(target)) continue
      keep.add(target)
      queue.push(target)
    }
  }

  return {
    ...graph,
    nodes: graph.nodes.filter(node => keep.has(coordinateId(node.coordinate))),
    edges: graph.edges.filter(edge => (
      keep.has(coordinateId(edge.source)) && keep.has(coordinateId(edge.target))
    )),
  }
}
