import { useCallback, useMemo, useState } from 'react'
import {
  ReactFlow, Background, Controls, MiniMap,
  Handle, Position, type NodeProps,
  type Node, type Edge,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import type { GraphData, GraphNode, FieldCell } from '../bindings/index'
import { DataCardNode, NODE_PEEK_FIELDS, countVisibleRows } from './DataCard'
import { Icon } from './Icon'

// ─── Constants (must match CSS / DataCard) ──────────────────────────────────

const NODE_W     = 280
const COL_GAP    = 200
const ROW_GAP    = 60      // gap between nodes in same column
const COMP_GAP   = 80      // vertical gap between connected components
const HEADER_H   = 42
const ROW_H      = 22
const MORE_BTN_H = 28
const PAD_V      = 12

// ─── Node data ───────────────────────────────────────────────────────────────

interface NodeData extends Record<string, unknown> {
  graphNode: GraphNode
  expanded: boolean
  onToggleExpand: () => void
  onRowToggle: (path: string, exp: boolean) => void
}

// ─── Per-field handles ───────────────────────────────────────────────────────
// When expanded, we render one source handle per visible Ref field so edges
// attach to the right row. When collapsed the single default handle is used.

function refFieldHandles(fields: FieldCell[], expanded: boolean, showAll: boolean) {
  if (!expanded) return null
  const visible = showAll ? fields : fields.slice(0, NODE_PEEK_FIELDS)
  return visible.map((f, i) => {
    if (f.value.kind !== 'Ref') return null
    // Approximate y: header + rows before this one, centered on row
    const offsetY = HEADER_H + i * ROW_H + ROW_H / 2
    return (
      <Handle
        key={`src-${f.name}`}
        type="source"
        position={Position.Right}
        id={`field-${f.name}`}
        style={{ top: offsetY, bottom: 'auto' }}
      />
    )
  })
}

// ─── CfdNode ─────────────────────────────────────────────────────────────────

function CfdNode({ data }: NodeProps) {
  const { graphNode: gn, expanded, onToggleExpand, onRowToggle } = data as NodeData

  return (
    <div className={`graph-node${gn.in_focus_file ? ' focused' : ' dim'}`}>
      <Handle type="target" position={Position.Left} id="__in" />
      {/* Per-field source handles when expanded */}
      {refFieldHandles(gn.fields, expanded, expanded)}
      {/* Default source handle (collapsed or no-ref nodes) */}
      <Handle
        type="source"
        position={Position.Right}
        id="__out"
        style={expanded ? { opacity: 0, pointerEvents: 'none' } : {}}
      />
      <div className="gn-header">
        <span className="gn-type">{gn.actual_type}</span>
        <span className="gn-key">{gn.key}</span>
        <span className="gn-file">{gn.file_path.split('/').pop()}</span>
      </div>
      {gn.is_collapsed ? (
        <div className="gn-collapsed">折叠（超出深度）</div>
      ) : (
        <DataCardNode
          fields={gn.fields}
          showAll={expanded}
          onToggle={onToggleExpand}
          onRowToggle={onRowToggle}
        />
      )}
    </div>
  )
}

const nodeTypes = { cfd: CfdNode }

// ─── Height estimation ────────────────────────────────────────────────────────

function estimateNodeHeight(
  gn: GraphNode,
  expanded: boolean,
  expandedRows: Set<string>,
): number {
  if (gn.is_collapsed) return HEADER_H + 28 + PAD_V
  if (!expanded) {
    const visible = Math.min(NODE_PEEK_FIELDS, gn.fields.length)
    const hasMore = gn.fields.length > NODE_PEEK_FIELDS
    return HEADER_H + visible * ROW_H + (hasMore ? MORE_BTN_H : 0) + PAD_V
  }
  const rows = countVisibleRows(gn.fields, expandedRows)
  const hasMore = gn.fields.length > NODE_PEEK_FIELDS
  return HEADER_H + rows * ROW_H + (hasMore ? MORE_BTN_H : 0) + PAD_V
}

// ─── Field path → top-level field name ───────────────────────────────────────

function topLevelField(path: string): string {
  const m = path.match(/^[^.[]+/)
  return m ? m[0] : path
}

// ─── Connected components ─────────────────────────────────────────────────────

function connectedComponents(
  nodeIds: string[],
  edges: { source: string; target: string }[],
): string[][] {
  const adj = new Map<string, Set<string>>()
  for (const id of nodeIds) adj.set(id, new Set())
  for (const e of edges) {
    adj.get(e.source)?.add(e.target)
    adj.get(e.target)?.add(e.source)
  }
  const visited = new Set<string>()
  const comps: string[][] = []
  for (const id of nodeIds) {
    if (visited.has(id)) continue
    const comp: string[] = []
    const q = [id]
    while (q.length) {
      const cur = q.shift()!
      if (visited.has(cur)) continue
      visited.add(cur)
      comp.push(cur)
      for (const nb of adj.get(cur) ?? []) if (!visited.has(nb)) q.push(nb)
    }
    comps.push(comp)
  }
  return comps
}

// ─── Back-edge detection (DFS, breaks cycles) ────────────────────────────────

function detectBackEdges(
  nodes: { id: string }[],
  edges: { source: string; target: string }[],
): Set<string> {
  const adj = new Map<string, string[]>()
  for (const n of nodes) adj.set(n.id, [])
  for (const e of edges) adj.get(e.source)?.push(e.target)

  const state = new Map<string, 'white' | 'gray' | 'black'>()
  for (const n of nodes) state.set(n.id, 'white')
  const backEdgeKeys = new Set<string>()

  function dfs(id: string) {
    state.set(id, 'gray')
    for (const nb of adj.get(id) ?? []) {
      if (state.get(nb) === 'gray') {
        backEdgeKeys.add(`${id}→${nb}`)
      } else if (state.get(nb) === 'white') {
        dfs(nb)
      }
    }
    state.set(id, 'black')
  }
  for (const n of nodes) if (state.get(n.id) === 'white') dfs(n.id)
  return backEdgeKeys
}

// ─── Longest-path layering ────────────────────────────────────────────────────

function layerByLongestPath(
  nodes: { id: string }[],
  edges: { source: string; target: string }[],
  forcedRoots: Set<string>,
): Map<string, number> {
  const incoming = new Map<string, string[]>()
  for (const n of nodes) incoming.set(n.id, [])
  for (const e of edges) incoming.get(e.target)?.push(e.source)

  const layer = new Map<string, number>()
  const inProg = new Set<string>()

  function visit(id: string): number {
    if (layer.has(id)) return layer.get(id)!
    if (forcedRoots.has(id)) { layer.set(id, 0); return 0 }
    if (inProg.has(id)) return 0
    inProg.add(id)
    let max = 0
    for (const pred of incoming.get(id) ?? []) {
      const l = visit(pred) + 1
      if (l > max) max = l
    }
    inProg.delete(id)
    layer.set(id, max)
    return max
  }
  for (const n of nodes) visit(n.id)
  return layer
}

// ─── Barycenter with dummy nodes ──────────────────────────────────────────────

function barycenterOrder(
  layerToNodes: Map<number, { id: string }[]>,
  forwardEdges: { source: string; target: string }[],
  layerMap: Map<string, number>,
  layers: number[],
) {
  interface BcItem { id: string }
  const bcLayers = new Map<number, BcItem[]>()
  for (const [l, ns] of layerToNodes) bcLayers.set(l, ns.map(n => ({ id: n.id })))

  const bcOut = new Map<string, string[]>()
  const bcIn  = new Map<string, string[]>()

  for (const e of forwardEdges) {
    const sl = layerMap.get(e.source) ?? 0
    const tl = layerMap.get(e.target) ?? 0

    if (tl - sl <= 1) {
      ;(bcOut.get(e.source) ?? (bcOut.set(e.source, []), bcOut.get(e.source)!)).push(e.target)
      ;(bcIn.get(e.target)  ?? (bcIn.set(e.target, []),  bcIn.get(e.target)!)).push(e.source)
    } else {
      let prev = e.source
      for (let dl = sl + 1; dl < tl; dl++) {
        const dId = `__d_${e.source}_${e.target}_${dl}`
        if (!bcLayers.has(dl)) bcLayers.set(dl, [])
        bcLayers.get(dl)!.push({ id: dId })
        ;(bcOut.get(prev)  ?? (bcOut.set(prev,  []), bcOut.get(prev)!)).push(dId)
        ;(bcIn.get(dId)    ?? (bcIn.set(dId,    []), bcIn.get(dId)!)).push(prev)
        prev = dId
      }
      ;(bcOut.get(prev)      ?? (bcOut.set(prev,      []), bcOut.get(prev)!)).push(e.target)
      ;(bcIn.get(e.target)   ?? (bcIn.set(e.target,   []), bcIn.get(e.target)!)).push(prev)
    }
  }

  const bcLayerList = Array.from(bcLayers.keys()).sort((a, b) => a - b)

  function idx(id: string, list: BcItem[]) { return list.findIndex(x => x.id === id) }
  function meanIdx(id: string, nbs: string[] | undefined, nl: number) {
    if (!nbs?.length) return Infinity
    const list = bcLayers.get(nl); if (!list) return Infinity
    let s = 0, n = 0
    for (const nb of nbs) { const i = idx(nb, list); if (i >= 0) { s += i; n++ } }
    return n === 0 ? Infinity : s / n
  }

  for (let it = 0; it < 24; it++) {
    for (let i = 1; i < bcLayerList.length; i++) {
      const cur = bcLayerList[i], prev = bcLayerList[i - 1]
      const list = bcLayers.get(cur)!
      list.sort((a, b) => {
        const d = meanIdx(a.id, bcIn.get(a.id), prev) - meanIdx(b.id, bcIn.get(b.id), prev)
        return d !== 0 ? d : a.id.localeCompare(b.id)
      })
    }
    for (let i = bcLayerList.length - 2; i >= 0; i--) {
      const cur = bcLayerList[i], next = bcLayerList[i + 1]
      const list = bcLayers.get(cur)!
      list.sort((a, b) => {
        const d = meanIdx(a.id, bcOut.get(a.id), next) - meanIdx(b.id, bcOut.get(b.id), next)
        return d !== 0 ? d : a.id.localeCompare(b.id)
      })
    }
  }

  // Write ordering back to real layers (drop dummy nodes)
  for (const l of layers) {
    const real = layerToNodes.get(l)
    if (!real) continue
    const ordered = (bcLayers.get(l) ?? [])
      .filter(x => !x.id.startsWith('__d_'))
      .map(x => x.id)
    const order = new Map(ordered.map((id, i) => [id, i]))
    real.sort((a, b) => (order.get(a.id) ?? 0) - (order.get(b.id) ?? 0))
  }
}

// ─── Layout one connected component ──────────────────────────────────────────

function layoutComponent(
  compNodes: GraphNode[],
  forwardEdges: { source: string; target: string; field_path: string }[],
  forcedRoots: Set<string>,
  nodeExpandedMap: Map<string, boolean>,
  nodeRowExpandedMap: Map<string, Set<string>>,
): Map<string, { x: number; y: number }> {
  const layer = layerByLongestPath(compNodes, forwardEdges, forcedRoots)

  const layerToNodes = new Map<number, GraphNode[]>()
  for (const n of compNodes) {
    const l = layer.get(n.id) ?? 0
    if (!layerToNodes.has(l)) layerToNodes.set(l, [])
    layerToNodes.get(l)!.push(n)
  }
  for (const [, list] of layerToNodes) {
    list.sort((a, b) => {
      if (a.in_focus_file !== b.in_focus_file) return a.in_focus_file ? -1 : 1
      if (a.file_path !== b.file_path) return a.file_path.localeCompare(b.file_path)
      return a.key.localeCompare(b.key)
    })
  }

  const layers = Array.from(layerToNodes.keys()).sort((a, b) => a - b)
  barycenterOrder(layerToNodes, forwardEdges, layer, layers)

  const adjOut = new Map<string, string[]>()
  const adjIn  = new Map<string, string[]>()
  for (const e of forwardEdges) {
    ;(adjOut.get(e.source) ?? (adjOut.set(e.source, []), adjOut.get(e.source)!)).push(e.target)
    ;(adjIn.get(e.target)  ?? (adjIn.set(e.target,  []), adjIn.get(e.target)!)).push(e.source)
  }

  const positions = new Map<string, { x: number; y: number }>()

  function nodeH(n: GraphNode) {
    const exp = nodeExpandedMap.get(n.id) ?? false
    const rows = nodeRowExpandedMap.get(n.id) ?? new Set<string>()
    return estimateNodeHeight(n, exp, rows)
  }

  // Initial slot-based y
  for (const l of layers) {
    const list = layerToNodes.get(l)!
    const colX = l * (NODE_W + COL_GAP)
    const heights = list.map(n => nodeH(n))
    const totalH = heights.reduce((s, h) => s + h, 0) + (list.length - 1) * ROW_GAP
    let curY = -totalH / 2
    list.forEach((n, i) => {
      positions.set(n.id, { x: colX, y: curY })
      curY += heights[i] + ROW_GAP
    })
  }

  // Y-refinement: snap toward neighbour average
  function refine() {
    for (const l of layers) {
      const list = layerToNodes.get(l)!
      const heights = list.map(n => nodeH(n))
      const desired = list.map(n => {
        const ins  = (adjIn.get(n.id)  ?? []).map(id => positions.get(id)?.y).filter((y): y is number => y !== undefined)
        const outs = (adjOut.get(n.id) ?? []).map(id => positions.get(id)?.y).filter((y): y is number => y !== undefined)
        const all = [...ins, ...outs]
        if (!all.length) return positions.get(n.id)!.y
        return all.reduce((a, b) => a + b, 0) / all.length
      })
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
  for (let i = 0; i < 3; i++) refine()

  return positions
}

// ─── Full layout (all components, stacked vertically) ────────────────────────

interface LayoutResult {
  positions: Map<string, { x: number; y: number }>
  visibleNodes: GraphNode[]
  forwardEdges: { source: string; target: string; field_path: string }[]
  backEdges:    { source: string; target: string; field_path: string }[]
}

function layoutAll(
  graph: GraphData,
  enabledFields: Set<string>,
  activeType: string | undefined,
  nodeExpandedMap: Map<string, boolean>,
  nodeRowExpandedMap: Map<string, Set<string>>,
): LayoutResult {
  // Filter edges by toolbar
  let activeEdges = graph.edges.filter(e => enabledFields.has(topLevelField(e.field_path)))

  // Compute visible set
  const touched = new Set<string>()
  for (const e of activeEdges) { touched.add(e.source); touched.add(e.target) }

  let visibleSet: Set<string>
  if (activeType) {
    const roots = graph.nodes
      .filter(n => n.in_focus_file && n.actual_type === activeType && touched.has(n.id))
      .map(n => n.id)
    visibleSet = new Set(roots)
    const out = new Map<string, string[]>()
    for (const e of activeEdges) {
      ;(out.get(e.source) ?? (out.set(e.source, []), out.get(e.source)!)).push(e.target)
    }
    const q = [...roots]
    while (q.length) {
      const cur = q.shift()!
      for (const nb of out.get(cur) ?? []) {
        if (!visibleSet.has(nb)) { visibleSet.add(nb); q.push(nb) }
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

  // Detect back-edges (cycles)
  const backEdgeKeys = detectBackEdges(visibleNodes, activeEdges)
  const forwardEdges = activeEdges.filter(e => !backEdgeKeys.has(`${e.source}→${e.target}`))
  const backEdges    = activeEdges.filter(e =>  backEdgeKeys.has(`${e.source}→${e.target}`))

  // Split into connected components using ALL edges (forward + back)
  const comps = connectedComponents(visibleNodes.map(n => n.id), activeEdges)
  const nodeToComp = new Map<string, number>()
  comps.forEach((comp, ci) => comp.forEach(id => nodeToComp.set(id, ci)))
  const nodeById = new Map(visibleNodes.map(n => [n.id, n]))

  // Sort components: largest first, then by first node key
  comps.sort((a, b) => {
    if (b.length !== a.length) return b.length - a.length
    return a[0].localeCompare(b[0])
  })

  const allPositions = new Map<string, { x: number; y: number }>()
  let yOffset = 0

  for (const comp of comps) {
    const compNodes = comp.map(id => nodeById.get(id)!).filter(Boolean)
    const compForward = forwardEdges.filter(e =>
      visibleSet.has(e.source) && visibleSet.has(e.target) &&
      nodeToComp.get(e.source) === nodeToComp.get(compNodes[0]?.id)
    )
    const compForcedRoots = new Set(comp.filter(id => forcedRoots.has(id)))
    const localPos = layoutComponent(compNodes, compForward, compForcedRoots, nodeExpandedMap, nodeRowExpandedMap)

    // Find y-extent of this component's layout
    let minY = Infinity, maxY = -Infinity
    for (const { y } of localPos.values()) { if (y < minY) minY = y; if (y > maxY) maxY = y }
    // Shift so component starts at yOffset (minY maps to yOffset)
    const shift = yOffset - minY
    for (const [id, pos] of localPos) {
      allPositions.set(id, { x: pos.x, y: pos.y + shift })
    }
    // Advance yOffset past this component + gap
    // maxY + shift = old maxY mapped to new coords; add max node height estimate + gap
    const compHeight = maxY - minY + 300 // 300 is generous max node height
    yOffset += compHeight + COMP_GAP
  }

  return { positions: allPositions, visibleNodes, forwardEdges, backEdges }
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
  useMemo(() => {
    setEnabledFields(prev => {
      const next = new Set<string>()
      for (const f of allFields) if (prev.has(f) || prev.size === 0) next.add(f)
      return next
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [allFields.join('|')])

  // Expand states lifted here so layout recalcs on any change
  const [nodeExpandedMap, setNodeExpandedMap] = useState<Map<string, boolean>>(new Map())
  // Per-node set of expanded sub-row paths
  const [nodeRowExpandedMap, setNodeRowExpandedMap] = useState<Map<string, Set<string>>>(new Map())
  const [hoveredId, setHoveredId] = useState<string | null>(null)

  const toggleNodeExpanded = useCallback((id: string) => {
    setNodeExpandedMap(prev => {
      const next = new Map(prev)
      next.set(id, !(prev.get(id) ?? false))
      return next
    })
  }, [])

  const handleRowToggle = useCallback((nodeId: string, path: string, expanded: boolean) => {
    setNodeRowExpandedMap(prev => {
      const next = new Map(prev)
      const set = new Set(prev.get(nodeId) ?? [])
      if (expanded) set.add(path)
      else set.delete(path)
      next.set(nodeId, set)
      return next
    })
  }, [])

  const { positions, visibleNodes, forwardEdges, backEdges } = useMemo(
    () => layoutAll(graphData, enabledFields, activeType, nodeExpandedMap, nodeRowExpandedMap),
    [graphData, enabledFields, activeType, nodeExpandedMap, nodeRowExpandedMap]
  )

  // Build edge sourceHandle: if source node is expanded and edge field matches a visible ref field,
  // use the per-field handle; otherwise use the default __out handle.
  function edgeHandleId(
    sourceId: string,
    fieldPath: string,
  ): { sourceHandle: string; targetHandle: string } {
    const expanded = nodeExpandedMap.get(sourceId) ?? false
    if (!expanded) return { sourceHandle: '__out', targetHandle: '__in' }
    const top = topLevelField(fieldPath)
    // Check if the source node has a ref field with this name in visible rows
    const srcNode = visibleNodes.find(n => n.id === sourceId)
    if (!srcNode) return { sourceHandle: '__out', targetHandle: '__in' }
    const visible = srcNode.fields.slice(0, nodeExpandedMap.get(sourceId) ? undefined : NODE_PEEK_FIELDS)
    const match = visible.find(f => f.name === top && f.value.kind === 'Ref')
    return match
      ? { sourceHandle: `field-${top}`, targetHandle: '__in' }
      : { sourceHandle: '__out', targetHandle: '__in' }
  }

  const rfNodes: Node[] = useMemo(
    () => visibleNodes.map(n => ({
      id: n.id,
      type: 'cfd',
      position: positions.get(n.id) ?? { x: 0, y: 0 },
      data: {
        graphNode: n,
        expanded: nodeExpandedMap.get(n.id) ?? false,
        onToggleExpand: () => toggleNodeExpanded(n.id),
        onRowToggle: (path: string, exp: boolean) => handleRowToggle(n.id, path, exp),
      } satisfies NodeData,
    })),
    [visibleNodes, positions, nodeExpandedMap, toggleNodeExpanded, handleRowToggle]
  )

  const rfEdges: Edge[] = useMemo(() => {
    const fwdEdges: Edge[] = forwardEdges
      .filter(e => positions.has(e.source) && positions.has(e.target))
      .map((e, i) => {
        const { sourceHandle, targetHandle } = edgeHandleId(e.source, e.field_path)
        const connected = hoveredId && (e.source === hoveredId || e.target === hoveredId)
        return {
          id: `f${i}`,
          source: e.source,
          target: e.target,
          sourceHandle,
          targetHandle,
          label: e.field_path,
          type: 'bezier',
          animated: false,
          style: connected
            ? { stroke: '#8aa8d4', strokeWidth: 2 }
            : hoveredId
              ? { stroke: '#4a525e', strokeWidth: 1.2, opacity: 0.15 }
              : { stroke: '#4a525e', strokeWidth: 1.2 },
          zIndex: connected ? 1000 : 0,
          labelStyle: { fill: '#7a828f', fontSize: 10, fontFamily: 'JetBrains Mono, monospace' },
          labelBgStyle: { fill: '#1a1e25', fillOpacity: 0.92 },
          labelBgPadding: [4, 2] as [number, number],
          labelBgBorderRadius: 3,
        }
      })

    // Back-edges: route above/below to avoid overlapping forward edges.
    // Use type='bezier' with elevated zIndex and dashed stroke.
    const bkEdges: Edge[] = backEdges
      .filter(e => positions.has(e.source) && positions.has(e.target))
      .map((e, i) => {
        const connected = hoveredId && (e.source === hoveredId || e.target === hoveredId)
        return {
          id: `b${i}`,
          source: e.source,
          target: e.target,
          sourceHandle: '__out',
          targetHandle: '__in',
          label: e.field_path,
          type: 'bezier',
          animated: false,
          style: connected
            ? { stroke: '#d97a7a', strokeWidth: 2, strokeDasharray: '6 3' }
            : hoveredId
              ? { stroke: '#d97a7a', strokeWidth: 1.2, opacity: 0.2, strokeDasharray: '6 3' }
              : { stroke: '#d97a7a', strokeWidth: 1.2, opacity: 0.6, strokeDasharray: '6 3' },
          zIndex: connected ? 1100 : 1,
          labelStyle: { fill: '#d97a7a', fontSize: 10, fontFamily: 'JetBrains Mono, monospace' },
          labelBgStyle: { fill: '#1a1e25', fillOpacity: 0.92 },
          labelBgPadding: [4, 2] as [number, number],
          labelBgBorderRadius: 3,
          markerEnd: undefined,
        }
      })

    return [...fwdEdges, ...bkEdges]
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [forwardEdges, backEdges, positions, hoveredId, nodeExpandedMap, visibleNodes])

  const onNodeDoubleClick = useCallback((_: unknown, node: Node) => {
    const { graphNode } = node.data as NodeData
    onOpenRecord(graphNode.file_path, graphNode.key)
  }, [onOpenRecord])

  const onNodeMouseEnter = useCallback((_: unknown, node: Node) => setHoveredId(node.id), [])
  const onNodeMouseLeave = useCallback(() => setHoveredId(null), [])

  function toggleField(name: string) {
    setEnabledFields(prev => {
      const next = new Set(prev)
      if (next.has(name)) next.delete(name); else next.add(name)
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
            fitViewOptions={{ padding: 0.15, minZoom: 0.2, maxZoom: 1.2 }}
            proOptions={{ hideAttribution: true }}
            minZoom={0.1}
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
