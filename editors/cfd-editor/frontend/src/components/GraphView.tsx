import { useCallback, useMemo, useState } from 'react'
import {
  ReactFlow, Background, Controls, MiniMap,
  Handle, Position, type NodeProps,
  type Node, type Edge,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import type { GraphData, GraphNode } from '../bindings/index'
import { DataCardNode } from './DataCard'
import { Icon } from './Icon'

interface NodeData extends Record<string, unknown> {
  graphNode: GraphNode
}

function CfdNode({ data }: NodeProps) {
  const { graphNode: gn } = data as NodeData

  return (
    <div className={`graph-node${gn.in_focus_file ? ' focused' : ' dim'}`}>
      <Handle type="target" position={Position.Left} />
      <div className="gn-header">
        <span className="gn-type">{gn.actual_type}</span>
        <span className="gn-key">{gn.key}</span>
        <span className="gn-file">{gn.file_path.split('/').pop()}</span>
      </div>
      {gn.is_collapsed ? (
        <div className="gn-collapsed">折叠（超出深度）</div>
      ) : (
        <DataCardNode fields={gn.fields} />
      )}
      <Handle type="source" position={Position.Right} />
    </div>
  )
}

const nodeTypes = { cfd: CfdNode }

// ─── Field path → top-level field name (drops[0] → drops, reward.item → reward) ─

function topLevelField(path: string): string {
  const m = path.match(/^[^.[]+/)
  return m ? m[0] : path
}

// ─── Longest-path layering ──────────────────────────────────────────────────
// For DAGs over the filtered edge set: each node's layer = max(predecessor) + 1.
// Cycles are broken by ignoring back-edges encountered during DFS.

function layerByLongestPath(
  nodes: GraphNode[],
  edges: { source: string; target: string }[],
  forcedRoots: Set<string>,
): Map<string, number> {
  const incoming = new Map<string, string[]>()
  for (const n of nodes) incoming.set(n.id, [])
  for (const e of edges) {
    if (incoming.has(e.target)) incoming.get(e.target)!.push(e.source)
  }

  const layer = new Map<string, number>()
  const inProgress = new Set<string>()

  function visit(id: string): number {
    if (layer.has(id)) return layer.get(id)!
    if (forcedRoots.has(id)) {
      layer.set(id, 0)
      return 0
    }
    if (inProgress.has(id)) return 0 // cycle
    inProgress.add(id)
    let max = 0
    // Only consider predecessors that are not forced roots themselves; if all
    // predecessors are forced roots, we still get max = 0 + 1 = 1 (correct).
    for (const pred of incoming.get(id) ?? []) {
      const l = visit(pred) + 1
      if (l > max) max = l
    }
    inProgress.delete(id)
    layer.set(id, max)
    return max
  }

  for (const n of nodes) visit(n.id)
  return layer
}

// ─── Layout ────────────────────────────────────────────────────────────────

function layoutNodes(
  graph: GraphData,
  enabledFields: Set<string>,
  activeType: string | undefined,
): { nodes: Node[]; edges: Edge[] } {
  const NODE_W = 280
  const NODE_H_EST = 220
  const COL_GAP = 180
  const ROW_GAP = 36

  // Filter edges by toolbar selection
  let activeEdges = graph.edges.filter(e =>
    enabledFields.has(topLevelField(e.field_path))
  )

  const nodeById = new Map(graph.nodes.map(n => [n.id, n]))

  // When activeType is set, layer 0 = activeType records of focus file.
  // Walk only OUTGOING edges from those roots; other-type nodes appear only
  // when they're referenced (directly or transitively) by an activeType root.
  const touched = new Set<string>()
  for (const e of activeEdges) {
    touched.add(e.source)
    touched.add(e.target)
  }
  let visibleSet: Set<string>
  if (activeType) {
    const roots = graph.nodes
      .filter(n => n.in_focus_file && n.actual_type === activeType && touched.has(n.id))
      .map(n => n.id)
    visibleSet = new Set(roots)

    const outgoing = new Map<string, string[]>()
    for (const e of activeEdges) {
      if (!outgoing.has(e.source)) outgoing.set(e.source, [])
      outgoing.get(e.source)!.push(e.target)
    }
    const queue = [...roots]
    while (queue.length > 0) {
      const cur = queue.shift()!
      for (const nb of outgoing.get(cur) ?? []) {
        if (!visibleSet.has(nb)) {
          visibleSet.add(nb)
          queue.push(nb)
        }
      }
    }
    activeEdges = activeEdges.filter(e => visibleSet.has(e.source) && visibleSet.has(e.target))
  } else {
    visibleSet = touched
  }

  const visibleNodes = graph.nodes.filter(n => visibleSet.has(n.id))
  void nodeById

  // First-level roots = focus-file activeType nodes that survived filtering.
  const forcedRoots = new Set<string>(
    activeType
      ? visibleNodes
          .filter(n => n.in_focus_file && n.actual_type === activeType)
          .map(n => n.id)
      : []
  )

  const layer = layerByLongestPath(visibleNodes, activeEdges, forcedRoots)

  // Group by layer
  const layerToNodes = new Map<number, GraphNode[]>()
  for (const n of visibleNodes) {
    const l = layer.get(n.id) ?? 0
    if (!layerToNodes.has(l)) layerToNodes.set(l, [])
    layerToNodes.get(l)!.push(n)
  }
  // Initial order: focus first, then file group, then key
  for (const [, list] of layerToNodes) {
    list.sort((a, b) => {
      if (a.in_focus_file !== b.in_focus_file) return a.in_focus_file ? -1 : 1
      if (a.file_path !== b.file_path) return a.file_path.localeCompare(b.file_path)
      return a.key.localeCompare(b.key)
    })
  }

  const layers = Array.from(layerToNodes.keys()).sort((a, b) => a - b)

  // Barycenter ordering: iteratively re-order each layer by the mean position
  // of its incoming neighbours in the previous layer (and outgoing in the next).
  // 24 iterations of alternating sweeps is plenty for graphs of this size.
  const indexInLayer = (id: string): number => {
    const l = layer.get(id) ?? 0
    return layerToNodes.get(l)!.indexOf(id as unknown as GraphNode) // placeholder, replaced below
  }
  void indexInLayer

  const adjOut = new Map<string, string[]>()
  const adjIn = new Map<string, string[]>()
  for (const e of activeEdges) {
    if (!adjOut.has(e.source)) adjOut.set(e.source, [])
    adjOut.get(e.source)!.push(e.target)
    if (!adjIn.has(e.target)) adjIn.set(e.target, [])
    adjIn.get(e.target)!.push(e.source)
  }

  function indexOf(id: string, list: GraphNode[]): number {
    for (let i = 0; i < list.length; i++) if (list[i].id === id) return i
    return -1
  }

  function meanIndex(id: string, neighbors: string[] | undefined, neighborLayer: number): number {
    if (!neighbors || neighbors.length === 0) return Number.POSITIVE_INFINITY
    const list = layerToNodes.get(neighborLayer)
    if (!list) return Number.POSITIVE_INFINITY
    let sum = 0
    let n = 0
    for (const nb of neighbors) {
      const idx = indexOf(nb, list)
      if (idx >= 0) {
        sum += idx
        n += 1
      }
    }
    return n === 0 ? Number.POSITIVE_INFINITY : sum / n
  }

  const ITER = 24
  for (let it = 0; it < ITER; it++) {
    // Forward pass: order each layer by mean index of its predecessors
    for (let i = 1; i < layers.length; i++) {
      const cur = layers[i]
      const prev = layers[i - 1]
      const list = layerToNodes.get(cur)!
      const ranks = new Map<string, number>()
      for (const n of list) {
        ranks.set(n.id, meanIndex(n.id, adjIn.get(n.id), prev))
      }
      list.sort((a, b) => {
        const ra = ranks.get(a.id)!
        const rb = ranks.get(b.id)!
        if (ra !== rb) return ra - rb
        return a.key.localeCompare(b.key)
      })
    }
    // Backward pass: order by mean index of successors
    for (let i = layers.length - 2; i >= 0; i--) {
      const cur = layers[i]
      const next = layers[i + 1]
      const list = layerToNodes.get(cur)!
      const ranks = new Map<string, number>()
      for (const n of list) {
        ranks.set(n.id, meanIndex(n.id, adjOut.get(n.id), next))
      }
      list.sort((a, b) => {
        const ra = ranks.get(a.id)!
        const rb = ranks.get(b.id)!
        if (ra !== rb) return ra - rb
        return a.key.localeCompare(b.key)
      })
    }
  }

  // ─── Position assignment ────────────────────────────────────────────────
  // Layer x position is fixed. Within a layer, place each node at the
  // average y of its neighbours when possible, otherwise fall back to slot
  // index. This pulls each node toward its connected neighbours and removes
  // most label/line passes through unrelated nodes.
  const positions = new Map<string, { x: number; y: number }>()
  // Pass 1: slot-based y, used as initial positions
  for (const l of layers) {
    const list = layerToNodes.get(l)!
    const colX = l * (NODE_W + COL_GAP)
    const totalH = list.length * NODE_H_EST + (list.length - 1) * ROW_GAP
    const startY = -totalH / 2
    list.forEach((n, i) => {
      positions.set(n.id, { x: colX, y: startY + i * (NODE_H_EST + ROW_GAP) })
    })
  }
  // Pass 2: snap each node toward the average y of its neighbours, but never
  // closer than NODE_H_EST + ROW_GAP from its layer-mates (preserve order).
  function refineYByNeighbours() {
    for (const l of layers) {
      const list = layerToNodes.get(l)!
      // Compute desired y for each node
      const desired = list.map(n => {
        const ins = (adjIn.get(n.id) ?? [])
          .map(id => positions.get(id)?.y)
          .filter((y): y is number => y !== undefined)
        const outs = (adjOut.get(n.id) ?? [])
          .map(id => positions.get(id)?.y)
          .filter((y): y is number => y !== undefined)
        const all = [...ins, ...outs]
        if (all.length === 0) return positions.get(n.id)!.y
        return all.reduce((a, b) => a + b, 0) / all.length
      })
      // Clamp to non-overlapping order
      const minSpacing = NODE_H_EST + ROW_GAP
      const sorted = list
        .map((n, i) => ({ n, want: desired[i] }))
        .sort((a, b) => a.want - b.want)
      let prev = -Infinity
      for (const item of sorted) {
        const y = Math.max(item.want, prev + minSpacing)
        positions.set(item.n.id, { x: positions.get(item.n.id)!.x, y })
        prev = y
      }
    }
  }
  for (let i = 0; i < 3; i++) refineYByNeighbours()

  const rfNodes: Node[] = visibleNodes.map(n => ({
    id: n.id,
    type: 'cfd',
    position: positions.get(n.id) ?? { x: 0, y: 0 },
    data: { graphNode: n } satisfies NodeData,
  }))

  const rfEdges: Edge[] = activeEdges
    .filter(e => positions.has(e.source) && positions.has(e.target))
    .map((e, i) => ({
      id: `e${i}`,
      source: e.source,
      target: e.target,
      label: e.field_path,
      type: 'bezier',
      animated: false,
      style: { stroke: '#4a525e', strokeWidth: 1.2 },
      labelStyle: { fill: '#7a828f', fontSize: 10, fontFamily: 'JetBrains Mono, monospace' },
      labelBgStyle: { fill: '#1a1e25', fillOpacity: 0.92 },
      labelBgPadding: [4, 2] as [number, number],
      labelBgBorderRadius: 3,
    }))

  return { nodes: rfNodes, edges: rfEdges }
}

// ─── Component ─────────────────────────────────────────────────────────────

interface Props {
  graphData: GraphData
  activeType?: string
  onOpenRecord: (file: string, key: string) => void
}

export function GraphView({ graphData, activeType, onOpenRecord }: Props) {
  // Distinct top-level field names appearing in edges
  const allFields = useMemo(() => {
    const set = new Set<string>()
    for (const e of graphData.edges) set.add(topLevelField(e.field_path))
    return Array.from(set).sort()
  }, [graphData])

  const [enabledFields, setEnabledFields] = useState<Set<string>>(() => new Set(allFields))

  // Re-sync enabled set when graph changes
  useMemo(() => {
    setEnabledFields(prev => {
      const next = new Set<string>()
      for (const f of allFields) if (prev.has(f) || prev.size === 0) next.add(f)
      return next
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [allFields.join('|')])

  const { nodes, edges } = useMemo(
    () => layoutNodes(graphData, enabledFields, activeType),
    [graphData, enabledFields, activeType]
  )

  const onNodeDoubleClick = useCallback((_: unknown, node: Node) => {
    const { graphNode } = node.data as NodeData
    onOpenRecord(graphNode.file_path, graphNode.key)
  }, [onOpenRecord])

  function toggleField(name: string) {
    setEnabledFields(prev => {
      const next = new Set(prev)
      if (next.has(name)) next.delete(name)
      else next.add(name)
      return next
    })
  }

  const allOn = enabledFields.size === allFields.length
  const noneOn = enabledFields.size === 0

  return (
    <div className="graph-view-wrap">
      {allFields.length > 0 && (
        <div className="graph-toolbar">
          <span className="graph-toolbar-label">字段过滤</span>
          <div className="graph-field-chips">
            {allFields.map(f => {
              const on = enabledFields.has(f)
              return (
                <button
                  key={f}
                  className={`field-chip${on ? ' on' : ''}`}
                  onClick={() => toggleField(f)}
                  title={on ? '点击隐藏此字段的连线' : '点击显示此字段的连线'}
                >
                  {on ? <Icon name="check" size={10} /> : <Icon name="dot" size={10} />}
                  {f}
                </button>
              )
            })}
          </div>
          <span className="graph-toolbar-spacer" />
          <button
            className="btn"
            onClick={() => setEnabledFields(allOn ? new Set() : new Set(allFields))}
          >
            {allOn ? '全部隐藏' : noneOn ? '全部显示' : '反选'}
          </button>
        </div>
      )}
      <div className="graph-view">
        {nodes.length === 0 ? (
          <div className="empty-hint">无可显示的引用关系</div>
        ) : (
          <ReactFlow
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            onNodeDoubleClick={onNodeDoubleClick}
            fitView
            fitViewOptions={{ padding: 0.2, minZoom: 0.3, maxZoom: 1.2 }}
            proOptions={{ hideAttribution: true }}
            minZoom={0.2}
            maxZoom={2}
          >
            <Background color="#262b34" gap={24} size={1} />
            <Controls showInteractive={false} />
            <MiniMap
              nodeColor={n => {
                const { graphNode } = n.data as NodeData
                return graphNode.in_focus_file ? '#8a93a3' : '#3a3f48'
              }}
              maskColor="rgba(14, 16, 20, 0.75)"
              pannable
              zoomable
            />
          </ReactFlow>
        )}
      </div>
    </div>
  )
}
