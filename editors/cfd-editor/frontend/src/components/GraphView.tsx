import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, memo, type MouseEvent as ReactMouseEvent, type PointerEvent as ReactPointerEvent } from 'react'
import {
  ReactFlow, Background, Controls, MiniMap, ConnectionLineType, SelectionMode,
  Handle, Position, useUpdateNodeInternals, type NodeProps,
  BaseEdge, type EdgeProps,
  ControlButton, getViewportForBounds, ViewportPortal,
  type Node, type Edge, type ReactFlowInstance,
  type Connection, applyNodeChanges, type NodeChange,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import type { GraphData } from '../bindings/GraphData'
import { useReferenceShortName } from './ShortNameContext'
import { shortNameLabel } from '../state/shortNames'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import type { CollectionEdit } from '../bindings/CollectionEdit'
import type { RecordRow } from '../bindings/RecordRow'
import type { WriterCapabilities } from '../bindings/WriterCapabilities'
import {
  graphEdgeView,
  coordinateId,
  type DiagnosticItem,
  type FieldPathSegment,
  type FieldValue,
  type GraphNodeView,
} from '../wire'
import { isEditableCapabilities, isEditableFile } from '../utils/editable'
import { DataCardNode, CardHeader } from './DataCard'
import { DiagBadge } from './DiagBadge'
import { typeColor } from '../utils/typeColor'
import {
  defaultEnabledFields,
  estimateNodeHeight,
  estimateHandleOffsets,
  graphEdgeId,
  graphTopologySignature,
  isCompactGraphZoom,
  layoutGraph,
  type GraphLayoutResult,
} from './GraphView.layout'
import { runGraphLayoutInWorker } from './GraphLayoutWorkerAdapter'
import { graphCardFields, relationPorts, relationValue, type RelationPort } from './GraphView.relations'
import { useEditorLookups } from '../utils/editContext'
import { SearchableSelect } from './SearchableSelect'
import { Icon } from './Icon'
import type { GraphPositions } from '../state/editorState'
import {
  graphHighlights, polylineIntersectsRect, reconcileFlowNodes, reconcileGraphViews,
  sameGraphValue, selectedNodeBounds, type GraphFocus, type GraphRect,
} from './GraphView.state'
import { CreateRecordDialog } from './CreateRecordDialog'
import { RecordContextMenu } from './RecordContextMenu'
import { ConfirmDialog, TextInputDialog } from './ActionDialog'
import type { CreateRecordDraft } from '../bindings/CreateRecordDraft'
import type { EditorRecordGroup } from '../bindings/EditorRecordGroup'
import type { RefTarget } from '../bindings/RefTarget'
import {
  buildRecordDiagnosticIndex,
  diagnosticsForRecord,
} from '../state/recordDiagnostics'


// ─── Node data ───────────────────────────────────────────────────────────────

interface NodeData extends Record<string, unknown> {
  graphNode: GraphNodeView
  expanded: boolean
  /** Distinct edge field_paths whose source is this node (e.g. ["unlockGeneList[0]", "unlockGeneList[1]"]) */
  outgoingPaths: string[]
  ports: RelationPort[]
  compact: boolean
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
// 关系端口按真实行位置测量，集合展开与异步内容变化后仍准确落在对应元素上。

function CfdNode(props: NodeProps) {
  const { graphNode } = props.data as NodeData
  const shortName = useReferenceShortName(graphNode.actual_type, graphNode.key)
  return <CfdNodeContent id={props.id} data={props.data} shortName={shortName} />
}

// 查询 generation 变化只重新解析名称；名称未变时不重新执行卡片字段树。
const CfdNodeContent = memo(function CfdNodeContent({ id, data, shortName }: Pick<NodeProps, 'id' | 'data'> & { shortName?: string }) {
  const { graphNode: gn, expanded, outgoingPaths, ports, compact, rowExpandKey, expandedPaths, onToggleExpand, onRowToggle, onEdit, onCollectionEdit, onCtrlClick, selected, diagSeverity, onDiagBadgeClick } = data as NodeData
  const rootRef = useRef<HTMLDivElement>(null)
  const headerRef = useRef<HTMLDivElement>(null)
  const updateNodeInternals = useUpdateNodeInternals()
  const graphRelations = useMemo(() => ports.length > 0 ? {
    expandedPaths: relationPorts(gn.fields).expanded,
    appendPaths: new Set(ports.filter(port => port.append).map(port => JSON.stringify(port.path))),
  } : undefined, [gn.fields, ports])

  const outgoingKey = outgoingPaths.join('|')
  const estimatedHandles = useMemo(
    () => estimateHandleOffsets(gn, outgoingPaths, expanded),
    [gn, outgoingKey, expanded],
  )
  // Per-path Y offsets; estimates exist on first render so React Flow can
  // resolve handles before optional DOM measurement lands.
  const [pathOffsets, setPathOffsets] = useState<Map<string, number>>(() => estimatedHandles.pathOffsets)
  const [headerCenterY, setHeaderCenterY] = useState(() => estimatedHandles.headerCenterY)

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
    const measure = () => {
      const headerY = headerRef.current
        ? offsetWithin(headerRef.current, root) + headerRef.current.offsetHeight / 2
        : 21
      const next = new Map<string, number>()
      for (const path of outgoingPaths) {
        const port = ports.find(port => port.id === path)
        let row = root.querySelector<HTMLElement>(
          port?.append
            ? `[data-add-path-wire="${CSS.escape(JSON.stringify(port.path))}"]`
            : port ? `.dc-row[data-field-path-wire="${CSS.escape(JSON.stringify(port.path))}"]`
              : `.dc-row[data-field-path="${CSS.escape(path)}"]`,
        )
        if (!row) {
          const top = path.match(/^[^.[]+/)?.[0]
          if (top) row = root.querySelector<HTMLElement>(`.dc-row[data-field-name="${CSS.escape(top)}"]`)
        }
        next.set(path, row ? offsetWithin(row, root) + row.offsetHeight / 2 : headerY)
      }
      setHeaderCenterY(prev => prev === headerY ? prev : headerY)
      setPathOffsets(prev => {
        if (prev.size !== next.size) return next
        for (const [k, v] of next) if (prev.get(k) !== v) return next
        return prev
      })
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(root)
    return () => observer.disconnect()
  }, [outgoingKey, expanded, rowExpandKey, gn.fields, ports])

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
      <Handle type="target" position={Position.Left} id="__in" isConnectableStart={false} style={{ top: headerCenterY }} />
      {/* Render a handle for EVERY outgoing path on first render (default Y=0)
          so React Flow can resolve the edge sourceHandle on initial mount;
          useLayoutEffect then updates Y values to row centres. */}
      {outgoingPaths.map(path => (
        <Handle
          key={`src-${path}`}
          type="source"
          position={Position.Right}
          id={`path-${path}`}
          isConnectable={!!onEdit && !!ports.find(port => port.id === path && !port.readOnly && (!port.append || !!onCollectionEdit))}
          title={ports.find(port => port.id === path)?.append ? '追加引用' : path}
          style={{ top: pathOffsets.get(path) ?? headerCenterY, bottom: 'auto' }}
        />
      ))}
      {compact && (
        <div className="gn-compact-body">
          <div className="gn-compact-key">{shortName ?? gn.key}</div>
          {(diagSeverity === 'error' || diagSeverity === 'warning') && (
            <DiagBadge severity={diagSeverity} onClick={onDiagBadgeClick} />
          )}
        </div>
      )}
      {/* 缩略只替换视觉内容，完整卡片仍占据原来的空间，端口坐标不随缩放变化。 */}
      <div className="gn-detail" style={compact ? { visibility: 'hidden', pointerEvents: 'none' } : undefined} inert={compact}>
          <div ref={headerRef}>
            <CardHeader
              recordKey={gn.key}
              shortName={shortName}
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
              graphRelations={graphRelations}
            />
          )}
      </div>
    </div>
  )
})

// 卡片不读取坐标；移动节点只更新外层变换，不重新执行字段树和端口测量。
const CfdNodeMemo = memo(CfdNode, (previous, next) => previous.id === next.id && previous.data === next.data)
const nodeTypes = { cfd: CfdNodeMemo }

// Shader Graph 风格：端口先水平伸出一小段，再用一段平滑曲线连接，避免端口附近出现直角重叠。
function ShaderEdge({ id, sourceX, sourceY, targetX, targetY, style, markerEnd }: EdgeProps) {
  const stub = 28
  const direction = targetX >= sourceX ? 1 : -1
  const sx = sourceX + direction * stub
  const tx = targetX - direction * stub
  const distance = Math.max(24, Math.abs(tx - sx) * 0.45)
  const path = `M ${sourceX} ${sourceY} L ${sx} ${sourceY} C ${sx + direction * distance} ${sourceY}, ${tx - direction * distance} ${targetY}, ${tx} ${targetY} L ${targetX} ${targetY}`
  // 线体拖动：找到离按下点较近的重连锚点，把原生事件转发给它，复用 React Flow 的重连流程。
  const beginBodyDrag = (event: ReactMouseEvent<SVGPathElement>) => {
    if (event.button !== 0) return
    const group = event.currentTarget.closest('.react-flow__edge')
    if (!group) return
    const anchors = Array.from(group.querySelectorAll<SVGCircleElement>('.react-flow__edgeupdater'))
    let nearest: SVGCircleElement | null = null
    let nearestDistance = Infinity
    for (const anchor of anchors) {
      const rect = anchor.getBoundingClientRect()
      const dx = rect.left + rect.width / 2 - event.clientX
      const dy = rect.top + rect.height / 2 - event.clientY
      const squared = dx * dx + dy * dy
      if (squared < nearestDistance) { nearestDistance = squared; nearest = anchor }
    }
    if (!nearest) return
    event.preventDefault()
    event.stopPropagation()
    nearest.dispatchEvent(new MouseEvent('mousedown', {
      bubbles: true, cancelable: true, view: window,
      clientX: event.clientX, clientY: event.clientY, button: 0,
    }))
  }
  return (
    <>
      <BaseEdge path={path} style={style} markerEnd={markerEnd} id={id} />
      <path
        d={path}
        className="graph-edge-drag"
        fill="none"
        stroke="transparent"
        strokeWidth={22}
        onMouseDown={beginBodyDrag}
      />
    </>
  )
}
const edgeTypes = { shader: ShaderEdge }

// 拖线到空白处的下拉列表固定首项：先创建记录，再建立引用。
const NEW_TARGET_OPTION = '__new_target__'


// ─── Edge handle id (outside component, stable reference) ────────────────────

function edgeHandleId(
  _sourceId: string,
  fieldPath: string,
): { sourceHandle: string; targetHandle: string } {
  return { sourceHandle: `path-${fieldPath}`, targetHandle: '__in' }
}

// ─── Component ───────────────────────────────────────────────────────────────

interface Props {
  viewKey: string
  savedPositions?: GraphPositions
  onSavePositions?: (positions: GraphPositions, recordHistory: boolean) => Promise<void>
  graphData: GraphData
  /** 图视图所属文件，用于在空白处新建节点时决定写入位置。 */
  filePath: string
  activeType?: string
  enabledFieldsOverride?: readonly string[]
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
  /** 空白处新建节点时的字段草稿与插入通道，复用记录创建对话框。 */
  onCreateRecordDraft?: (actualType: string) => Promise<CreateRecordDraft>
  onInsertRecord?: (recordKey: string, actualType: string, fields: FieldValue) => Promise<void>
  /** 节点右键共用记录菜单所需的数据与操作。 */
  recordGroups?: readonly EditorRecordGroup[]
  onDropRecordIntoGroup?: (sources: readonly RecordCoordinate[], groupId: string) => void
  onRenameRecord?: (filePath: string, coordinate: RecordCoordinate, newKey: string) => Promise<RecordRow | void>
  onDeleteRecord?: (filePath: string, coordinate: RecordCoordinate) => Promise<void>
  /** 将“新建节点 + 建立引用”合并成单步撤销。 */
  runHistoryBatch?: (operation: () => Promise<void>) => Promise<void>
  onExitLeft?: () => void
  onExitUp?: () => void
  onExitRight?: () => void
  firstRecordFocusRequest?: number
  onFirstRecordFocusConsumed?: (request: number) => void
}

export function GraphView({ viewKey, savedPositions, onSavePositions, graphData, filePath, activeType, enabledFieldsOverride, visibleCardFields, fileCapabilities, diagnostics, onOpenRecord, onSelectRecord, onClearSelection, selectedCoordinate, onWriteField, onCollectionEdit, onDiagnosticBadgeClick, onCreateRecordDraft, onInsertRecord, recordGroups, onDropRecordIntoGroup, onRenameRecord, onDeleteRecord, runHistoryBatch, onExitLeft, onExitUp, onExitRight, firstRecordFocusRequest, onFirstRecordFocusConsumed }: Props) {
  const lookups = useEditorLookups()
  const previousViews = useRef<GraphNodeView[]>([])
  const [selectedElement, setSelectedElement] = useState<GraphFocus>(null)
  const [selectedNodeIds, setSelectedNodeIds] = useState<Set<string>>(new Set())
  // 框选连线允许批量选择；删除时逐条复用现有字段/集合写入历史。
  const [selectedEdgeIds, setSelectedEdgeIds] = useState<Set<string>>(new Set())
  useEffect(() => {
    if (selectedCoordinate) setSelectedElement({ kind: 'node', id: coordinateId(selectedCoordinate.coordinate) })
    else setSelectedElement(current => current?.kind === 'node' ? null : current)
  }, [selectedCoordinate?.file, selectedCoordinate?.coordinate.actual_type, selectedCoordinate?.coordinate.key])
  const [zoomCompactNodes, setZoomCompactNodes] = useState(false)
  const sourceGraph = useMemo(
    () => ({
      nodes: (previousViews.current = reconcileGraphViews(previousViews.current, graphData.nodes)),
      edges: graphData.edges.map(graphEdgeView),
    }),
    [graphData],
  )
  const topologySignature = useMemo(() => graphTopologySignature(sourceGraph), [sourceGraph])

  const availableFieldsKey = JSON.stringify(graphData.available_fields.slice().sort())
  const availableFields = useMemo<string[]>(
    () => JSON.parse(availableFieldsKey),
    [availableFieldsKey],
  )

  const defaultFields = useMemo(
    () => defaultEnabledFields(sourceGraph, availableFields, activeType),
    [topologySignature, availableFields, activeType],
  )

  const enabledFieldsKey = JSON.stringify((enabledFieldsOverride ?? defaultFields)
    .filter(field => availableFields.includes(field)).slice().sort())
  const enabledFields = useMemo<Set<string>>(() => new Set(JSON.parse(enabledFieldsKey)), [enabledFieldsKey])

  const graph = useMemo(() => ({
    ...sourceGraph,
    nodes: sourceGraph.nodes.map(node => ({
      ...node,
      fields: graphCardFields(node.fields, visibleCardFields, enabledFields),
    })),
  }), [sourceGraph, visibleCardFields, enabledFields])


  // 展开状态只改变卡片内容，不重新执行关系布局。
  const [nodeExpandedMap, setNodeExpandedMap] = useState<Map<string, boolean>>(new Map())
  // Per-node set of expanded sub-row paths
  const [nodeRowExpandedMap, setNodeRowExpandedMap] = useState<Map<string, Set<string>>>(new Map())
  // 仅为选中的关系生成端口和强制展开路径，其他字段遵守可见性设置。
  const relations = useMemo(() => new Map(sourceGraph.nodes.map(node => [node.id,
    relationPorts(node.fields.filter(field => enabledFields.has(field.name))),
  ])), [sourceGraph.nodes, enabledFields])
  const expandedRows = useMemo(() => new Map(graph.nodes.map(node => [node.id,
    new Set([...(nodeRowExpandedMap.get(node.id) ?? []), ...(relations.get(node.id)?.expanded ?? [])]),
  ])), [graph.nodes, nodeRowExpandedMap, relations])
  const expandedNodes = useMemo(() => new Map(graph.nodes.map(node => [node.id,
    (relations.get(node.id)?.ports.length ?? 0) > 0 || (nodeExpandedMap.get(node.id) ?? false),
  ])), [graph.nodes, relations, nodeExpandedMap])

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
  const [positionSaving, setPositionSaving] = useState(false)
  const [layoutError, setLayoutError] = useState<string | null>(null)
  const retainedPositions = useRef(new Map<string, { x: number; y: number }>())
  const layoutEpoch = useRef(0)
  const savePositionsRef = useRef(onSavePositions)
  savePositionsRef.current = onSavePositions
  const dragStartPositions = useRef(new Map<string, { x: number; y: number }>())
  const fitted = useRef(false)
  const viewportRef = useRef({ x: 0, y: 0, zoom: 1 })
  useEffect(() => {
    retainedPositions.current.clear()
    fitted.current = false
  }, [viewKey])

  useEffect(() => {
    if (savedPositions) retainedPositions.current = new Map(Object.entries(savedPositions)
      .map(([id, [x, y]]) => [id, { x, y }]))
  }, [savedPositions, viewKey])

  async function persistPositions(next: Map<string, { x: number; y: number }>, recordHistory: boolean) {
    const serialized: GraphPositions = Object.fromEntries([...next].map(([id, point]) => [id, [point.x, point.y]]))
    await savePositionsRef.current?.(serialized, recordHistory)
  }

  async function relayout() {
    const epoch = ++layoutEpoch.current
    const previous = new Map(retainedPositions.current)
    setLayoutBusy(true)
    setLayoutError(null)
    try {
      const next = await layoutGraph(graph, enabledFields, activeType, expandedNodes, expandedRows, runGraphLayoutInWorker)
      if (epoch !== layoutEpoch.current) return
      // 明确重新布局时覆盖用户位置；撤销仍可恢复操作前的位置。
      retainedPositions.current = new Map(next.positions)
      setLayout(next)
      requestAnimationFrame(fitGraph)
      setPositionSaving(true)
      await persistPositions(next.positions, true)
    } catch (error) {
      if (epoch !== layoutEpoch.current) return
      retainedPositions.current = previous
      setLayout(current => ({ ...current, positions: previous }))
      setLayoutError(error instanceof Error ? error.message : String(error))
    } finally {
      setPositionSaving(false)
      if (epoch === layoutEpoch.current) setLayoutBusy(false)
    }
  }

  useEffect(() => {
    let cancelled = false
    const epoch = ++layoutEpoch.current
    setLayoutBusy(retainedPositions.current.size === 0 && graph.nodes.length > 0 && enabledFields.size > 0)
    setLayoutError(null)
    layoutGraph(
      graph,
      enabledFields,
      activeType,
      expandedNodes,
      expandedRows,
      runGraphLayoutInWorker,
      retainedPositions.current,
      new Map(reactFlowRef.current?.getNodes().flatMap(node => node.measured?.height
        ? [[node.id, node.measured.height] as const] : []) ?? []),
    )
      .then(next => {
        if (!cancelled && epoch === layoutEpoch.current) {
          const changed = [...next.positions].some(([id, point]) => {
            const previous = retainedPositions.current.get(id)
            return !previous || previous.x !== point.x || previous.y !== point.y
          })
          retainedPositions.current = new Map([...retainedPositions.current, ...next.positions])
          setLayout(next)
          setLayoutBusy(false)
          if (changed) void persistPositions(retainedPositions.current, false).catch(error => {
            if (!cancelled) setLayoutError(error instanceof Error ? error.message : String(error))
          })
        }
      })
      .catch(err => {
        console.error('Failed to layout graph', err)
        if (!cancelled) {
          setLayoutBusy(false)
          setLayoutError(err instanceof Error ? err.message : String(err))
        }
      })
    return () => {
      cancelled = true
      if (epoch === layoutEpoch.current) layoutEpoch.current++
    }
  }, [viewKey, topologySignature, enabledFields, activeType, savedPositions])

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

  // Group outgoing edge paths by source node id (used to render per-path handles).
  const outgoingPathsByNode = useMemo(() => {
    const m = new Map<string, string[]>()
    for (const [id, relation] of relations) m.set(id, relation.ports.map(port => port.id))
    for (const e of [...forwardEdges, ...backEdges]) {
      const list = m.get(e.source) ?? []
      if (!list.includes(e.field_path)) list.push(e.field_path)
      m.set(e.source, list)
    }
    return m
  }, [forwardEdges, backEdges, relations])

  const nodeActionPort = useRef({ onWriteField, onCollectionEdit, onOpenRecord, onDiagnosticBadgeClick })
  nodeActionPort.current = { onWriteField, onCollectionEdit, onOpenRecord, onDiagnosticBadgeClick }
  const nodeActions = useRef(new Map<string, {
    expand: () => void
    row: (path: string, expanded: boolean) => void
    edit: (path: FieldPathSegment[], value: FieldValue) => void
    collection: (path: FieldPathSegment[], edit: CollectionEdit) => void
    open: () => void
    diagnostic: () => void
  }>())

  const nodeDescriptions: Node[] = useMemo(
    () => (
      visibleNodes.map(n => {
        const actionKey = JSON.stringify([n.file_path, n.id])
        let actions = nodeActions.current.get(actionKey)
        if (!actions) {
          actions = {
            expand: () => toggleNodeExpanded(n.id),
            row: (path, expanded) => handleRowToggle(n.id, path, expanded),
            edit: (path, value) => { void nodeActionPort.current.onWriteField?.(n.file_path, n.coordinate, path, value) },
            collection: (path, edit) => { void nodeActionPort.current.onCollectionEdit?.(n.file_path, n.coordinate, path, edit) },
            open: () => nodeActionPort.current.onOpenRecord(n.file_path, n.coordinate),
            diagnostic: () => nodeActionPort.current.onDiagnosticBadgeClick?.(n.file_path, n.coordinate, null),
          }
          nodeActions.current.set(actionKey, actions)
        }
        const capability = fileCapabilities?.[n.file_path]
        const editable = !!onWriteField && (capability ? isEditableCapabilities(capability) : isEditableFile(n.file_path))
        const rowExpanded = expandedRows.get(n.id)
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
        const outgoingPaths = (outgoingPathsByNode.get(n.id) ?? []).filter(path => {
          const port = relations.get(n.id)?.ports.find(port => port.id === path)
          return !port?.append || (editable && !!onCollectionEdit && !port.readOnly)
        })
        const estimated = estimateHandleOffsets(n, outgoingPaths, expandedNodes.get(n.id) ?? false)
        return {
          id: n.id,
          selected: selectedNodeIds.has(n.id),
          // 节点代表记录，Delete 只应删除连线，不能把节点从画布移除。
          deletable: false,
          type: 'cfd',
          initialWidth: 280,
          initialHeight: estimateNodeHeight(n, expandedNodes.get(n.id) ?? false, rowExpanded ?? new Set()),
          // 尚未进入屏幕的节点也提供端口几何，使跨越视口的连线可以绘制。
          handles: [
            { id: '__in', type: 'target', position: Position.Left, x: -6, y: estimated.headerCenterY - 6, width: 12, height: 12 },
            ...outgoingPaths.map(path => ({ id: `path-${path}`, type: 'source' as const, position: Position.Right,
              x: 274, y: (estimated.pathOffsets.get(path) ?? estimated.headerCenterY) - 6, width: 12, height: 12 })),
          ],
          position: retainedPositions.current.get(n.id) ?? positions.get(n.id) ?? { x: 0, y: 0 },
          data: {
            graphNode: n,
            expanded: expandedNodes.get(n.id) ?? false,
            ports: relations.get(n.id)?.ports ?? [],
            outgoingPaths,
            compact: compactNodes,
            rowExpandKey: rowExpanded ? Array.from(rowExpanded).sort().join('|') : '',
            expandedPaths: rowExpanded ?? new Set<string>(),
            onToggleExpand: actions.expand,
            onRowToggle: actions.row,
            onEdit: editable ? actions.edit : undefined,
            onCollectionEdit: editable && onCollectionEdit ? actions.collection : undefined,
            onCtrlClick: actions.open,
            selected: selectedNodeIds.has(n.id) || (selectedElement?.kind === 'node' && selectedElement.id === n.id),
            diagSeverity: nodeSev,
            onDiagBadgeClick: onDiagnosticBadgeClick ? actions.diagnostic : undefined,
          } satisfies NodeData,
        }
      })
    ),
    [visibleNodes, positions, expandedNodes, expandedRows, relations, outgoingPathsByNode, compactNodes, toggleNodeExpanded, handleRowToggle, onWriteField, onCollectionEdit, fileCapabilities, selectedElement, selectedNodeIds, diagnosticIndex, onDiagnosticBadgeClick]
  )
  const [rfNodes, setRfNodes] = useState<Node[]>([])
  useLayoutEffect(() => {
    setRfNodes(current => reconcileFlowNodes(current, nodeDescriptions))
  }, [nodeDescriptions])
  const onNodesChange = useCallback((changes: NodeChange[]) => {
    // 不在拖动帧中重建 layout 和所有卡片；只修改变化节点并保留 measured。
    setRfNodes(current => {
      const measured = new Set(changes.filter(change => change.type === 'dimensions').map(change => change.id))
      const occupied = current.filter(node => !changes.some(change => change.type === 'position' && change.id === node.id))
      const adjusted = changes.map(change => {
        if (change.type !== 'position' || !change.position) return change
        let { x, y } = change.position
        for (const other of occupied) {
          const ox = other.position.x; const oy = other.position.y
          const ow = other.measured?.width ?? 280; const oh = other.measured?.height ?? 160
          if (Math.abs(x - ox - ow) <= 18) x = ox + ow + 24
          else if (Math.abs(x + 280 - ox) <= 18) x = ox - 304
          if (Math.abs(y - oy) <= 18) y = oy
          else if (Math.abs(y + 160 - oy - oh) <= 18) y = oy + oh - 160
        }
        return { ...change, position: { x, y } }
      })
      for (const change of adjusted) {
        if (change.type === 'position' && change.position) retainedPositions.current.set(change.id, change.position)
      }
      return applyNodeChanges(adjusted, current).map(node => measured.has(node.id) && node.handles
        ? { ...node, handles: undefined } : node)
    })
  }, [])
  const reactFlowRef = useRef<ReactFlowInstance<Node, Edge> | null>(null)
  const fitGraph = useCallback(() => {
    const instance = reactFlowRef.current
    const container = wrapRef.current
    if (!instance || !container || !instance.getNodes().length) return
    // 直接按已知节点边界计算视口，避免等待屏幕外节点测量造成闪烁。
    void instance.setViewport(getViewportForBounds(
      instance.getNodesBounds(instance.getNodes()),
      container.clientWidth,
      container.clientHeight,
      0.1,
      1.2,
      0.25,
    ))
  }, [])
  const dragSource = useRef<{ nodeId: string; portId: string } | null>(null)
  const connectionDone = useRef(false)
  const reconnectEdgeRef = useRef<Edge | null>(null)
  // 重连开始时 React Flow 会同步再触发一次 onConnectStart，用一次性标记跳过它。
  const reconnectStartRef = useRef(false)
  const [connectionError, setConnectionError] = useState<string | null>(null)
  const [picker, setPicker] = useState<{
    nodeId: string; portId: string; x: number; y: number; targets: RefTarget[]
    targetType: string
    /** 空白处落点的 flow 坐标；仅拖线到空白时存在，用于“新建节点”落位。 */
    flowPosition: { x: number; y: number } | null
  } | null>(null)
  // 选择“新建节点”后暂停连接，待记录创建完成再连线并落位。
  const [newTarget, setNewTarget] = useState<{
    nodeId: string; portId: string; targetType: string
    flowPosition: { x: number; y: number }
  } | null>(null)
  // 节点右键共用菜单及其后续对话框。
  const [nodeMenu, setNodeMenu] = useState<{
    anchorX: number; anchorY: number; filePath: string; coordinate: RecordCoordinate
  } | null>(null)
  const [nodeMenuAction, setNodeMenuAction] = useState<
    | { kind: 'rename'; filePath: string; coordinate: RecordCoordinate }
    | { kind: 'delete'; filePath: string; coordinate: RecordCoordinate }
    | null
  >(null)
  const dragVersion = useRef(0)

  function sourcePort(nodeId: string, handle: string | null | undefined) {
    return relations.get(nodeId)?.ports.find(port => `path-${port.id}` === handle)
  }
  function editablePort(nodeId: string, port: RelationPort | undefined) {
    const node = currentNodeById.get(nodeId)
    if (!node || !port || port.readOnly || !onWriteField || (port.append && !onCollectionEdit)) return false
    const capability = fileCapabilities?.[node.file_path]
    return capability ? isEditableCapabilities(capability) : isEditableFile(node.file_path)
  }
  function beginConnection(nodeId: string | null, handle: string | null) {
    dragVersion.current++
    connectionDone.current = false
    dragSource.current = null
    setPicker(null)
    setNewTarget(null)
    setConnectionError(null)
    if (!nodeId) return
    const port = sourcePort(nodeId, handle)
    if (!port || !editablePort(nodeId, port)) return
    dragSource.current = { nodeId, portId: port.id }
    void lookups.loadRefTargets(port.targetType)
  }
  async function commitRelation(nodeId: string, portId: string, target: RefTarget) {
    const node = currentNodeById.get(nodeId)
    const port = relations.get(nodeId)?.ports.find(port => port.id === portId)
    if (!node || !port || !editablePort(nodeId, port)) return
    try {
      // 使用现有写入通道，统一执行校验、配置落盘和撤销历史记录。
      const value = relationValue(port, target.coordinate.key)
      if (port.append) await onCollectionEdit!(node.file_path, node.coordinate, port.path, { kind: 'array_append', value })
      else await onWriteField!(node.file_path, node.coordinate, port.path, value)
      setPicker(null)
    } catch (error) {
      setConnectionError(error instanceof Error ? error.message : String(error))
    }
  }
  function compatibleTarget(nodeId: string, targets: RefTarget[]) {
    const node = currentNodeById.get(nodeId)
    return targets.find(target => target.coordinate.actual_type === node?.actual_type && target.coordinate.key === node?.key)
  }
  function validConnection(connection: Connection | Edge) {
    const port = sourcePort(connection.source, connection.sourceHandle)
    return !!port && editablePort(connection.source, port)
      && !!compatibleTarget(connection.target, lookups.cachedRefTargets(port.targetType) ?? [])
  }
  function connect(connection: Connection) {
    const port = sourcePort(connection.source, connection.sourceHandle)
    if (!port) return
    const target = compatibleTarget(connection.target, lookups.cachedRefTargets(port.targetType) ?? [])
    if (!target || !editablePort(connection.source, port)) return
    connectionDone.current = true
    const reconnecting = reconnectEdgeRef.current
    // 拖动起点是把引用搬到新起点：先建立新引用，再清掉旧起点的引用，避免残留成重复边。
    if (reconnecting
      && (connection.source !== reconnecting.source || connection.sourceHandle !== reconnecting.sourceHandle)) {
      onEdgesDelete([reconnecting])
    }
    void commitRelation(connection.source, port.id, target)
  }

  const onEdgesDelete = useCallback((deleted: Edge[]) => {
    // React Flow 默认使用 Delete/Backspace 删除选中的边；删除操作复用字段写入历史，因而可撤销。
    setSelectedEdgeIds(new Set())
    const arrayRemovals: { file: string; coordinate: RecordCoordinate; path: FieldPathSegment[]; index: number }[] = []
    for (const edge of deleted) {
      const graphEdge = [...forwardEdges, ...backEdges].find(item => graphEdgeId(
        forwardEdges.includes(item) ? 'fwd' : 'back', item,
      ) === edge.id)
      if (!graphEdge) continue
      const port = relations.get(graphEdge.source)?.ports.find(item => item.id === graphEdge.field_path)
      const node = currentNodeById.get(graphEdge.source)
      if (!port || !node || !editablePort(graphEdge.source, port)) continue
      const lastPath = port.path[port.path.length - 1]
      if (lastPath?.kind === 'index') {
        // 列表项关系的连线对应数组元素；批量删除时按索引降序执行，避免删除后索引位移。
        arrayRemovals.push({ file: node.file_path, coordinate: node.coordinate,
          path: port.path.slice(0, -1), index: lastPath.value })
      } else if (port.append) {
        continue
      } else if (!port.nullable) {
        // 必填引用不能存在空值；Delete 进入目标选择，确认新目标后原关系才替换。
        void lookups.loadRefTargets(port.targetType).then(result => {
          if (!result.ok) { setConnectionError(result.error ?? '加载引用目标失败'); return }
          const element = wrapRef.current?.querySelector<HTMLElement>(`.graph-node[data-nodeid="${CSS.escape(node.id)}"]`)
          const rect = element?.getBoundingClientRect()
          setPicker({ nodeId: graphEdge.source, portId: port.id,
            x: Math.max(8, Math.min(rect?.right ?? 300, window.innerWidth - 288)),
            y: Math.max(8, Math.min(rect?.top ?? 80, window.innerHeight - 40)), targets: result.value,
            targetType: port.targetType, flowPosition: null })
        })
      } else {
        void onWriteField?.(node.file_path, node.coordinate, port.path, { kind: 'option_none' })
      }
    }
    arrayRemovals.sort((a, b) => b.index - a.index)
    for (const removal of arrayRemovals) {
      void onCollectionEdit?.(removal.file, removal.coordinate, removal.path, { kind: 'array_remove', index: removal.index })
    }
  }, [forwardEdges, backEdges, relations, currentNodeById, onWriteField, onCollectionEdit, fileCapabilities, lookups])

  async function endConnection(event: MouseEvent | TouchEvent) {
    const source = dragSource.current
    dragSource.current = null
    if (!source || connectionDone.current) return
    const version = dragVersion.current
    const port = relations.get(source.nodeId)?.ports.find(port => port.id === source.portId)
    if (!port) return
    const point = 'changedTouches' in event ? event.changedTouches[0] : event
    if (!point) return
    const targetId = document.elementFromPoint(point.clientX, point.clientY)?.closest<HTMLElement>('.graph-node')?.dataset.nodeid
    const result = await lookups.loadRefTargets(port.targetType)
    if (version !== dragVersion.current) return
    if (!result.ok) {
      setConnectionError(result.error ?? '加载引用目标失败')
      return
    }
    if (targetId) {
      const target = compatibleTarget(targetId, result.value)
      if (target) await commitRelation(source.nodeId, source.portId, target)
      else setConnectionError('目标节点类型不兼容')
    } else {
      const reconnecting = reconnectEdgeRef.current
      reconnectEdgeRef.current = null
      if (reconnecting) {
        // 重连拖到空白处即删除原关系，不在拖动中提前改变数据。
        onEdgesDelete([reconnecting])
        return
      }
      const flowPosition = reactFlowRef.current?.screenToFlowPosition({
        x: point.clientX, y: point.clientY,
      }) ?? null
      setPicker({ nodeId: source.nodeId, portId: source.portId,
        x: Math.max(8, Math.min(point.clientX, window.innerWidth - 288)),
        y: Math.max(8, Math.min(point.clientY, window.innerHeight - 40)), targets: result.value,
        targetType: port.targetType, flowPosition })
    }
  }

  useEffect(() => {
    dragVersion.current++
    setPicker(null)
    setNewTarget(null)
    setNodeMenu(null)
    setConnectionError(null)
    setSelectedEdgeIds(new Set())
    return () => { dragVersion.current++; dragSource.current = null }
  }, [viewKey, topologySignature])

  useEffect(() => {
    if (rfNodes.length === 0 || fitted.current) return
    let fitFrame = 0
    const measureFrame = requestAnimationFrame(() => {
      fitFrame = requestAnimationFrame(() => {
        const instance = reactFlowRef.current
        if (instance && !fitted.current) {
          fitGraph()
          fitted.current = true
        }
      })
    })
    return () => {
      cancelAnimationFrame(measureFrame)
      cancelAnimationFrame(fitFrame)
    }
  }, [positions, rfNodes.length])

  const previousEdges = useRef<Edge[]>([])
  const rfEdges: Edge[] = useMemo(() => {
    const fwdEdges: Edge[] = forwardEdges
      .filter(e => positions.has(e.source) && positions.has(e.target))
      .map(e => {
        const { sourceHandle, targetHandle } = edgeHandleId(e.source, e.field_path)
        return {
          id: graphEdgeId('fwd', e),
          selected: selectedEdgeIds.has(graphEdgeId('fwd', e))
            || (selectedElement?.kind === 'edge' && selectedElement.id === graphEdgeId('fwd', e)),
          reconnectable: true,
          source: e.source,
          target: e.target,
          sourceHandle,
          targetHandle,
          label: compactNodes ? undefined : e.field_path,
          type: 'shader',
          animated: false,
          className: 'rf-edge rf-edge-fwd',
          interactionWidth: 24,
          style: { stroke: 'var(--graph-edge)', strokeWidth: 2 },
          labelStyle: { fill: 'var(--graph-edge-label)', fontSize: 10, fontFamily: 'JetBrains Mono, monospace' },
          labelBgStyle: { fill: 'var(--graph-edge-label-bg)', fillOpacity: 0.92 },
          labelBgPadding: [4, 2] as [number, number],
          labelBgBorderRadius: 3,
          pathOptions: { curvature: 0.28 },
        }
      })

    const bkEdges: Edge[] = backEdges
      .filter(e => positions.has(e.source) && positions.has(e.target))
      .map(e => ({
        id: graphEdgeId('back', e),
        selected: selectedEdgeIds.has(graphEdgeId('back', e))
          || (selectedElement?.kind === 'edge' && selectedElement.id === graphEdgeId('back', e)),
        reconnectable: true,
        source: e.source,
        target: e.target,
        sourceHandle: `path-${e.field_path}`,
        targetHandle: '__in',
        label: compactNodes ? undefined : e.field_path,
        type: 'shader',
        animated: false,
        className: 'rf-edge rf-edge-bk',
        interactionWidth: 24,
        style: { stroke: 'var(--graph-back-edge)', strokeWidth: 2, strokeDasharray: '6 3' },
        zIndex: 1,
        labelStyle: { fill: 'var(--graph-back-edge)', fontSize: 10, fontFamily: 'JetBrains Mono, monospace' },
        labelBgStyle: { fill: 'var(--graph-edge-label-bg)', fillOpacity: 0.92 },
        labelBgPadding: [4, 2] as [number, number],
        labelBgBorderRadius: 3,
        pathOptions: { curvature: 0.28 },
      }))

    const byId = new Map(previousEdges.current.map(edge => [edge.id, edge]))
    const next = [...fwdEdges, ...bkEdges].map(edge => {
      const old = byId.get(edge.id)
      return old && sameGraphValue(old, edge) ? old : edge
    })
    if (next.length === previousEdges.current.length && next.every((edge, index) => edge === previousEdges.current[index])) return previousEdges.current
    previousEdges.current = next
    return next
  }, [forwardEdges, backEdges, positions, compactNodes, relations, fileCapabilities, onWriteField, onCollectionEdit, selectedElement, selectedEdgeIds])

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

  const hoveredElement = useRef<GraphFocus>(null)
  const highlightState = useRef({ edges: rfEdges, selected: selectedElement })
  highlightState.current = { edges: rfEdges, selected: selectedElement }
  const applyHighlights = useCallback(() => {
    const wrap = wrapRef.current
    if (!wrap) return
    const { edges, selected } = highlightState.current
    const focus = graphHighlights(edges, selected, hoveredElement.current)
    const active = !!selected || !!hoveredElement.current
    wrap.classList.toggle('is-hovering', active)
    wrap.querySelectorAll<HTMLElement>('.graph-node').forEach(element => {
      const highlighted = focus.nodes.has(element.dataset.nodeid ?? '')
      element.classList.toggle('hover-highlight', active && highlighted)
      element.classList.toggle('hover-dim', active && !highlighted)
    })
    wrap.querySelectorAll<SVGGElement>('.react-flow__edge').forEach(element => {
      const highlighted = focus.edges.has(element.dataset.id ?? '')
      element.classList.toggle('hover-highlight', active && highlighted)
      element.classList.toggle('hover-dim', active && !highlighted)
    })
  }, [])
  useLayoutEffect(applyHighlights, [rfEdges, selectedElement, rfNodes, applyHighlights])
  useEffect(() => {
    const wrap = wrapRef.current
    if (!wrap) return
    // 可视区域裁剪重新挂载元素后补齐高亮，不用修改节点数据触发整图渲染。
    let frame = 0
    const observer = new MutationObserver(() => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(applyHighlights)
    })
    observer.observe(wrap, { childList: true, subtree: true })
    return () => { observer.disconnect(); cancelAnimationFrame(frame) }
  }, [applyHighlights])
  const onNodeMouseEnter = useCallback((_: unknown, node: Node) => {
    hoveredElement.current = { kind: 'node', id: node.id }
    applyHighlights()
  }, [applyHighlights])
  const onNodeMouseLeave = useCallback(() => {
    hoveredElement.current = null
    applyHighlights()
  }, [applyHighlights])

  const handleViewportChange = useCallback((viewport: { x: number; y: number; zoom: number }) => {
    viewportRef.current = viewport
    const next = isCompactGraphZoom(viewport.zoom)
    setZoomCompactNodes(prev => prev === next ? prev : next)
  }, [])
  const boxSelectRef = useRef<{ x: number; y: number } | null>(null)
  // 拖动中显示真实框选范围，结束后收起为选中节点包围盒。
  const [boxSelect, setBoxSelect] = useState<{ x: number; y: number; width: number; height: number } | null>(null)
  // 右键拖动框选：节点用重叠面积判定，连线用曲线采样判定。
  useEffect(() => {
    const root = wrapRef.current
    if (!root) return
    const pathIntersectsRect = (path: SVGPathElement, rect: GraphRect): boolean => {
      const bounds = path.getBoundingClientRect()
      if (bounds.right < rect.left || bounds.left > rect.right
        || bounds.bottom < rect.top || bounds.top > rect.bottom) return false
      const total = path.getTotalLength()
      const matrix = path.getScreenCTM()
      if (!total || !matrix) return false
      const samples: { x: number; y: number }[] = []
      const step = Math.max(4, total / 160)
      for (let length = 0; length <= total + step / 2; length += step) {
        const point = path.getPointAtLength(Math.min(length, total))
        const screen = new DOMPoint(point.x, point.y).matrixTransform(matrix)
        samples.push({ x: screen.x, y: screen.y })
      }
      return polylineIntersectsRect(samples, rect)
    }
    const updateSelection = (left: number, top: number, right: number, bottom: number) => {
      const rect: GraphRect = { left, top, right, bottom }
      const nextNodes = new Set<string>()
      root.querySelectorAll<HTMLElement>('.react-flow__node').forEach(element => {
        const bounds = element.getBoundingClientRect()
        const overlap = Math.max(0, Math.min(bounds.right, right) - Math.max(bounds.left, left))
          * Math.max(0, Math.min(bounds.bottom, bottom) - Math.max(bounds.top, top))
        if (overlap >= bounds.width * bounds.height * 0.2 && element.dataset.id) nextNodes.add(element.dataset.id)
      })
      const nextEdges = new Set<string>()
      root.querySelectorAll<SVGGElement>('.react-flow__edge').forEach(element => {
        const path = element.querySelector<SVGPathElement>('.react-flow__edge-path')
        if (path && element.dataset.id && pathIntersectsRect(path, rect)) nextEdges.add(element.dataset.id)
      })
      setSelectedNodeIds(nextNodes)
      setSelectedEdgeIds(nextEdges)
    }
    const down = (event: PointerEvent | MouseEvent) => {
      if (event.button !== 2) return
      // pointer 与 mouse 事件在部分环境同时到达，第二次直接忽略。
      if (boxSelectRef.current) return
      event.preventDefault()
      event.stopPropagation()
      boxSelectRef.current = { x: event.clientX, y: event.clientY }
      const move = (moveEvent: PointerEvent) => {
        const start = boxSelectRef.current
        if (!start) return
        const left = Math.min(start.x, moveEvent.clientX)
        const top = Math.min(start.y, moveEvent.clientY)
        const right = Math.max(start.x, moveEvent.clientX)
        const bottom = Math.max(start.y, moveEvent.clientY)
        setBoxSelect({ x: left, y: top, width: right - left, height: bottom - top })
        updateSelection(left, top, right, bottom)
      }
      const up = () => {
        boxSelectRef.current = null
        setBoxSelect(null)
        window.removeEventListener('pointermove', move, true)
        window.removeEventListener('pointerup', up, true)
      }
      window.addEventListener('pointermove', move, true)
      window.addEventListener('pointerup', up, true)
    }
    const captureDown = (event: PointerEvent) => {
      if (root.contains(event.target as globalThis.Node)) down(event)
    }
    const mouseDown = (event: MouseEvent) => {
      if ((event.target as Element)?.closest?.('.graph-view-wrap') === root) down(event)
    }
    window.addEventListener('pointerdown', captureDown, true)
    window.addEventListener('mousedown', mouseDown, true)
    return () => {
      window.removeEventListener('pointerdown', captureDown, true)
      window.removeEventListener('mousedown', mouseDown, true)
    }
  }, [])

  // 框选后显示选中节点的包围盒；初始拖拽中一旦有节点入选即出现。
  const selectionBounds = useMemo(
    () => selectedNodeIds.size > 0
      ? selectedNodeBounds(rfNodes, selectedNodeIds, { width: 280, height: 160 })
      : null,
    [rfNodes, selectedNodeIds],
  )
  // 拖动包围盒即整组移动选中节点，落点按 flow 坐标换算，避免受缩放影响。
  const beginGroupDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || selectedNodeIds.size === 0) return
    event.preventDefault()
    event.stopPropagation()
    const instance = reactFlowRef.current
    if (!instance) return
    // 以当前渲染中的坐标为准，保证刚布局完还没落盘的节点也能整组移动。
    const start = new Map<string, { x: number; y: number }>()
    for (const node of rfNodes) {
      if (selectedNodeIds.has(node.id)) start.set(node.id, { ...node.position })
    }
    if (start.size === 0) return
    const startFlow = instance.screenToFlowPosition({ x: event.clientX, y: event.clientY })
    const move = (moveEvent: PointerEvent) => {
      const flow = reactFlowRef.current?.screenToFlowPosition({ x: moveEvent.clientX, y: moveEvent.clientY })
      if (!flow) return
      const dx = flow.x - startFlow.x
      const dy = flow.y - startFlow.y
      for (const [id, base] of start) retainedPositions.current.set(id, { x: base.x + dx, y: base.y + dy })
      setRfNodes(current => current.map(node => {
        const base = start.get(node.id)
        return base ? { ...node, position: { x: base.x + dx, y: base.y + dy } } : node
      }))
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      const changed = [...start].some(([id, base]) => {
        const now = retainedPositions.current.get(id)
        return !now || now.x !== base.x || now.y !== base.y
      })
      if (!changed) return
      setPositionSaving(true)
      void persistPositions(new Map(retainedPositions.current), true).catch(error => {
        for (const [id, base] of start) retainedPositions.current.set(id, base)
        setRfNodes(current => current.map(node => {
          const base = start.get(node.id)
          return base ? { ...node, position: { ...base } } : node
        }))
        setLayoutError(error instanceof Error ? error.message : String(error))
      }).finally(() => setPositionSaving(false))
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  // 新建节点时做重复 Key 提示；以后端校验为准，这里只覆盖图里已有的同类型记录。
  const existingTargetKeys = useMemo(
    () => newTarget
      ? graphData.nodes.filter(node => node.coordinate.actual_type === newTarget.targetType).map(node => node.coordinate.key)
      : [],
    [graphData.nodes, newTarget],
  )

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
      onContextMenu={event => event.preventDefault()}
    >
      <div className="graph-view">
        {rfNodes.length === 0 ? (
          <div className="empty-hint">
            {layoutBusy
              ? '布局图谱中…'
              : layoutError
                ? `图谱布局失败：${layoutError}`
                : availableFields.length > 0 && enabledFields.size === 0
                  ? '未选择引用字段'
                  : '无可显示的引用关系'}
          </div>
        ) : (
          <>
            <ReactFlow
            nodes={rfNodes}
            defaultViewport={viewportRef.current}
            edges={rfEdges}
            nodeTypes={nodeTypes}
            edgeTypes={edgeTypes}
            nodesDraggable={!layoutBusy && !positionSaving}
            onlyRenderVisibleElements
            onNodeDragStart={(_event, node) => {
              dragStartPositions.current = new Map(retainedPositions.current)
              setSelectedElement({ kind: 'node', id: node.id })
            }}
            onNodeDragStop={() => {
              const previous = dragStartPositions.current
              const next = new Map(retainedPositions.current)
              if (![...next].some(([id, point]) => {
                const before = previous.get(id)
                return !before || before.x !== point.x || before.y !== point.y
              })) return
              setPositionSaving(true)
              void persistPositions(next, true).catch(error => {
                retainedPositions.current = previous
                setLayout(current => ({ ...current, positions: new Map(previous) }))
                setLayoutError(error instanceof Error ? error.message : String(error))
              }).finally(() => setPositionSaving(false))
            }}
            onNodesChange={onNodesChange}
            isValidConnection={validConnection}
            // 重连时 React Flow 会先触发 onReconnectStart，再触发 onConnectStart；
            // 后者会用固定端覆盖拖动源，必须跳过，否则端点拖不动。
            onConnectStart={(_event, params) => {
              if (reconnectStartRef.current) {
                reconnectStartRef.current = false
                return
              }
              beginConnection(params.nodeId, params.handleId)
            }}
            onConnect={connect}
            onEdgesDelete={onEdgesDelete}
            onConnectEnd={event => { void endConnection(event) }}
            edgesReconnectable
            edgesFocusable
            elementsSelectable
            deleteKeyCode={["Backspace", "Delete"]}
            onReconnectStart={(_event, edge) => {
              reconnectEdgeRef.current = edge
              reconnectStartRef.current = true
              beginConnection(edge.source, edge.sourceHandle ?? null)
            }}
            onReconnect={(_edge, connection) => connect(connection)}
            onReconnectEnd={event => {
              void endConnection(event)
              reconnectEdgeRef.current = null
              reconnectStartRef.current = false
            }}
            onNodeMouseEnter={onNodeMouseEnter}
            onNodeMouseLeave={onNodeMouseLeave}
            onEdgeMouseEnter={(_event, edge) => { hoveredElement.current = { kind: 'edge', id: edge.id }; applyHighlights() }}
            onEdgeMouseLeave={onNodeMouseLeave}
            onEdgeClick={(event, edge) => {
              event.stopPropagation()
              setSelectedEdgeIds(new Set())
              setSelectedElement({ kind: 'edge', id: edge.id })
            }}
            onNodeClick={(e, node) => {
              // Ctrl/Cmd+click jumps to the record view (handled in CfdNode).
              // Plain click opens the side inspector.
              if (e.ctrlKey || e.metaKey) return
              // 字段控件自行处理编辑，冒泡选择会触发侧栏提交并打断输入。
              if ((e.target as Element).closest('input, textarea, select, button, a, [contenteditable="true"], .nodrag')) return
              if (selectedElement?.kind === 'node' && selectedElement.id === node.id) return
              setSelectedNodeIds(new Set())
              setSelectedEdgeIds(new Set())
              setSelectedElement({ kind: 'node', id: node.id })
              if (!onSelectRecord) return
              const gn = (node.data as NodeData).graphNode
              onSelectRecord(gn.file_path, gn.coordinate)
            }}
            onNodeContextMenu={(event, node) => {
              event.preventDefault()
              const gn = (node.data as NodeData).graphNode
              setNodeMenu({
                anchorX: event.clientX, anchorY: event.clientY,
                filePath: gn.file_path, coordinate: gn.coordinate,
              })
            }}
            onPaneClick={() => {
              setPicker(null)
              setNodeMenu(null)
              setSelectedElement(null)
              setSelectedNodeIds(new Set())
              setSelectedEdgeIds(new Set())
              onClearSelection?.()
            }}
            onPaneContextMenu={event => event.preventDefault()}
            onViewportChange={handleViewportChange}
            selectionMode={SelectionMode.Full}
            selectNodesOnDrag
            connectionLineType={ConnectionLineType.Bezier}
            reconnectRadius={28}
            // 左键恢复平移；右键拖动框选节点。
            panOnDrag={[0]}
            selectionOnDrag={false}
            selectionKeyCode={null}
            onInit={instance => {
              reactFlowRef.current = instance
              requestAnimationFrame(() => {
                if (!fitted.current) {
                  fitGraph()
                  fitted.current = true
                }
              })
            }}
            proOptions={{ hideAttribution: true }}
            minZoom={0.1}
            maxZoom={2}
          >
            <Background color="var(--graph-bg-grid)" gap={24} size={1} />
            <Controls showInteractive={false}>
              <ControlButton onClick={fitGraph} title="适应视图" aria-label="适应视图">
                <Icon name="frame" size={16} />
              </ControlButton>
              <ControlButton onClick={() => { void relayout() }} disabled={layoutBusy || positionSaving}
                title="重新布局（重置节点位置）" aria-label="重新布局">
                <Icon name="refresh" size={16} />
              </ControlButton>
            </Controls>
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
            {/* 选中节点的持久包围盒：渲染在 viewport 内，随平移缩放自动贴合并可作为整组拖动手柄。 */}
            <ViewportPortal>
              {!boxSelect && selectionBounds && (
                <div
                  className="graph-selection-bbox nodrag nopan"
                  data-testid="graph-selection-bbox"
                  style={{
                    left: selectionBounds.left - 16,
                    top: selectionBounds.top - 16,
                    width: selectionBounds.right - selectionBounds.left + 32,
                    height: selectionBounds.bottom - selectionBounds.top + 32,
                  }}
                  onPointerDown={beginGroupDrag}
                  onClick={event => event.stopPropagation()}
                />
              )}
            </ViewportPortal>
          </ReactFlow>
          {!connectionError && <div className="graph-hint" title="点击节点打开侧边面板，Ctrl+点击跳转到记录视图">
            点击节点查看 · Ctrl+点击跳转
          </div>}
          </>
        )}
      </div>
      {boxSelect && <div className="graph-selection-box" style={{
        left: boxSelect.x, top: boxSelect.y, width: boxSelect.width, height: boxSelect.height,
      }} />}
      {picker && (
        <div className="nodrag nopan" style={{ position: 'fixed', left: picker.x, top: picker.y, width: 280, maxWidth: 'calc(100vw - 16px)', zIndex: 1000 }}>
          <SearchableSelect
            value=""
            autoFocus
            className="dc-input"
            ariaLabel="选择引用目标"
            placeholder="选择引用目标"
            options={[
              // 仅拖线到空白且具备记录创建通道时才提供“新建节点”，删除后补齐目标不改变落点语义。
              ...(picker.flowPosition && onCreateRecordDraft && onInsertRecord
                ? [{ value: NEW_TARGET_OPTION, label: '＋ 新建节点', description: `新建 ${picker.targetType} 并连接` }]
                : []),
              ...picker.targets.map((target, index) => ({ value: String(index), label: shortNameLabel(target.coordinate.key, target.short_name),
                description: `${target.coordinate.actual_type} · ${target.file_path}` })),
            ]}
            onCommit={value => {
              if (value === NEW_TARGET_OPTION && picker.flowPosition) {
                setNewTarget({
                  nodeId: picker.nodeId, portId: picker.portId,
                  targetType: picker.targetType, flowPosition: picker.flowPosition,
                })
                setPicker(null)
                return
              }
              const target = picker.targets[Number(value)]
              if (target) void commitRelation(picker.nodeId, picker.portId, target)
            }}
            onExit={() => setPicker(null)}
          />
        </div>
      )}
      {newTarget && onCreateRecordDraft && onInsertRecord && (
        <CreateRecordDialog
          actualType={newTarget.targetType}
          typeOptions={[]}
          existingKeys={existingTargetKeys}
          onCreateRecordDraft={onCreateRecordDraft}
          onInsertRecord={async (recordKey, actualType, fields) => {
            const create = async () => {
              await onInsertRecord(recordKey, actualType, fields)
              // 记录新节点坐标，布局刷新后即可落在拖放位置。
              retainedPositions.current.set(coordinateId({ actual_type: actualType, key: recordKey }), newTarget.flowPosition)
              setNewTarget(null)
              await commitRelation(newTarget.nodeId, newTarget.portId, {
                short_name: null,
                file_path: filePath,
                coordinate: { actual_type: actualType, key: recordKey },
              })
            }
            // 创建记录与建立引用合并成一步，Ctrl+Z 一次即可整体撤回。
            await (runHistoryBatch ? runHistoryBatch(create) : create())
          }}
          onClose={() => setNewTarget(null)}
        />
      )}
      {nodeMenu && (
        <RecordContextMenu
          request={{
            anchorX: nodeMenu.anchorX,
            anchorY: nodeMenu.anchorY,
            filePath: nodeMenu.filePath,
            coordinates: [nodeMenu.coordinate],
            primaryKey: nodeMenu.coordinate.key,
          }}
          groups={recordGroups}
          showOpenRecord
          canRename={!!onRenameRecord && (fileCapabilities?.[nodeMenu.filePath]?.can_edit_key ?? false)}
          canDelete={!!onDeleteRecord && (fileCapabilities?.[nodeMenu.filePath]?.can_delete_record ?? false)}
          canAddToGroup={(recordGroups?.length ?? 0) > 0 && !!onDropRecordIntoGroup}
          onOpenRecord={() => onOpenRecord(nodeMenu.filePath, nodeMenu.coordinate)}
          onRename={() => setNodeMenuAction({
            kind: 'rename', filePath: nodeMenu.filePath, coordinate: nodeMenu.coordinate,
          })}
          onDelete={() => setNodeMenuAction({
            kind: 'delete', filePath: nodeMenu.filePath, coordinate: nodeMenu.coordinate,
          })}
          onAddToGroup={onDropRecordIntoGroup}
          onClose={() => setNodeMenu(null)}
        />
      )}
      {nodeMenuAction?.kind === 'rename' && onRenameRecord && (
        <TextInputDialog
          title="重命名 Key"
          message="输入新的记录 Key"
          initialValue={nodeMenuAction.coordinate.key}
          confirmLabel="重命名"
          onClose={() => setNodeMenuAction(null)}
          onConfirm={async next => {
            if (next !== nodeMenuAction.coordinate.key) {
              await onRenameRecord(nodeMenuAction.filePath, nodeMenuAction.coordinate, next)
            }
            setNodeMenuAction(null)
          }}
        />
      )}
      {nodeMenuAction?.kind === 'delete' && onDeleteRecord && (
        <ConfirmDialog
          title="删除记录"
          message={`确认删除记录 ${nodeMenuAction.coordinate.key}？此操作不可撤销。`}
          confirmLabel="删除"
          danger
          onClose={() => setNodeMenuAction(null)}
          onConfirm={async () => {
            await onDeleteRecord(nodeMenuAction.filePath, nodeMenuAction.coordinate)
            setNodeMenuAction(null)
          }}
        />
      )}
      {(connectionError || layoutError) && <div role="alert" className="graph-hint">{connectionError ?? layoutError}</div>}
    </div>
  )
}
