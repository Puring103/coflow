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

// ─── Node data ──────────────────────────────────────────────────────────────

interface NodeData extends Record<string, unknown> {
  graphNode: GraphNode
  expanded: boolean
  onToggleExpand: () => void
}

// ─── CfdNode ─────────────────────────────────────────────────────────────────

function CfdNode({ data }: NodeProps) {
  const { graphNode: gn, expanded, onToggleExpand } = data as NodeData

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
        <DataCardNode fields={gn.fields} showAll={expanded} onToggle={onToggleExpand} />
      )}
      <Handle type="source" position={Position.Right} />
    </div>
  )
}

const nodeTypes = { cfd: CfdNode }

// ─── Field path → top-level field name ──────────────────────────────────────

function topLevelField(path: string): string {
  const m = path.match(/^[^.[]+/)
  return m ? m[0] : path
}

// ─── Longest-path layering ───────────────────────────────────────────────────
// forcedRoots are pinned to layer 0; all other nodes get longest-path from preds.

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
    if (forcedRoots.has(id)) { layer.set(id, 0); return 0 }
    if (inProgress.has(id)) return 0 // cycle break
    inProgress.add(id)
    let max = 0
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

// ─── Node height estimation ──────────────────────────────────────────────────
// Mirrors the CSS: gn-header ~42px + dc-row 22px each + more-btn 28px + padding.

const PEEK = 4
const HEADER_H = 42
const ROW_H = 22
const MORE_BTN_H = 28
const PAD_V = 12

function estimateNodeHeight(gn: GraphNode, expanded: boolean): number {
  if (gn.is_collapsed) return HEADER_H + 28 + PAD_V
  const visible = expanded ? gn.fields.length : Math.min(PEEK, gn.fields.length)
  const hasMore = gn.fields.length > PEEK
  return HEADER_H + visible * ROW_H + (hasMore ? MORE_BTN_H : 0) + PAD_V
}

// ─── Layout ──────────────────────────────────────────────────────────────────

interface LayoutResult {
  positions: Map<string, { x: number; y: number }>
  visibleNodes: GraphNode[]
  activeEdges: { source: string; target: string; field_path: string }[]
}

function layoutNodes(
  graph: GraphData,
  enabledFields: Set<string>,
  activeType: string | undefined,
  nodeExpandedMap: Map<string, boolean>,
): LayoutResult {
  const NODE_W = 280
  const COL_GAP = 180
  const ROW_GAP = 40

  // Filter edges by toolbar selection
  let activeEdges = graph.edges.filter(e =>
    enabledFields.has(topLevelField(e.field_path))
  )

  // ── Compute visible node set ────────────────────────────────────────────
  const touched = new Set<string>()
  for (const e of activeEdges) { touched.add(e.source); touched.add(e.target) }

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
        if (!visibleSet.has(nb)) { visibleSet.add(nb); queue.push(nb) }
      }
    }
    activeEdges = activeEdges.filter(e => visibleSet.has(e.source) && visibleSet.has(e.target))
  } else {
    visibleSet = touched
  }

  const visibleNodes = graph.nodes.filter(n => visibleSet.has(n.id))

  const forcedRoots = new Set<string>(
    activeType
      ? visibleNodes
          .filter(n => n.in_focus_file && n.actual_type === activeType)
          .map(n => n.id)
      : []
  )

  const layer = layerByLongestPath(visibleNodes, activeEdges, forcedRoots)

  // ── Group real nodes by layer ───────────────────────────────────────────
  const layerToNodes = new Map<number, GraphNode[]>()
  for (const n of visibleNodes) {
    const l = layer.get(n.id) ?? 0
    if (!layerToNodes.has(l)) layerToNodes.set(l, [])
    layerToNodes.get(l)!.push(n)
  }
  // Stable initial order: focus file first, then file group, then key
  for (const [, list] of layerToNodes) {
    list.sort((a, b) => {
      if (a.in_focus_file !== b.in_focus_file) return a.in_focus_file ? -1 : 1
      if (a.file_path !== b.file_path) return a.file_path.localeCompare(b.file_path)
      return a.key.localeCompare(b.key)
    })
  }

  const layers = Array.from(layerToNodes.keys()).sort((a, b) => a - b)

  // ── Barycenter ordering with dummy nodes ────────────────────────────────
  // For edges that span > 1 layer, insert invisible dummy nodes at each
  // intermediate layer so the barycenter heuristic accounts for long edges.
  // Dummies only exist in bcLayers/bcAdjIn/bcAdjOut; they are never rendered.

  interface BcItem { id: string }
  const bcLayers = new Map<number, BcItem[]>()
  for (const [l, nodes] of layerToNodes) {
    bcLayers.set(l, nodes.map(n => ({ id: n.id })))
  }

  const bcAdjOut = new Map<string, string[]>()
  const bcAdjIn = new Map<string, string[]>()

  for (const e of activeEdges) {
    const sl = layer.get(e.source) ?? 0
    const tl = layer.get(e.target) ?? 0

    if (tl - sl <= 1) {
      if (!bcAdjOut.has(e.source)) bcAdjOut.set(e.source, [])
      bcAdjOut.get(e.source)!.push(e.target)
      if (!bcAdjIn.has(e.target)) bcAdjIn.set(e.target, [])
      bcAdjIn.get(e.target)!.push(e.source)
    } else {
      // Chain: source → d_{sl+1} → … → d_{tl-1} → target
      let prev = e.source
      for (let dl = sl + 1; dl < tl; dl++) {
        const dId = `__d_${e.source}_${e.target}_${dl}`
        if (!bcLayers.has(dl)) bcLayers.set(dl, [])
        bcLayers.get(dl)!.push({ id: dId })
        if (!bcAdjOut.has(prev)) bcAdjOut.set(prev, [])
        bcAdjOut.get(prev)!.push(dId)
        if (!bcAdjIn.has(dId)) bcAdjIn.set(dId, [])
        bcAdjIn.get(dId)!.push(prev)
        prev = dId
      }
      if (!bcAdjOut.has(prev)) bcAdjOut.set(prev, [])
      bcAdjOut.get(prev)!.push(e.target)
      if (!bcAdjIn.has(e.target)) bcAdjIn.set(e.target, [])
      bcAdjIn.get(e.target)!.push(prev)
    }
  }

  const bcLayerList = Array.from(bcLayers.keys()).sort((a, b) => a - b)

  function bcIndexOf(id: string, list: BcItem[]): number {
    for (let i = 0; i < list.length; i++) if (list[i].id === id) return i
    return -1
  }

  function bcMeanIndex(id: string, neighbors: string[] | undefined, nl: number): number {
    if (!neighbors || neighbors.length === 0) return Number.POSITIVE_INFINITY
    const list = bcLayers.get(nl)
    if (!list) return Number.POSITIVE_INFINITY
    let sum = 0, n = 0
    for (const nb of neighbors) {
      const idx = bcIndexOf(nb, list)
      if (idx >= 0) { sum += idx; n++ }
    }
    return n === 0 ? Number.POSITIVE_INFINITY : sum / n
  }

  const ITER = 24
  for (let it = 0; it < ITER; it++) {
    // Forward pass: order by mean index of predecessors
    for (let i = 1; i < bcLayerList.length; i++) {
      const cur = bcLayerList[i]
      const prev = bcLayerList[i - 1]
      const list = bcLayers.get(cur)!
      const ranks = new Map<string, number>()
      for (const item of list) ranks.set(item.id, bcMeanIndex(item.id, bcAdjIn.get(item.id), prev))
      list.sort((a, b) => {
        const ra = ranks.get(a.id)!; const rb = ranks.get(b.id)!
        return ra !== rb ? ra - rb : a.id.localeCompare(b.id)
      })
    }
    // Backward pass: order by mean index of successors
    for (let i = bcLayerList.length - 2; i >= 0; i--) {
      const cur = bcLayerList[i]
      const next = bcLayerList[i + 1]
      const list = bcLayers.get(cur)!
      const ranks = new Map<string, number>()
      for (const item of list) ranks.set(item.id, bcMeanIndex(item.id, bcAdjOut.get(item.id), next))
      list.sort((a, b) => {
        const ra = ranks.get(a.id)!; const rb = ranks.get(b.id)!
        return ra !== rb ? ra - rb : a.id.localeCompare(b.id)
      })
    }
  }

  // Extract real-node ordering from bcLayers back to layerToNodes
  for (const [l, bcList] of bcLayers) {
    const realNodes = layerToNodes.get(l)
    if (!realNodes) continue
    const orderedIds = bcList.filter(x => !x.id.startsWith('__d_')).map(x => x.id)
    const order = new Map<string, number>()
    orderedIds.forEach((id, i) => order.set(id, i))
    realNodes.sort((a, b) => (order.get(a.id) ?? 0) - (order.get(b.id) ?? 0))
  }

  // ── Position assignment ─────────────────────────────────────────────────
  // Adjacency for y-refinement uses real edges only (no dummies).
  const adjOut = new Map<string, string[]>()
  const adjIn = new Map<string, string[]>()
  for (const e of activeEdges) {
    if (!adjOut.has(e.source)) adjOut.set(e.source, [])
    adjOut.get(e.source)!.push(e.target)
    if (!adjIn.has(e.target)) adjIn.set(e.target, [])
    adjIn.get(e.target)!.push(e.source)
  }

  const positions = new Map<string, { x: number; y: number }>()

  // Pass 1: slot-based y using per-node height estimates
  for (const l of layers) {
    const list = layerToNodes.get(l)!
    const colX = l * (NODE_W + COL_GAP)
    const heights = list.map(n => estimateNodeHeight(n, nodeExpandedMap.get(n.id) ?? false))
    const totalH = heights.reduce((s, h) => s + h, 0) + (list.length - 1) * ROW_GAP
    let curY = -totalH / 2
    list.forEach((n, i) => {
      positions.set(n.id, { x: colX, y: curY })
      curY += heights[i] + ROW_GAP
    })
  }

  // Pass 2+: snap each node toward average y of its neighbours,
  // preserving barycenter order and maintaining minimum spacing.
  function refineYByNeighbours() {
    for (const l of layers) {
      const list = layerToNodes.get(l)!
      const heights = list.map(n => estimateNodeHeight(n, nodeExpandedMap.get(n.id) ?? false))
      const desired = list.map(n => {
        const ins = (adjIn.get(n.id) ?? []).map(id => positions.get(id)?.y).filter((y): y is number => y !== undefined)
        const outs = (adjOut.get(n.id) ?? []).map(id => positions.get(id)?.y).filter((y): y is number => y !== undefined)
        const all = [...ins, ...outs]
        if (all.length === 0) return positions.get(n.id)!.y
        return all.reduce((a, b) => a + b, 0) / all.length
      })
      // Sort by desired y, then clamp to non-overlapping using actual heights
      const sorted = list.map((n, i) => ({ n, want: desired[i], h: heights[i] }))
        .sort((a, b) => a.want - b.want)
      let prevBottom = -Infinity
      for (const item of sorted) {
        const y = Math.max(item.want, prevBottom + ROW_GAP)
        positions.set(item.n.id, { x: positions.get(item.n.id)!.x, y })
        prevBottom = y + item.h
      }
    }
  }
  for (let i = 0; i < 3; i++) refineYByNeighbours()

  return { positions, visibleNodes, activeEdges }
}

// ─── Component ───────────────────────────────────────────────────────────────

interface Props {
  graphData: GraphData
  activeType?: string
  onOpenRecord: (file: string, key: string) => void
}

export function GraphView({ graphData, activeType, onOpenRecord }: Props) {
  const allFields = useMemo(() => {
    const set = new Set<string>()
    for (const e of graphData.edges) set.add(topLevelField(e.field_path))
    return Array.from(set).sort()
  }, [graphData])

  const [enabledFields, setEnabledFields] = useState<Set<string>>(() => new Set(allFields))

  // Re-sync enabled set when graph changes (keep previously enabled fields)
  useMemo(() => {
    setEnabledFields(prev => {
      const next = new Set<string>()
      for (const f of allFields) if (prev.has(f) || prev.size === 0) next.add(f)
      return next
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [allFields.join('|')])

  // Per-node showAll state — lifted here so layout recalculates on expand/collapse
  const [nodeExpandedMap, setNodeExpandedMap] = useState<Map<string, boolean>>(new Map())
  // Hovered node id — used for edge highlighting only, does not re-trigger layout
  const [hoveredId, setHoveredId] = useState<string | null>(null)

  const toggleNodeExpanded = useCallback((id: string) => {
    setNodeExpandedMap(prev => {
      const next = new Map(prev)
      next.set(id, !(prev.get(id) ?? false))
      return next
    })
  }, [])

  const { positions, visibleNodes, activeEdges } = useMemo(
    () => layoutNodes(graphData, enabledFields, activeType, nodeExpandedMap),
    [graphData, enabledFields, activeType, nodeExpandedMap]
  )

  const rfNodes: Node[] = useMemo(
    () => visibleNodes.map(n => ({
      id: n.id,
      type: 'cfd',
      position: positions.get(n.id) ?? { x: 0, y: 0 },
      data: {
        graphNode: n,
        expanded: nodeExpandedMap.get(n.id) ?? false,
        onToggleExpand: () => toggleNodeExpanded(n.id),
      } satisfies NodeData,
    })),
    [visibleNodes, positions, nodeExpandedMap, toggleNodeExpanded]
  )

  const rfEdges: Edge[] = useMemo(() => {
    const base = activeEdges
      .filter(e => positions.has(e.source) && positions.has(e.target))
      .map((e, i) => ({
        id: `e${i}`,
        source: e.source,
        target: e.target,
        label: e.field_path,
        type: 'bezier',
        animated: false,
        labelStyle: { fill: '#7a828f', fontSize: 10, fontFamily: 'JetBrains Mono, monospace' },
        labelBgStyle: { fill: '#1a1e25', fillOpacity: 0.92 },
        labelBgPadding: [4, 2] as [number, number],
        labelBgBorderRadius: 3,
      }))

    if (!hoveredId) {
      return base.map(e => ({ ...e, style: { stroke: '#4a525e', strokeWidth: 1.2 } }))
    }
    return base.map(e => {
      const connected = e.source === hoveredId || e.target === hoveredId
      return {
        ...e,
        style: connected
          ? { stroke: '#8aa8d4', strokeWidth: 2 }
          : { stroke: '#4a525e', strokeWidth: 1.2, opacity: 0.15 },
        zIndex: connected ? 1000 : 0,
      }
    })
  }, [activeEdges, positions, hoveredId])

  const onNodeDoubleClick = useCallback((_: unknown, node: Node) => {
    const { graphNode } = node.data as NodeData
    onOpenRecord(graphNode.file_path, graphNode.key)
  }, [onOpenRecord])

  const onNodeMouseEnter = useCallback((_: unknown, node: Node) => {
    setHoveredId(node.id)
  }, [])

  const onNodeMouseLeave = useCallback(() => {
    setHoveredId(null)
  }, [])

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
        {rfNodes.length === 0 ? (
          <div className="empty-hint">无可显示的引用关系</div>
        ) : (
          <ReactFlow
            nodes={rfNodes}
            edges={rfEdges}
            nodeTypes={nodeTypes}
            onNodeDoubleClick={onNodeDoubleClick}
            onNodeMouseEnter={onNodeMouseEnter}
            onNodeMouseLeave={onNodeMouseLeave}
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
