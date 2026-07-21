import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, memo } from 'react'
import {
  ReactFlow, Background, Controls, MiniMap,
  Handle, Position, useUpdateNodeInternals, type NodeProps,
  type Node, type Edge, type ReactFlowInstance,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import type { GraphData } from '../bindings/GraphData'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { CollectionEdit } from '../bindings/CollectionEdit'
import type { RecordRow } from '../bindings/RecordRow'
import type { WriterCapabilities } from '../bindings/WriterCapabilities'
import {
  graphEdgeView,
  graphNodeView,
  type DiagnosticItem,
  type FieldPathSegment,
  type FieldValue,
  type GraphNodeView,
} from '../wire'
import { isEditableCapabilities, isEditableFile } from '../utils/editable'
import { DataCardNode, CardHeader } from './DataCard'
import { DiagBadge } from './DiagBadge'
import { Icon } from './Icon'
import { typeColor } from '../utils/typeColor'
import {
  defaultEnabledFields,
  estimateHandleOffsets,
  graphEdgeId,
  graphTopologySignature,
  isCompactGraphZoom,
  layoutGraph,
  sameOffsetMap,
  type GraphLayoutResult,
} from './GraphView.layout'
import { runGraphLayoutInWorker } from './GraphLayoutWorkerAdapter'
import {
  buildRecordDiagnosticIndex,
  diagnosticsForRecord,
} from '../state/recordDiagnostics'

const MEASURE_HANDLE_NODE_LIMIT = 80

// ─── Node data ───────────────────────────────────────────────────────────────

interface NodeData extends Record<string, unknown> {
  graphNode: GraphNodeView
  expanded: boolean
  /** Distinct edge field_paths whose source is this node (e.g. ["unlockGeneList[0]", "unlockGeneList[1]"]) */
  outgoingPaths: string[]
  compact: boolean
  measureHandles: boolean
  /** Stable signature of the per-row expanded set, so CfdNode can re-measure
   *  handle Y positions only when something that affects row geometry changes. */
  rowExpandKey: string
  expandedPaths: ReadonlySet<string>
  onToggleExpand: () => void
  onRowToggle: (path: string, exp: boolean) => void
  onEdit?: (fieldPath: FieldPathSegment[], newValue: FieldValue) => void
  onCollectionEdit?: (fieldPath: FieldPathSegment[], edit: CollectionEdit) => void
  /** Ctrl+click on a node body opens that record in the record view. */
  onCtrlClick?: () => void
  /** Visually mark this node as the current inspector selection. */
  selected?: boolean
  /** Record-level severity, drives the corner badge on the graph node. */
  diagSeverity?: 'error' | 'warning' | null
  onDiagBadgeClick?: () => void
}

// ─── CfdNode ─────────────────────────────────────────────────────────────────
// Per-field source handles measure exact DOM offsets only for small, expanded
// card graphs. Compact and large graphs use deterministic estimates so zoom
// threshold changes don't query every rendered node.

function CfdNode({ id, data }: NodeProps) {
  const { graphNode: gn, expanded, outgoingPaths, compact, measureHandles, rowExpandKey, expandedPaths, onToggleExpand, onRowToggle, onEdit, onCollectionEdit, onCtrlClick, selected, diagSeverity, onDiagBadgeClick } = data as NodeData
  const rootRef = useRef<HTMLDivElement>(null)
  const headerRef = useRef<HTMLDivElement>(null)
  const updateNodeInternals = useUpdateNodeInternals()

  const outgoingKey = outgoingPaths.join('|')
  const estimatedHandles = useMemo(
    () => estimateHandleOffsets(gn, outgoingPaths, expanded, compact),
    [gn, outgoingKey, expanded, compact],
  )
  // Per-path Y offsets; estimates exist on first render so React Flow can
  // resolve handles before optional DOM measurement lands.
  const [pathOffsets, setPathOffsets] = useState<Map<string, number>>(() => estimatedHandles.pathOffsets)
  const [headerCenterY, setHeaderCenterY] = useState(() => estimatedHandles.headerCenterY)

  useLayoutEffect(() => {
    if (measureHandles) return
    setHeaderCenterY(prev => prev === estimatedHandles.headerCenterY ? prev : estimatedHandles.headerCenterY)
    setPathOffsets(prev => sameOffsetMap(prev, estimatedHandles.pathOffsets) ? prev : estimatedHandles.pathOffsets)
  }, [measureHandles, estimatedHandles])

  useLayoutEffect(() => {
    if (!measureHandles) return
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
  }, [measureHandles, outgoingKey, expanded, rowExpandKey])

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
          {(diagSeverity === 'error' || diagSeverity === 'warning') && (
            <DiagBadge severity={diagSeverity} onClick={onDiagBadgeClick} />
          )}
        </div>
      ) : (
        <>
          <div ref={headerRef}>
            <CardHeader
              recordKey={gn.key}
              actualType={gn.actual_type}
              filePath={gn.file_path}
              diagSeverity={diagSeverity}
              onDiagBadgeClick={onDiagBadgeClick}
            />
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
              expandedPaths={expandedPaths}
              onEdit={onEdit}
              onCollectionEdit={onCollectionEdit}
            />
          )}
        </>
      )}
    </div>
  )
}

const CfdNodeMemo = memo(CfdNode)
const nodeTypes = { cfd: CfdNodeMemo }

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
  enabledFieldsOverride?: readonly string[]
  onEnabledFieldsChange?: (fields: string[]) => void
  /** Custom graph view: restrict node card fields to this set (undefined = all). */
  visibleCardFields?: ReadonlySet<string>
  fileCapabilities?: Record<string, WriterCapabilities>
  /** Full diagnostics list (not pre-filtered by file) — nodes in the graph
   *  can point at records that live outside the focus file. */
  diagnostics?: DiagnosticItem[]
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
  onCollectionEdit?: (
    filePath: string, coordinate: RecordCoordinate, fieldPath: FieldPathSegment[], edit: CollectionEdit
  ) => Promise<RecordRow | void>
  onDiagnosticBadgeClick?: (
    file: string, coordinate: RecordCoordinate, fieldPath: string | null,
  ) => void
  onExitLeft?: () => void
  onExitUp?: () => void
  onExitRight?: () => void
  firstRecordFocusRequest?: number
  onFirstRecordFocusConsumed?: (request: number) => void
}

export function GraphView({ graphData, activeType, enabledFieldsOverride, onEnabledFieldsChange, visibleCardFields, fileCapabilities, diagnostics, onOpenRecord, onSelectRecord, onClearSelection, selectedCoordinate, onWriteField, onCollectionEdit, onDiagnosticBadgeClick, onExitLeft, onExitUp, onExitRight, firstRecordFocusRequest, onFirstRecordFocusConsumed }: Props) {
  const [zoomCompactNodes, setZoomCompactNodes] = useState(false)
  const graph = useMemo(
    () => ({
      nodes: graphData.nodes.map(node => {
        const view = graphNodeView(node)
        if (!visibleCardFields) return view
        // Custom graph view: show only the selected card fields.
        return { ...view, fields: view.fields.filter(cell => visibleCardFields.has(cell.name)) }
      }),
      edges: graphData.edges.map(graphEdgeView),
    }),
    [graphData, visibleCardFields],
  )
  const topologySignature = useMemo(() => graphTopologySignature(graph), [graph])

  const availableFields = useMemo(
    () => graphData.available_fields.slice().sort(),
    [graphData.available_fields],
  )

  const defaultFields = useMemo(
    () => defaultEnabledFields(graph, availableFields, activeType),
    [topologySignature, availableFields, activeType],
  )

  const enabledFields = useMemo(
    () => enabledFieldsOverride === undefined
      ? new Set(defaultFields)
      : new Set(enabledFieldsOverride.filter(field => availableFields.includes(field))),
    [enabledFieldsOverride, defaultFields, availableFields],
  )

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

  const [layout, setLayout] = useState<GraphLayoutResult>({
    positions: new Map(),
    visibleNodes: [],
    forwardEdges: [],
    backEdges: [],
  })
  const [layoutBusy, setLayoutBusy] = useState(false)
  const [layoutError, setLayoutError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setLayoutBusy(graph.nodes.length > 0 && enabledFields.size > 0)
    setLayoutError(null)
    layoutGraph(
      graph,
      enabledFields,
      activeType,
      nodeExpandedMap,
      nodeRowExpandedMap,
      runGraphLayoutInWorker,
    )
      .then(next => {
        if (!cancelled) {
          setLayout(next)
          setLayoutBusy(false)
        }
      })
      .catch(err => {
        console.error('Failed to layout graph', err)
        if (!cancelled) {
          setLayout({ positions: new Map(), visibleNodes: [], forwardEdges: [], backEdges: [] })
          setLayoutBusy(false)
          setLayoutError(err instanceof Error ? err.message : String(err))
        }
      })
    return () => {
      cancelled = true
    }
  }, [topologySignature, enabledFields, activeType, nodeExpandedMap, nodeRowExpandedMap])

  const { positions, forwardEdges, backEdges } = layout
  const currentNodeById = useMemo(
    () => new Map(graph.nodes.map(node => [node.id, node])),
    [graph.nodes],
  )
  const visibleNodes = useMemo(
    () => layout.visibleNodes.map(node => currentNodeById.get(node.id) ?? node),
    [layout.visibleNodes, currentNodeById],
  )
  const diagnosticIndex = useMemo(
    () => buildRecordDiagnosticIndex(
      graph.nodes.map(node => ({ filePath: node.file_path, coordinate: node.coordinate })),
      diagnostics,
    ),
    [graph.nodes, diagnostics],
  )
  const compactNodes = zoomCompactNodes
  const measureHandles = !compactNodes && visibleNodes.length <= MEASURE_HANDLE_NODE_LIMIT

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
        const nodeSev = diagnosticsForRecord(
          diagnosticIndex,
          { filePath: n.file_path, coordinate: n.coordinate },
          {
            fieldDiagnostics: n.field_diagnostics,
            severity: n.diagnostic_severity === 'error' || n.diagnostic_severity === 'warning'
              ? n.diagnostic_severity
              : null,
          },
        ).severity
        return {
          id: n.id,
          type: 'cfd',
          position: positions.get(n.id) ?? { x: 0, y: 0 },
          data: {
            graphNode: n,
            expanded: nodeExpandedMap.get(n.id) ?? false,
            outgoingPaths: outgoingPathsByNode.get(n.id) ?? [],
            compact: compactNodes,
            measureHandles,
            rowExpandKey: rowExpanded ? Array.from(rowExpanded).sort().join('|') : '',
            expandedPaths: rowExpanded ?? new Set<string>(),
            onToggleExpand: () => toggleNodeExpanded(n.id),
            onRowToggle: (path: string, exp: boolean) => handleRowToggle(n.id, path, exp),
            onEdit: editable
              ? (path: FieldPathSegment[], val: FieldValue) => { onWriteField!(n.file_path, n.coordinate, path, val) }
              : undefined,
            onCollectionEdit: editable && onCollectionEdit
              ? (path: FieldPathSegment[], edit: CollectionEdit) => { onCollectionEdit(n.file_path, n.coordinate, path, edit) }
              : undefined,
            onCtrlClick: onOpenRecord ? () => onOpenRecord(n.file_path, n.coordinate) : undefined,
            selected: !!selectedCoordinate
              && selectedCoordinate.file === n.file_path
              && selectedCoordinate.coordinate.actual_type === n.coordinate.actual_type
              && selectedCoordinate.coordinate.key === n.coordinate.key,
            diagSeverity: nodeSev,
            onDiagBadgeClick: onDiagnosticBadgeClick
              ? () => onDiagnosticBadgeClick(n.file_path, n.coordinate, null)
              : undefined,
          } satisfies NodeData,
        }
      })
    ),
    [visibleNodes, positions, nodeExpandedMap, nodeRowExpandedMap, outgoingPathsByNode, compactNodes, measureHandles, toggleNodeExpanded, handleRowToggle, onWriteField, onOpenRecord, fileCapabilities, selectedCoordinate, diagnosticIndex, onDiagnosticBadgeClick]
  )
  const reactFlowRef = useRef<ReactFlowInstance<Node, Edge> | null>(null)

  useEffect(() => {
    if (rfNodes.length === 0) return
    let fitFrame = 0
    const measureFrame = requestAnimationFrame(() => {
      fitFrame = requestAnimationFrame(() => {
        void reactFlowRef.current?.fitView({ padding: 0.25, minZoom: 0.2, maxZoom: 1.2 })
      })
    })
    return () => {
      cancelAnimationFrame(measureFrame)
      cancelAnimationFrame(fitFrame)
    }
  }, [positions, rfNodes.length])

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

  useEffect(() => {
    if (!firstRecordFocusRequest) return
    const first = visibleNodes[0]
    if (first) onSelectRecord?.(first.file_path, first.coordinate)
    requestAnimationFrame(() => {
      const target = wrapRef.current?.querySelector<HTMLElement>('.react-flow__node')
        ?? wrapRef.current
      target?.focus({ preventScroll: true })
    })
    onFirstRecordFocusConsumed?.(firstRecordFocusRequest)
  }, [firstRecordFocusRequest])

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
    const next = new Set(enabledFields)
    if (next.has(name)) next.delete(name); else next.add(name)
    onEnabledFieldsChange?.(Array.from(next).sort())
  }

  const allOn = enabledFields.size === availableFields.length
  const noneOn = enabledFields.size === 0
  const hiddenCount = availableFields.length - enabledFields.size
  const handleViewportChange = useCallback((viewport: { zoom: number }) => {
    const next = isCompactGraphZoom(viewport.zoom)
    setZoomCompactNodes(prev => prev === next ? prev : next)
  }, [])

  return (
    <div
      className="graph-view-wrap"
      ref={wrapRef}
      tabIndex={0}
      onKeyDown={event => {
        if (event.target !== event.currentTarget) return
        if (event.key === 'ArrowLeft') {
          event.preventDefault()
          onExitLeft?.()
        } else if (event.key === 'ArrowUp') {
          event.preventDefault()
          onExitUp?.()
        } else if (event.key === 'ArrowRight') {
          event.preventDefault()
          onExitRight?.()
        } else if (event.key === 'Enter') {
          const first = event.currentTarget.querySelector<HTMLElement>('.react-flow__node')
          if (first) {
            event.preventDefault()
            first.focus({ preventScroll: true })
          }
        }
      }}
    >
      <div className="graph-view">
        {rfNodes.length === 0 ? (
          <div className="empty-hint">
            {layoutBusy
              ? '布局图谱中…'
              : layoutError
                ? '图谱布局失败'
                : availableFields.length > 0 && enabledFields.size === 0
                  ? '未选择引用字段'
                  : '无可显示的引用关系'}
          </div>
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
            onInit={instance => {
              reactFlowRef.current = instance
              requestAnimationFrame(() => {
                requestAnimationFrame(() => {
                  void instance.fitView({ padding: 0.25, minZoom: 0.2, maxZoom: 1.2 })
                })
              })
            }}
            proOptions={{ hideAttribution: true }}
            minZoom={0.1}
            maxZoom={2}
          >
            <Background color="var(--graph-bg-grid)" gap={24} size={1} />
            <Controls showInteractive={false} />
            <MiniMap
              style={{ width: 88, height: 60 }}
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
                    onClick={() => onEnabledFieldsChange?.(allOn ? [] : [...availableFields])}
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
