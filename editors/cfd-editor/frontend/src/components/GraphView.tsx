import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, memo } from 'react'
import type { ELK, ElkNode } from 'elkjs/lib/elk-api'
import {
  ReactFlow, Background, Controls, MiniMap,
  Handle, Position, useUpdateNodeInternals, type NodeProps,
  type Node, type Edge,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import type { GraphData } from '../bindings/GraphData'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { RecordRow } from '../bindings/RecordRow'
import type { WriterCapabilities } from '../bindings/WriterCapabilities'
import {
  graphEdgeView,
  graphNodeView,
  type FieldPathSegment,
  type FieldValue,
  type GraphEdgeView,
  type GraphNodeView,
} from '../wire'
import { isEditableCapabilities, isEditableFile } from '../utils/editable'
import { DataCardNode, CardHeader, NODE_PEEK_FIELDS, countVisibleRows } from './DataCard'
import { Icon } from './Icon'
import { typeColor } from '../utils/typeColor'

// ─── Constants (must match CSS / DataCard) ────────────────────────────────── (must match CSS / DataCard) ──────────────────────────────────

const NODE_W     = 280
const COL_GAP    = 280
const ROW_GAP    = 90      // gap between nodes in same column
const COMP_GAP   = 120     // vertical gap between connected components
const COMPACT_ZOOM_THRESHOLD = 0.65
const HEADER_H   = 42
const ROW_H      = 22
const MORE_BTN_H = 28
const PAD_V      = 12
let elkPromise: Promise<ELK> | null = null

async function getElk(): Promise<ELK> {
  if (!elkPromise) {
    elkPromise = import('elkjs/lib/elk.bundled.js').then(({ default: Elk }) => new Elk())
  }
  return elkPromise
}

// ─── Node data ───────────────────────────────────────────────────────────────

interface NodeData extends Record<string, unknown> {
  graphNode: GraphNodeView
  expanded: boolean
  /** Distinct edge field_paths whose source is this node (e.g. ["unlockGeneList[0]", "unlockGeneList[1]"]) */
  outgoingPaths: string[]
  compact: boolean
  /** Stable signature of the per-row expanded set, so CfdNode can re-measure
   *  handle Y positions only when something that affects row geometry changes. */
  rowExpandKey: string
  onToggleExpand: () => void
  onRowToggle: (path: string, exp: boolean) => void
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
  /** Ctrl+click on a node body opens that record in the record view. */
  onCtrlClick?: () => void
  /** Visually mark this node as the current inspector selection. */
  selected?: boolean
}

// ─── CfdNode ─────────────────────────────────────────────────────────────────
// Per-field source handles use useLayoutEffect to measure each row's actual
// DOM offset, so handle Y positions are exact regardless of CSS padding,
// header height variation, or sub-row expansion.

function CfdNode({ id, data }: NodeProps) {
  const { graphNode: gn, expanded, outgoingPaths, compact, rowExpandKey, onToggleExpand, onRowToggle, onEdit, onCtrlClick, selected } = data as NodeData
  const rootRef = useRef<HTMLDivElement>(null)
  const headerRef = useRef<HTMLDivElement>(null)
  const updateNodeInternals = useUpdateNodeInternals()

  // Per-path Y offsets; default to 0 so handles exist on first render
  // (React Flow needs the handle DOM to compute edge endpoints).
  const [pathOffsets, setPathOffsets] = useState<Map<string, number>>(new Map())
  const [headerCenterY, setHeaderCenterY] = useState(21)

  const outgoingKey = outgoingPaths.join('|')
  useLayoutEffect(() => {
    const root = rootRef.current
    if (!root) return
    // Use offsetTop/offsetHeight (CSS pixels relative to offsetParent =
    // .graph-node) instead of getBoundingClientRect, which gives screen
    // pixels distorted by React Flow's viewport zoom transform.
    function offsetWithin(el: HTMLElement, ancestor: HTMLElement): number {
      let y = 0
      let cur: HTMLElement | null = el
      while (cur && cur !== ancestor) {
        y += cur.offsetTop
        cur = cur.offsetParent as HTMLElement | null
      }
      return y
    }
    const headerY = headerRef.current
      ? offsetWithin(headerRef.current, root) + headerRef.current.offsetHeight / 2
      : 21
    const next = new Map<string, number>()
    for (const path of outgoingPaths) {
      let row = root.querySelector<HTMLElement>(
        `.dc-row[data-field-path="${CSS.escape(path)}"]`,
      )
      if (!row) {
        const top = path.match(/^[^.[]+/)?.[0]
        if (top) {
          row = root.querySelector<HTMLElement>(`.dc-row[data-field-name="${CSS.escape(top)}"]`)
        }
      }
      next.set(path, row
        ? offsetWithin(row, root) + row.offsetHeight / 2
        : headerY)
    }
    setHeaderCenterY(prev => prev === headerY ? prev : headerY)
    setPathOffsets(prev => {
      if (prev.size !== next.size) return next
      for (const [k, v] of next) if (prev.get(k) !== v) return next
      return prev
    })
    // Re-measure only when the set of outgoing paths, the node's expand
    // state, or any sub-row expand state changes — not on every render.
  }, [outgoingKey, expanded, compact, rowExpandKey])

  // Tell React Flow to recompute edge paths AFTER our handle Y values land
  // in the DOM (i.e. after the render that uses pathOffsets/headerCenterY).
  useEffect(() => {
    updateNodeInternals(id)
  }, [pathOffsets, headerCenterY, id, updateNodeInternals])

  return (
    <div
      ref={rootRef}
      className={`graph-node${compact ? ' compact' : ''}${gn.in_focus_file ? ' focused' : ' dim'}${selected ? ' selected' : ''}`}
      data-nodeid={gn.id}
      style={{'--node-color': typeColor(gn.actual_type)} as React.CSSProperties}
      onClick={e => {
        // Ctrl+click (or Cmd+click on macOS) opens the record. Plain click
        // is left for React Flow's selection/drag handling.
        if ((e.ctrlKey || e.metaKey) && onCtrlClick) {
          e.preventDefault()
          e.stopPropagation()
          onCtrlClick()
        }
      }}
      title={onCtrlClick ? `${gn.key} — Ctrl+点击打开记录` : gn.key}
    >
      <Handle type="target" position={Position.Left} id="__in" style={{ top: headerCenterY }} />
      {/* Render a handle for EVERY outgoing path on first render (default Y=0)
          so React Flow can resolve the edge sourceHandle on initial mount;
          useLayoutEffect then updates Y values to row centres. */}
      {outgoingPaths.map(path => (
        <Handle
          key={`src-${path}`}
          type="source"
          position={Position.Right}
          id={`path-${path}`}
          style={{ top: pathOffsets.get(path) ?? headerCenterY, bottom: 'auto' }}
        />
      ))}
      <Handle
        type="source"
        position={Position.Right}
        id="__out"
        style={{ top: headerCenterY }}
      />
      {compact ? (
        <div ref={headerRef} className="gn-compact-body">
          <div className="gn-compact-key">{gn.key}</div>
        </div>
      ) : (
        <>
          <div ref={headerRef}>
            <CardHeader recordKey={gn.key} actualType={gn.actual_type} filePath={gn.file_path} />
          </div>
          {gn.is_collapsed ? (
            <div className="gn-collapsed">折叠（超出深度）</div>
          ) : (
            <DataCardNode
              fields={gn.fields}
              actualType={gn.actual_type}
              showAll={expanded}
              onToggle={onToggleExpand}
              onRowToggle={onRowToggle}
              onEdit={onEdit}
            />
          )}
        </>
      )}
    </div>
  )
}

const CfdNodeMemo = memo(CfdNode)
const nodeTypes = { cfd: CfdNodeMemo }

// ─── Height estimation ────────────────────────────────────────────────────────

function estimateNodeHeight(
  gn: GraphNodeView,
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

function isCompactGraphZoom(zoom: number): boolean {
  return zoom < COMPACT_ZOOM_THRESHOLD
}

// ─── Field path → top-level field name ───────────────────────────────────────

function topLevelField(path: string): string {
  const m = path.match(/^[^.[]+/)
  return m ? m[0] : path
}

function defaultEnabledFields(
  graph: { nodes: GraphNodeView[]; edges: GraphEdgeView[] },
  availableFields: string[],
  activeType: string | undefined,
): string[] {
  if (!activeType) return availableFields
  const nodeById = new Map(graph.nodes.map(n => [n.id, n]))
  const fields = new Set<string>()
  for (const e of graph.edges) {
    const source = nodeById.get(e.source)
    const target = nodeById.get(e.target)
    if (source?.actual_type === activeType && target?.actual_type === activeType) {
      fields.add(topLevelField(e.field_path))
    }
  }
  return availableFields.filter(field => fields.has(field))
}

function graphEdgeId(kind: 'fwd' | 'back', edge: { source: string; target: string; field_path: string }): string {
  return `${kind}:${edge.source}->${edge.target}:${encodeURIComponent(edge.field_path)}`
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

// ─── Layout one connected component ──────────────────────────────────────────

function sortedGraphNodes(nodes: GraphNodeView[]): GraphNodeView[] {
  return [...nodes].sort((a, b) => {
    if (a.in_focus_file !== b.in_focus_file) return a.in_focus_file ? -1 : 1
    if (a.file_path !== b.file_path) return a.file_path.localeCompare(b.file_path)
    if (a.key !== b.key) return a.key.localeCompare(b.key)
    return a.id.localeCompare(b.id)
  })
}

function sourcePortId(nodeId: string, fieldPath: string): string {
  return `${nodeId}:out:${encodeURIComponent(fieldPath)}`
}

function targetPortId(nodeId: string): string {
  return `${nodeId}:in`
}

function nodeH(
  gn: GraphNodeView,
  nodeExpandedMap: Map<string, boolean>,
  nodeRowExpandedMap: Map<string, Set<string>>,
) {
  const exp = nodeExpandedMap.get(gn.id) ?? false
  const rows = nodeRowExpandedMap.get(gn.id) ?? new Set<string>()
  return estimateNodeHeight(gn, exp, rows)
}

async function layoutComponent(
  compNodes: GraphNodeView[],
  forwardEdges: GraphEdgeView[],
  forcedRoots: Set<string>,
  nodeExpandedMap: Map<string, boolean>,
  nodeRowExpandedMap: Map<string, Set<string>>,
): Promise<Map<string, { x: number; y: number }>> {
  const outgoingPaths = new Map<string, string[]>()
  for (const edge of forwardEdges) {
    const list = outgoingPaths.get(edge.source) ?? []
    if (!list.includes(edge.field_path)) list.push(edge.field_path)
    outgoingPaths.set(edge.source, list)
  }

  const elkGraph: ElkNode = {
    id: 'root',
    layoutOptions: {
      'elk.algorithm': 'layered',
      'elk.direction': 'RIGHT',
      'elk.spacing.nodeNode': `${ROW_GAP}`,
      'elk.layered.spacing.nodeNodeBetweenLayers': `${COL_GAP}`,
      'elk.layered.crossingMinimization.strategy': 'LAYER_SWEEP',
      'elk.layered.nodePlacement.strategy': 'BRANDES_KOEPF',
      'elk.layered.nodePlacement.bk.fixedAlignment': 'BALANCED',
      'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
      'elk.portConstraints': 'FIXED_ORDER',
      'elk.edgeRouting': 'SPLINES',
    },
    children: sortedGraphNodes(compNodes).map(n => {
      const paths = (outgoingPaths.get(n.id) ?? []).sort((a, b) => {
        const aTop = topLevelField(a)
        const bTop = topLevelField(b)
        const aIdx = n.fields.findIndex(f => f.name === aTop)
        const bIdx = n.fields.findIndex(f => f.name === bTop)
        if (aIdx !== bIdx) return (aIdx === -1 ? Number.MAX_SAFE_INTEGER : aIdx) - (bIdx === -1 ? Number.MAX_SAFE_INTEGER : bIdx)
        return a.localeCompare(b)
      })
      return {
        id: n.id,
        width: NODE_W,
        height: nodeH(n, nodeExpandedMap, nodeRowExpandedMap),
        layoutOptions: {
          ...(forcedRoots.has(n.id) ? { 'elk.layered.layering.layerConstraint': 'FIRST' } : {}),
          'elk.portConstraints': 'FIXED_ORDER',
        },
        ports: [
          {
            id: targetPortId(n.id),
            width: 1,
            height: 1,
            layoutOptions: {
              'elk.port.side': 'WEST',
              'elk.port.index': '0',
            },
          },
          ...paths.map((path, i) => ({
            id: sourcePortId(n.id, path),
            width: 1,
            height: 1,
            layoutOptions: {
              'elk.port.side': 'EAST',
              'elk.port.index': `${i + 1}`,
            },
          })),
        ],
      }
    }),
    edges: forwardEdges.map(e => ({
      id: graphEdgeId('fwd', e),
      sources: [sourcePortId(e.source, e.field_path)],
      targets: [targetPortId(e.target)],
    })),
  }

  const elk = await getElk()
  const laidOut = await elk.layout(elkGraph)
  const minX = Math.min(...(laidOut.children ?? []).map(n => n.x ?? 0))
  const positions = new Map<string, { x: number; y: number }>()
  for (const n of laidOut.children ?? []) {
    positions.set(n.id, { x: (n.x ?? 0) - minX, y: n.y ?? 0 })
  }
  return positions
}

// ─── Full layout (all components, stacked vertically) ────────────────────────

interface LayoutResult {
  positions: Map<string, { x: number; y: number }>
  visibleNodes: GraphNodeView[]
  forwardEdges: GraphEdgeView[]
  backEdges: GraphEdgeView[]
}

async function layoutAll(
  graph: { nodes: GraphNodeView[]; edges: GraphEdgeView[] },
  enabledFields: Set<string>,
  activeType: string | undefined,
  nodeExpandedMap: Map<string, boolean>,
  nodeRowExpandedMap: Map<string, Set<string>>,
): Promise<LayoutResult> {
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

  // Detect back-edges (cycles)
  const backEdgeKeys = detectBackEdges(visibleNodes, activeEdges)
  const forwardEdges = activeEdges.filter(e => !backEdgeKeys.has(`${e.source}→${e.target}`))
  const backEdges    = activeEdges.filter(e =>  backEdgeKeys.has(`${e.source}→${e.target}`))
  const nodeById = new Map(visibleNodes.map(n => [n.id, n]))

  const forcedRoots = new Set<string>()
  if (activeType) {
    const sameTableTargets = new Set<string>()
    for (const e of forwardEdges) {
      const source = nodeById.get(e.source)
      const target = nodeById.get(e.target)
      if (
        source?.in_focus_file &&
        target?.in_focus_file &&
        source.actual_type === activeType &&
        target.actual_type === activeType
      ) {
        sameTableTargets.add(target.id)
      }
    }
    for (const n of visibleNodes) {
      if (n.in_focus_file && n.actual_type === activeType && !sameTableTargets.has(n.id)) {
        forcedRoots.add(n.id)
      }
    }
  }

  // Split into connected components using ALL edges (forward + back)
  const comps = connectedComponents(visibleNodes.map(n => n.id), activeEdges)
  const nodeToComp = new Map<string, number>()
  comps.forEach((comp, ci) => comp.forEach(id => nodeToComp.set(id, ci)))

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
    const localPos = await layoutComponent(compNodes, compForward, compForcedRoots, nodeExpandedMap, nodeRowExpandedMap)

    // Find y-extent of this component's layout
    let minY = Infinity, maxY = -Infinity
    for (const [id, { y }] of localPos) {
      const node = nodeById.get(id)
      const h = node ? nodeH(node, nodeExpandedMap, nodeRowExpandedMap) : 0
      if (y < minY) minY = y
      if (y + h > maxY) maxY = y + h
    }
    // Shift so component starts at yOffset (minY maps to yOffset)
    const shift = yOffset - minY
    for (const [id, pos] of localPos) {
      allPositions.set(id, { x: pos.x, y: pos.y + shift })
    }
    const compHeight = maxY - minY
    yOffset += compHeight + COMP_GAP
  }

  return { positions: allPositions, visibleNodes, forwardEdges, backEdges }
}

// ─── Edge handle id (outside component, stable reference) ────────────────────

function edgeHandleId(
  _sourceId: string,
  fieldPath: string,
): { sourceHandle: string; targetHandle: string } {
  return { sourceHandle: `path-${fieldPath}`, targetHandle: '__in' }
}

// ─── Component ───────────────────────────────────────────────────────────────

interface Props {
  graphData: GraphData
  activeType?: string
  fileCapabilities?: Record<string, WriterCapabilities>
  onEnabledFieldsChange?: (fields: string[]) => void
  onOpenRecord: (file: string, coordinate: RecordCoordinate) => void
  /** Plain click on a node: open the side inspector for that record. */
  onSelectRecord?: (file: string, coordinate: RecordCoordinate) => void
  /** Click on empty pane: deselect / close inspector. */
  onClearSelection?: () => void
  /** Currently selected coordinate (used to highlight the node). */
  selectedCoordinate?: { file: string; coordinate: RecordCoordinate } | null
  onWriteField?: (
    filePath: string, coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], newValue: FieldValue
  ) => Promise<RecordRow | void>
}

export function GraphView({ graphData, activeType, fileCapabilities, onEnabledFieldsChange, onOpenRecord, onSelectRecord, onClearSelection, selectedCoordinate, onWriteField }: Props) {
  const [compactNodes, setCompactNodes] = useState(false)
  const graph = useMemo(
    () => ({
      nodes: graphData.nodes.map(graphNodeView),
      edges: graphData.edges.map(graphEdgeView),
    }),
    [graphData],
  )

  const availableFields = useMemo(
    () => graphData.available_fields.slice().sort(),
    [graphData.available_fields],
  )

  const [enabledFields, setEnabledFields] = useState<Set<string>>(
    () => new Set(defaultEnabledFields(graph, availableFields, activeType)),
  )
  useEffect(() => {
    setEnabledFields(prev => {
      const next = new Set<string>()
      if (prev.size === 0) {
        for (const f of defaultEnabledFields(graph, availableFields, activeType)) next.add(f)
      } else {
        for (const f of availableFields) if (prev.has(f)) next.add(f)
      }
      return next
    })
  }, [availableFields, graph, activeType])
  useEffect(() => {
    if (availableFields.length === 0) return
    onEnabledFieldsChange?.(Array.from(enabledFields).sort())
  }, [availableFields.length, enabledFields, onEnabledFieldsChange])

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

  const [layout, setLayout] = useState<LayoutResult>({
    positions: new Map(),
    visibleNodes: [],
    forwardEdges: [],
    backEdges: [],
  })

  useEffect(() => {
    let cancelled = false
    layoutAll(graph, enabledFields, activeType, nodeExpandedMap, nodeRowExpandedMap)
      .then(next => {
        if (!cancelled) setLayout(next)
      })
      .catch(err => {
        console.error('Failed to layout graph', err)
        if (!cancelled) {
          setLayout({ positions: new Map(), visibleNodes: [], forwardEdges: [], backEdges: [] })
        }
      })
    return () => { cancelled = true }
  }, [graph, enabledFields, activeType, nodeExpandedMap, nodeRowExpandedMap])

  const { positions, visibleNodes, forwardEdges, backEdges } = layout

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

  const rfNodes: Node[] = useMemo(
    () => (
      visibleNodes.map(n => {
        const capability = fileCapabilities?.[n.file_path]
        const editable = !!onWriteField && (capability ? isEditableCapabilities(capability) : isEditableFile(n.file_path))
        const rowExpanded = nodeRowExpandedMap.get(n.id)
        return {
          id: n.id,
          type: 'cfd',
          position: positions.get(n.id) ?? { x: 0, y: 0 },
          data: {
            graphNode: n,
            expanded: nodeExpandedMap.get(n.id) ?? false,
            outgoingPaths: outgoingPathsByNode.get(n.id) ?? [],
            compact: compactNodes,
            rowExpandKey: rowExpanded ? Array.from(rowExpanded).sort().join('|') : '',
            onToggleExpand: () => toggleNodeExpanded(n.id),
            onRowToggle: (path: string, exp: boolean) => handleRowToggle(n.id, path, exp),
            onEdit: editable
              ? (path: FieldPathSegment[], val: FieldValue) => { onWriteField!(n.file_path, n.coordinate, path, val) }
              : undefined,
            onCtrlClick: onOpenRecord ? () => onOpenRecord(n.file_path, n.coordinate) : undefined,
            selected: !!selectedCoordinate
              && selectedCoordinate.file === n.file_path
              && selectedCoordinate.coordinate.actual_type === n.coordinate.actual_type
              && selectedCoordinate.coordinate.key === n.coordinate.key,
          } satisfies NodeData,
        }
      })
    ),
    [visibleNodes, positions, nodeExpandedMap, nodeRowExpandedMap, outgoingPathsByNode, compactNodes, toggleNodeExpanded, handleRowToggle, onWriteField, onOpenRecord, fileCapabilities, selectedCoordinate]
  )

  const rfEdges: Edge[] = useMemo(() => {
    const fwdEdges: Edge[] = forwardEdges
      .filter(e => positions.has(e.source) && positions.has(e.target))
      .map(e => {
        const { sourceHandle, targetHandle } = edgeHandleId(e.source, e.field_path)
        return {
          id: graphEdgeId('fwd', e),
          source: e.source,
          target: e.target,
          sourceHandle,
          targetHandle,
          label: compactNodes ? undefined : e.field_path,
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
      .map(e => ({
        id: graphEdgeId('back', e),
        source: e.source,
        target: e.target,
        sourceHandle: `path-${e.field_path}`,
        targetHandle: '__in',
        label: compactNodes ? undefined : e.field_path,
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
  }, [forwardEdges, backEdges, positions, compactNodes])

  // ── Imperative hover highlight (zero re-renders) ────────────────────────
  // We manipulate DOM classes directly to avoid the state→rerender→mouseleave
  // flicker cycle. The adjacency map is rebuilt whenever edges change.
  const wrapRef = useRef<HTMLDivElement>(null)

  // nodeId → set of nodeIds it is directly connected to
  const adjacencyRef = useRef<Map<string, Set<string>>>(new Map())
  useEffect(() => {
    const adj = new Map<string, Set<string>>()
    for (const e of [...forwardEdges, ...backEdges]) {
      if (!adj.has(e.source)) adj.set(e.source, new Set())
      if (!adj.has(e.target)) adj.set(e.target, new Set())
      adj.get(e.source)!.add(e.target)
      adj.get(e.target)!.add(e.source)
    }
    adjacencyRef.current = adj
  }, [forwardEdges, backEdges])

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
  const handleViewportChange = useCallback((viewport: { zoom: number }) => {
    const next = isCompactGraphZoom(viewport.zoom)
    setCompactNodes(prev => prev === next ? prev : next)
  }, [])

  return (
    <div className="graph-view-wrap" ref={wrapRef}>
      <div className="graph-view">
        {rfNodes.length === 0 ? (
          <div className="empty-hint">无可显示的引用关系</div>
        ) : (
          <>
            <ReactFlow
            nodes={rfNodes}
            edges={rfEdges}
            nodeTypes={nodeTypes}
            onNodeMouseEnter={onNodeMouseEnter}
            onNodeMouseLeave={onNodeMouseLeave}
            onNodeClick={(e, node) => {
              // Ctrl/Cmd+click jumps to the record view (handled in CfdNode).
              // Plain click opens the side inspector.
              if (e.ctrlKey || e.metaKey) return
              if (!onSelectRecord) return
              const gn = (node.data as NodeData).graphNode
              onSelectRecord(gn.file_path, gn.coordinate)
            }}
            onPaneClick={() => onClearSelection?.()}
            onViewportChange={handleViewportChange}
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
          <div className="graph-hint" title="点击节点打开侧边面板，Ctrl+点击跳转到记录视图">
            点击节点查看 · Ctrl+点击跳转
          </div>
          </>
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
