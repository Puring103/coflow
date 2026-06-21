import { useCallback, useLayoutEffect, useMemo, useRef, useState } from 'react'
import {
  ReactFlow, Background, Controls, MiniMap,
  Handle, Position, useUpdateNodeInternals, type NodeProps,
  type Node, type Edge,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import type { GraphData, GraphNode } from '../bindings/index'
import { DataCardNode, CardHeader, NODE_PEEK_FIELDS, countVisibleRows } from './DataCard'
import { Icon } from './Icon'
import { typeColor } from '../utils/typeColor'

// ─── Constants (must match CSS / DataCard) ────────────────────────────────── (must match CSS / DataCard) ──────────────────────────────────

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
  /** Distinct edge field_paths whose source is this node (e.g. ["unlockGeneList[0]", "unlockGeneList[1]"]) */
  outgoingPaths: string[]
  onToggleExpand: () => void
  onRowToggle: (path: string, exp: boolean) => void
}

// ─── CfdNode ─────────────────────────────────────────────────────────────────
// Per-field source handles use useLayoutEffect to measure each row's actual
// DOM offset, so handle Y positions are exact regardless of CSS padding,
// header height variation, or sub-row expansion.

function CfdNode({ id, data }: NodeProps) {
  const { graphNode: gn, expanded, outgoingPaths, onToggleExpand, onRowToggle } = data as NodeData
  const rootRef = useRef<HTMLDivElement>(null)
  const headerRef = useRef<HTMLDivElement>(null)
  const updateNodeInternals = useUpdateNodeInternals()

  // Map of edge field_path → measured Y offset (center of the matching row).
  // We try the exact path first (e.g. "unlockGeneList[1]"), and fall back to
  // the top-level field row when the array/dict is collapsed.
  const [pathOffsets, setPathOffsets] = useState<Map<string, number>>(new Map())
  const [headerCenterY, setHeaderCenterY] = useState(21)

  useLayoutEffect(() => {
    const root = rootRef.current
    if (!root) return
    const rootRect = root.getBoundingClientRect()
    const headerY = headerRef.current
      ? (() => {
          const h = headerRef.current!.getBoundingClientRect()
          return h.top - rootRect.top + h.height / 2
        })()
      : 21
    setHeaderCenterY(headerY)
    const next = new Map<string, number>()
    for (const path of outgoingPaths) {
      // Prefer exact path match (e.g. expanded array element)
      let row = root.querySelector<HTMLElement>(
        `.dc-row[data-field-path="${CSS.escape(path)}"]`,
      )
      if (!row) {
        // Fall back to top-level field row (array/dict collapsed but field visible)
        const top = path.match(/^[^.[]+/)?.[0]
        if (top) {
          row = root.querySelector<HTMLElement>(`.dc-row[data-field-name="${CSS.escape(top)}"]`)
        }
      }
      // Final fallback: header center (field hidden under "+N more")
      next.set(path, row
        ? (() => {
            const r = row.getBoundingClientRect()
            return r.top - rootRect.top + r.height / 2
          })()
        : headerY)
    }
    setPathOffsets(next)
    updateNodeInternals(id)
  }, [outgoingPaths, expanded, gn.fields, id, updateNodeInternals])

  return (
    <div
      ref={rootRef}
      className={`graph-node${gn.in_focus_file ? ' focused' : ' dim'}`}
      data-nodeid={gn.id}
      style={{'--node-color': typeColor(gn.actual_type)} as React.CSSProperties}
    >
      <Handle type="target" position={Position.Left} id="__in" style={{ top: headerCenterY }} />
      {/* One source handle per outgoing edge path, positioned at the matching row */}
      {Array.from(pathOffsets.entries()).map(([path, y]) => (
        <Handle
          key={`src-${path}`}
          type="source"
          position={Position.Right}
          id={`path-${path}`}
          style={{ top: y, bottom: 'auto' }}
        />
      ))}
      {/* Fallback source handle (used when no row could be matched) */}
      <Handle
        type="source"
        position={Position.Right}
        id="__out"
        style={{ top: headerCenterY }}
      />
      <div ref={headerRef}>
        <CardHeader recordKey={gn.key} actualType={gn.actual_type} filePath={gn.file_path} />
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
  // Fields that actually appear in the subgraph for the current activeType.
  // Mirrors layoutAll's visibility logic but ignores field filtering itself,
  // so toggling all chips off doesn't hide the chip list.
  const availableFields = useMemo(() => {
    const allEdges = graphData.edges
    let visibleSet: Set<string>
    if (activeType) {
      const touched = new Set<string>()
      for (const e of allEdges) { touched.add(e.source); touched.add(e.target) }
      const roots = graphData.nodes
        .filter(n => n.in_focus_file && n.actual_type === activeType && touched.has(n.id))
        .map(n => n.id)
      visibleSet = new Set(roots)
      const out = new Map<string, string[]>()
      for (const e of allEdges) {
        ;(out.get(e.source) ?? (out.set(e.source, []), out.get(e.source)!)).push(e.target)
      }
      const q = [...roots]
      while (q.length) {
        const cur = q.shift()!
        for (const nb of out.get(cur) ?? []) {
          if (!visibleSet.has(nb)) { visibleSet.add(nb); q.push(nb) }
        }
      }
    } else {
      visibleSet = new Set<string>()
      for (const e of allEdges) { visibleSet.add(e.source); visibleSet.add(e.target) }
    }
    const set = new Set<string>()
    for (const e of allEdges) {
      if (visibleSet.has(e.source) && visibleSet.has(e.target)) {
        set.add(topLevelField(e.field_path))
      }
    }
    return Array.from(set).sort()
  }, [graphData, activeType])

  const [enabledFields, setEnabledFields] = useState<Set<string>>(() => new Set(availableFields))
  useMemo(() => {
    setEnabledFields(prev => {
      const next = new Set<string>()
      for (const f of availableFields) if (prev.has(f) || prev.size === 0) next.add(f)
      return next
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [availableFields.join('|')])

  const [filterPanelOpen, setFilterPanelOpen] = useState(false)

  // Expand states lifted here so layout recalcs on any change
  const [nodeExpandedMap, setNodeExpandedMap] = useState<Map<string, boolean>>(new Map())
  // Per-node set of expanded sub-row paths
  const [nodeRowExpandedMap, setNodeRowExpandedMap] = useState<Map<string, Set<string>>>(new Map())

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

  // Group outgoing edge paths by source node id (used to render per-path handles).
  const outgoingPathsByNode = useMemo(() => {
    const m = new Map<string, string[]>()
    for (const e of [...forwardEdges, ...backEdges]) {
      const list = m.get(e.source) ?? []
      if (!list.includes(e.field_path)) list.push(e.field_path)
      m.set(e.source, list)
    }
    return m
  }, [forwardEdges, backEdges])

  // Edge → handle id. Each edge gets its own per-path source handle on the
  // source node (path-{field_path}). The matching <Handle> is rendered by the
  // node's CfdNode after measuring the corresponding row.
  function edgeHandleId(
    _sourceId: string,
    fieldPath: string,
  ): { sourceHandle: string; targetHandle: string } {
    return { sourceHandle: `path-${fieldPath}`, targetHandle: '__in' }
  }

  const rfNodes: Node[] = useMemo(
    () => visibleNodes.map(n => ({
      id: n.id,
      type: 'cfd',
      position: positions.get(n.id) ?? { x: 0, y: 0 },
      data: {
        graphNode: n,
        expanded: nodeExpandedMap.get(n.id) ?? false,
        outgoingPaths: outgoingPathsByNode.get(n.id) ?? [],
        onToggleExpand: () => toggleNodeExpanded(n.id),
        onRowToggle: (path: string, exp: boolean) => handleRowToggle(n.id, path, exp),
      } satisfies NodeData,
    })),
    [visibleNodes, positions, nodeExpandedMap, outgoingPathsByNode, toggleNodeExpanded, handleRowToggle]
  )

  const rfEdges: Edge[] = useMemo(() => {
    const fwdEdges: Edge[] = forwardEdges
      .filter(e => positions.has(e.source) && positions.has(e.target))
      .map((e, i) => {
        const { sourceHandle, targetHandle } = edgeHandleId(e.source, e.field_path)
        return {
          id: `f${i}`,
          source: e.source,
          target: e.target,
          sourceHandle,
          targetHandle,
          label: e.field_path,
          type: 'bezier',
          animated: false,
          className: `rf-edge rf-edge-fwd rf-src-${e.source} rf-tgt-${e.target}`,
          style: { stroke: 'var(--graph-edge)', strokeWidth: 1.2 },
          labelStyle: { fill: 'var(--graph-edge-label)', fontSize: 10, fontFamily: 'JetBrains Mono, monospace' },
          labelBgStyle: { fill: 'var(--graph-edge-label-bg)', fillOpacity: 0.92 },
          labelBgPadding: [4, 2] as [number, number],
          labelBgBorderRadius: 3,
        }
      })

    const bkEdges: Edge[] = backEdges
      .filter(e => positions.has(e.source) && positions.has(e.target))
      .map((e, i) => ({
        id: `b${i}`,
        source: e.source,
        target: e.target,
        sourceHandle: `path-${e.field_path}`,
        targetHandle: '__in',
        label: e.field_path,
        type: 'bezier',
        animated: false,
        className: `rf-edge rf-edge-bk rf-src-${e.source} rf-tgt-${e.target}`,
        style: { stroke: 'var(--graph-back-edge)', strokeWidth: 1.2, opacity: 0.6, strokeDasharray: '6 3' },
        zIndex: 1,
        labelStyle: { fill: 'var(--graph-back-edge)', fontSize: 10, fontFamily: 'JetBrains Mono, monospace' },
        labelBgStyle: { fill: 'var(--graph-edge-label-bg)', fillOpacity: 0.92 },
        labelBgPadding: [4, 2] as [number, number],
        labelBgBorderRadius: 3,
      }))

    return [...fwdEdges, ...bkEdges]
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [forwardEdges, backEdges, positions, nodeExpandedMap, visibleNodes])

  // ── Imperative hover highlight (zero re-renders) ────────────────────────
  // We manipulate DOM classes directly to avoid the state→rerender→mouseleave
  // flicker cycle. The adjacency map is rebuilt whenever edges change.
  const wrapRef = useRef<HTMLDivElement>(null)

  // nodeId → set of nodeIds it is directly connected to
  const adjacencyRef = useRef<Map<string, Set<string>>>(new Map())
  useMemo(() => {
    const adj = new Map<string, Set<string>>()
    for (const e of [...forwardEdges, ...backEdges]) {
      if (!adj.has(e.source)) adj.set(e.source, new Set())
      if (!adj.has(e.target)) adj.set(e.target, new Set())
      adj.get(e.source)!.add(e.target)
      adj.get(e.target)!.add(e.source)
    }
    adjacencyRef.current = adj
  }, [forwardEdges, backEdges])

  const onNodeDoubleClick = useCallback((_: unknown, node: Node) => {
    const { graphNode } = node.data as NodeData
    onOpenRecord(graphNode.file_path, graphNode.key)
  }, [onOpenRecord])

  const onNodeMouseEnter = useCallback((_: unknown, node: Node) => {
    const wrap = wrapRef.current
    if (!wrap) return
    const hovId = node.id
    const neighbors = adjacencyRef.current.get(hovId) ?? new Set<string>()

    wrap.classList.add('is-hovering')

    // Highlight hovered node + neighbors
    wrap.querySelectorAll<HTMLElement>('.graph-node').forEach(el => {
      const nid = el.dataset.nodeid
      if (nid === hovId || neighbors.has(nid ?? '')) {
        el.classList.add('hover-highlight')
      } else {
        el.classList.add('hover-dim')
      }
    })

    // Highlight connected edges, dim others
    wrap.querySelectorAll<SVGGElement>('.react-flow__edge').forEach(el => {
      const cls = el.classList
      const isSrc = cls.contains(`rf-src-${hovId}`)
      const isTgt = cls.contains(`rf-tgt-${hovId}`)
      if (isSrc || isTgt) {
        el.classList.add('hover-highlight')
      } else {
        el.classList.add('hover-dim')
      }
    })
  }, [])

  const onNodeMouseLeave = useCallback(() => {
    const wrap = wrapRef.current
    if (!wrap) return
    wrap.classList.remove('is-hovering')
    wrap.querySelectorAll('.hover-highlight, .hover-dim').forEach(el => {
      el.classList.remove('hover-highlight', 'hover-dim')
    })
  }, [])

  function toggleField(name: string) {
    setEnabledFields(prev => {
      const next = new Set(prev)
      if (next.has(name)) next.delete(name); else next.add(name)
      return next
    })
  }

  const allOn = enabledFields.size === availableFields.length
  const noneOn = enabledFields.size === 0
  const hiddenCount = availableFields.length - enabledFields.size

  return (
    <div className="graph-view-wrap" ref={wrapRef}>
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
            <Background color="var(--graph-bg-grid)" gap={24} size={1} />
            <Controls showInteractive={false} />
            <MiniMap
              nodeColor={n => {
                const { graphNode } = n.data as NodeData
                return graphNode.in_focus_file ? '#8a93a3' : '#3a3f48'
              }}
              maskColor="var(--minimap-mask, rgba(14, 16, 20, 0.75))"
              pannable
              zoomable
            />
          </ReactFlow>
        )}
        {availableFields.length > 0 && (
          <div className={`graph-filter-float${filterPanelOpen ? ' open' : ''}`}>
            <button
              className="graph-filter-trigger"
              onClick={() => setFilterPanelOpen(o => !o)}
              title="字段过滤"
            >
              <Icon name="filter" size={13} />
              <span>字段</span>
              {hiddenCount > 0 && <span className="graph-filter-badge">{hiddenCount}</span>}
            </button>
            {filterPanelOpen && (
              <div className="graph-filter-panel">
                <div className="graph-filter-head">
                  <span>字段过滤</span>
                  <button
                    className="btn btn-link"
                    onClick={() => setEnabledFields(allOn ? new Set() : new Set(availableFields))}
                  >
                    {allOn ? '全部隐藏' : noneOn ? '全部显示' : '反选'}
                  </button>
                </div>
                <div className="graph-field-chips">
                  {availableFields.map(f => {
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
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  )
}
