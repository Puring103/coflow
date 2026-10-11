/** 图拓扑唯一实现：BFS 可达、连通分量、回边检测，state 与 layout 共用。 */
export function bfsReachable(roots: string[], edges: Array<{ source: string; target: string }>): Set<string> {
  const outgoing = new Map<string, string[]>()
  for (const edge of edges) {
    const targets = outgoing.get(edge.source) ?? []
    targets.push(edge.target)
    outgoing.set(edge.source, targets)
  }
  const reachable = new Set(roots)
  const queue = [...roots]
  while (queue.length > 0) {
    const current = queue.shift()!
    for (const target of outgoing.get(current) ?? []) {
      if (reachable.has(target)) continue
      reachable.add(target)
      queue.push(target)
    }
  }
  return reachable
}

export function connectedComponents(
  nodeIds: string[],
  edges: Array<{ source: string; target: string }>,
): string[][] {
  const adjacency = new Map<string, Set<string>>()
  for (const id of nodeIds) adjacency.set(id, new Set())
  for (const edge of edges) {
    adjacency.get(edge.source)?.add(edge.target)
    adjacency.get(edge.target)?.add(edge.source)
  }
  const visited = new Set<string>()
  const components: string[][] = []
  for (const id of nodeIds) {
    if (visited.has(id)) continue
    const component: string[] = []
    const queue = [id]
    while (queue.length > 0) {
      const current = queue.shift()!
      if (visited.has(current)) continue
      visited.add(current)
      component.push(current)
      for (const neighbor of adjacency.get(current) ?? []) {
        if (!visited.has(neighbor)) queue.push(neighbor)
      }
    }
    components.push(component)
  }
  return components
}

export function detectBackEdges(
  nodes: Array<{ id: string }>,
  edges: Array<{ source: string; target: string }>,
): Set<string> {
  const adjacency = new Map<string, string[]>()
  for (const node of nodes) adjacency.set(node.id, [])
  for (const edge of edges) adjacency.get(edge.source)?.push(edge.target)
  const state = new Map<string, 'white' | 'gray' | 'black'>()
  for (const node of nodes) state.set(node.id, 'white')
  const backEdges = new Set<string>()
  const visit = (id: string): void => {
    state.set(id, 'gray')
    for (const target of adjacency.get(id) ?? []) {
      const targetState = state.get(target)
      if (targetState === 'gray') backEdges.add(`${id}→${target}`)
      else if (targetState === 'white') visit(target)
    }
    state.set(id, 'black')
  }
  for (const node of nodes) {
    if (state.get(node.id) === 'white') visit(node.id)
  }
  return backEdges
}
