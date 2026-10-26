import { nodeTypes, geometryHandles, GraphGeometryProbeHost, type NodeData } from './GraphNode'
import type { CreateLinkedRecordInput } from '../../../bindings/CreateLinkedRecordInput'

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { ReactFlow, Background, Controls, MiniMap, ConnectionLineType, SelectionMode, ControlButton, getViewportForBounds, ViewportPortal, type Node, type Edge, type ReactFlowInstance, type Connection, applyNodeChanges, type NodeChange } from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import type { GraphData } from '../../../bindings/GraphData'

import { shortNameLabel } from '../../record-editing/state/shortNames'
import type { RecordCoordinate } from '../../../bindings/RecordCoordinate'
import type { CollectionEdit } from '../../../bindings/CollectionEdit'
import type { RecordRow } from '../../../bindings/RecordRow'
import type { WriterCapabilities } from '../../../bindings/WriterCapabilities'
import { graphEdgeView, coordinateId, type DiagnosticItem, type FieldPathSegment, type FieldValue, type GraphNodeView } from '../../../wire'
import { isEditableCapabilities, isEditableFile } from '../../../shared/lib/editable'

import { GraphEdgeViewport, ShaderEdge } from './GraphView.edgeComponent'
import { boundsOf, getHelperLines, unionBounds, type AlignBounds, type HelperLines } from './GraphView.alignment'
import { defaultEnabledFields, graphEdgeId, graphTopologySignature, layoutGraph, type GraphLayoutResult } from './GraphView.layout'
import { runGraphLayoutInWorker } from '../layout/layoutWorkerClient'
import { LatestTaskQueue, SupersededTask } from '../layout/latestTaskQueue'
import { GraphRowGeometry } from '../layout/rowGeometry'

import { graphCardFields, relationPorts, relationValue, type RelationPort } from './GraphView.relations'
import { useEditorLookups } from '../../workspace/context/editContext'
import { SearchableSelect } from '../../../shared/ui/SearchableSelect'
import { Icon } from '../../../shared/ui/Icon'
import type { GraphPositions } from '../../workspace/state/editorState'
import { graphHighlights, polylineIntersectsRect, reconcileFlowNodes, reconcileGraphViews, sameGraphValue, selectedNodeBounds, type GraphFocus, type GraphRect } from './GraphView.state'
import { CreateRecordDialog } from '../../record-editing/components/CreateRecordDialog'
import { RecordContextMenu } from '../../record-editing/components/RecordContextMenu'
import { ConfirmDialog, TextInputDialog } from '../../../shared/ui/ActionDialog'
import type { CreateRecordDraft } from '../../../bindings/CreateRecordDraft'
import type { EditorRecordGroup } from '../../../bindings/EditorRecordGroup'
import type { RefTarget } from '../../../bindings/RefTarget'
import { buildRecordDiagnosticIndex, diagnosticsForRecord } from '../../diagnostics/state/recordDiagnostics'

// ─── Node data ───────────────────────────────────────────────────────────────

const edgeTypes = { shader: ShaderEdge }

// 拖线到空白处的下拉列表固定首项：先创建记录，再建立引用。
const NEW_TARGET_OPTION = '__new_target__'

/** 节点拖动对齐吸附阈值（flow 坐标像素）。 */
const ALIGN_THRESHOLD = 6
const NO_HELPER_LINES: HelperLines = {}

function sameHelperLines(left: HelperLines, right: HelperLines): boolean {
  return left.horizontal === right.horizontal && left.vertical === right.vertical
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
  viewKey: string
  savedPositions?: GraphPositions
  onSavePositions?: (positions: GraphPositions, recordHistory: boolean) => Promise<void>
  /** 缩略/完整模式按图视图持久化；缺省为缩略模式。 */
  savedCompact?: boolean
  onSaveCompact?: (compact: boolean) => void
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
  /** 节点右键共用记录菜单所需的数据与操作。 */
  recordGroups?: readonly EditorRecordGroup[]
  onDropRecordIntoGroup?: (sources: readonly RecordCoordinate[], groupId: string) => void
  onRenameRecord?: (filePath: string, coordinate: RecordCoordinate, newId: string) => Promise<RecordRow | void>
  onDeleteRecord?: (filePath: string, coordinate: RecordCoordinate) => Promise<void>
  /** 将“新建节点 + 建立引用”合并成单步撤销。 */
  onCreateLinkedRecord?: (input: CreateLinkedRecordInput) => Promise<void>
  onExitLeft?: () => void
  onExitUp?: () => void
  onExitRight?: () => void
  firstRecordFocusRequest?: number
  onFirstRecordFocusConsumed?: (request: number) => void
}

export function GraphView({ viewKey, savedPositions, onSavePositions, savedCompact, onSaveCompact, graphData, filePath, activeType, enabledFieldsOverride, visibleCardFields, fileCapabilities, diagnostics, onOpenRecord, onSelectRecord, onClearSelection, selectedCoordinate, onWriteField, onCollectionEdit, onDiagnosticBadgeClick, onCreateRecordDraft, recordGroups, onDropRecordIntoGroup, onRenameRecord, onDeleteRecord, onCreateLinkedRecord, onExitLeft, onExitUp, onExitRight, firstRecordFocusRequest, onFirstRecordFocusConsumed }: Props) {
  const lookups = useEditorLookups()
  const previousViews = useRef<GraphNodeView[]>([])
  const [selectedElement, setSelectedElement] = useState<GraphFocus>(null)
  const [activeEditorNodes, setActiveEditorNodes] = useState<ReadonlySet<string>>(new Set())
  const [connectionActive, setConnectionActive] = useState(false)
  const [selectedNodeIds, setSelectedNodeIds] = useState<Set<string>>(new Set())
  // 框选连线允许批量选择；删除时逐条复用现有字段/集合写入历史。
  const [selectedEdgeIds, setSelectedEdgeIds] = useState<Set<string>>(new Set())
  useEffect(() => {
    if (selectedCoordinate) setSelectedElement({ kind: 'node', id: coordinateId(selectedCoordinate.coordinate) })
    else setSelectedElement(current => current?.kind === 'node' ? null : current)
  }, [selectedCoordinate?.file, selectedCoordinate?.coordinate.actual_type, selectedCoordinate?.coordinate.id])
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

  // 字段投影按节点身份缓存：修改一条记录时，其他节点复用字段树及卡片数据。
  const projectNode = useMemo(() => {
    const cache = new WeakMap<GraphNodeView, GraphNodeView>()
    return (node: GraphNodeView) => {
      let projected = cache.get(node)
      if (!projected) {
        projected = { ...node, fields: graphCardFields(node.fields, visibleCardFields, enabledFields) }
        cache.set(node, projected)
      }
      return projected
    }
  }, [visibleCardFields, enabledFields])
  const graph = useMemo(() => ({ ...sourceGraph, nodes: sourceGraph.nodes.map(projectNode) }), [sourceGraph, projectNode])

  // 展开状态只改变卡片内容，不重新执行关系布局。
  const [nodeExpandedMap, setNodeExpandedMap] = useState<Map<string, boolean>>(new Map())
  // Per-node set of expanded sub-row paths
  const [nodeRowExpandedMap, setNodeRowExpandedMap] = useState<Map<string, Set<string>>>(new Map())
  // 仅为选中的关系生成端口和强制展开路径，其他字段遵守可见性设置。
  const nodeRelations = useMemo(() => {
    const cache = new WeakMap<GraphNodeView, ReturnType<typeof relationPorts>>()
    return (node: GraphNodeView) => {
      let value = cache.get(node)
      if (!value) { value = relationPorts(node.fields.filter(field => enabledFields.has(field.name))); cache.set(node, value) }
      return value
    }
  }, [enabledFields])
  const relations = useMemo(() => new Map(sourceGraph.nodes.map(node => [node.id, nodeRelations(node)])), [sourceGraph.nodes, nodeRelations])
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
  const layoutQueue = useMemo(() => new LatestTaskQueue<GraphLayoutResult>(), [])
  useEffect(() => () => layoutQueue.cancel(), [layoutQueue, viewKey])
  const savePositionsRef = useRef(onSavePositions)
  savePositionsRef.current = onSavePositions
  const dragStartPositions = useRef(new Map<string, { x: number; y: number }>())
  const fitted = useRef(false)
  const viewportRef = useRef({ x: 0, y: 0, zoom: 1 })
  const [drawingViewport, setDrawingViewport] = useState(viewportRef.current)
  const [drawingSize, setDrawingSize] = useState({ width: window.innerWidth, height: window.innerHeight })
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

  function scheduleLayout(saved?: ReadonlyMap<string, { x: number; y: number }>, heights?: ReadonlyMap<string, number>) {
    return layoutQueue.schedule(current => layoutGraph(graph, enabledFields, activeType, expandedNodes, expandedRows,
      async input => {
        if (!current()) throw new SupersededTask()
        const result = await runGraphLayoutInWorker(input)
        if (!current()) throw new SupersededTask()
        return result
      }, saved, heights))
  }

  async function relayout() {
    const epoch = ++layoutEpoch.current
    const previous = new Map(retainedPositions.current)
    setLayoutBusy(true)
    setLayoutError(null)
    try {
      const next = await scheduleLayout()
      if (!next || epoch !== layoutEpoch.current) return
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
    scheduleLayout(
      retainedPositions.current,
      new Map(reactFlowRef.current?.getNodes().flatMap(node => node.measured?.height
        ? [[node.id, node.measured.height] as const] : []) ?? []),
    )
      .then(next => {
        if (next && !cancelled && epoch === layoutEpoch.current) {
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
        if (!cancelled && epoch === layoutEpoch.current) {
          setLayoutBusy(false)
          setLayoutError(err instanceof Error ? err.message : String(err))
        }
      })
    return () => {
      cancelled = true
      layoutQueue.cancel()
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
  // 缩略/完整模式完全由用户手动切换并持久化，不再随缩放自动切换。
  const compactNodes = savedCompact ?? true
  const toggleCompactNodes = useCallback(() => {
    onSaveCompact?.(!compactNodes)
  }, [onSaveCompact, compactNodes])

  // Group outgoing edge paths by source node id (used to render per-path handles).
  const outgoingPathsByNode = useMemo(() => {
    const m = new Map<string, Set<string>>()
    for (const [id, relation] of relations) m.set(id, new Set(relation.ports.map(port => port.id)))
    for (const e of [...forwardEdges, ...backEdges]) {
      const list = m.get(e.source) ?? new Set<string>()
      list.add(e.field_path)
      m.set(e.source, list)
    }
    return new Map([...m].map(([id, paths]) => [id, [...paths]]))
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
    interaction: (active: boolean) => void
  }>())

  const geometryCache = useRef(new Map<string, { fields: GraphNodeView['fields']; key: string; geometry: GraphRowGeometry }>())
  const geometryView = useRef(viewKey)
  if (geometryView.current !== viewKey) {
    geometryCache.current.clear()
    nodeActions.current.clear()
    geometryView.current = viewKey
  }
  const nodeDescriptions: Node[] = useMemo(
    () => (
      visibleNodes.map(n => {
        const actionKey = JSON.stringify([n.file_path, n.id])
        let actions = nodeActions.current.get(actionKey)
        if (!actions) {
          actions = {
            expand: () => toggleNodeExpanded(n.id),
            row: (path, expanded) => handleRowToggle(n.id, path, expanded),
            edit: (path, value) => nodeActionPort.current.onWriteField?.(n.file_path, n.coordinate, path, value),
            collection: (path, edit) => {
              if (edit.kind === 'array_remove' || edit.kind === 'array_move') geometryCache.current.get(n.id)?.geometry.resetCollection(JSON.stringify(path))
              return nodeActionPort.current.onCollectionEdit?.(n.file_path, n.coordinate, path, edit)
            },
            open: () => nodeActionPort.current.onOpenRecord(n.file_path, n.coordinate),
            diagnostic: () => nodeActionPort.current.onDiagnosticBadgeClick?.(n.file_path, n.coordinate, null),
            interaction: active => setActiveEditorNodes(previous => {
              if (previous.has(n.id) === active) return previous
              const next = new Set(previous)
              if (active) next.add(n.id)
              else next.delete(n.id)
              return next
            }),
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
          const port = relations.get(n.id)?.byId.get(path)
          return !port?.append || (editable && !!onCollectionEdit && !port.readOnly)
        })
        const expanded = expandedNodes.get(n.id) ?? false
        const expansionKey = rowExpanded ? [...rowExpanded].sort().join('|') : ''
        const geometryKey = JSON.stringify([expanded, expansionKey, editable, !!onCollectionEdit, n.is_collapsed])
        const oldGeometry = geometryCache.current.get(n.id)
        const geometry = oldGeometry?.fields === n.fields && oldGeometry.key === geometryKey ? oldGeometry.geometry
          : new GraphRowGeometry(expanded ? n.fields : n.fields.slice(0, 5), rowExpanded ?? new Set(),
            onCollectionEdit ? relations.get(n.id)?.appendPaths ?? new Set() : new Set(), editable, oldGeometry?.geometry, { collapsed: n.is_collapsed, more: !expanded && !relations.get(n.id)?.ports.length && n.fields.length > 5 })
        geometryCache.current.set(n.id, { fields: n.fields, key: geometryKey, geometry })
        return {
          id: n.id,
          selected: selectedNodeIds.has(n.id),
          // 节点代表记录，Delete 只应删除连线，不能把节点从画布移除。
          deletable: false,
          type: 'cfd',
          initialWidth: 280,
          initialHeight: geometry.height,
          // 尚未进入屏幕的节点也提供端口几何，使跨越视口的连线可以绘制。
          handles: geometryHandles(geometry, outgoingPaths),
          position: retainedPositions.current.get(n.id) ?? positions.get(n.id) ?? { x: 0, y: 0 },
          data: {
            graphNode: n,
            expanded: expandedNodes.get(n.id) ?? false,
            relation: relations.get(n.id) ?? relationPorts([]),
            geometry,
            outgoingPaths,
            compact: compactNodes,
            rowExpandKey: rowExpanded ? Array.from(rowExpanded).sort().join('|') : '',
            expandedPaths: rowExpanded ?? new Set<string>(),
            onToggleExpand: actions.expand,
            onRowToggle: actions.row,
            onInteraction: actions.interaction,
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
  useEffect(() => {
    // 查询移除节点后及时释放旧字段树和动作闭包，缓存只覆盖当前逻辑图。
    const ids = new Set(visibleNodes.map(node => node.id))
    const actions = new Set(visibleNodes.map(node => JSON.stringify([node.file_path, node.id])))
    for (const id of geometryCache.current.keys()) if (!ids.has(id)) geometryCache.current.delete(id)
    for (const key of nodeActions.current.keys()) if (!actions.has(key)) nodeActions.current.delete(key)
    setActiveEditorNodes(previous => [...previous].every(id => ids.has(id)) ? previous : new Set([...previous].filter(id => ids.has(id))))
  }, [visibleNodes])
  const [rfNodes, setRfNodes] = useState<Node[]>([])
  const rfNodesRef = useRef<Node[]>([])
  rfNodesRef.current = rfNodes
  // 当前视口对应的 flow 坐标范围；对齐吸附只与屏幕内节点进行。
  const visibleFlowRef = useRef<{ left: number; top: number; right: number; bottom: number } | null>(null)
  // 拖动时显示的对齐参考线（flow 坐标）。
  const [helperLines, setHelperLines] = useState<HelperLines>(NO_HELPER_LINES)
  useLayoutEffect(() => {
    setRfNodes(current => reconcileFlowNodes(current, nodeDescriptions))
  }, [nodeDescriptions])
  const onNodesChange = useCallback((changes: NodeChange[]) => {
    // 拖动帧只修改变化节点并保留 measured；同时按附近节点做对齐吸附。
    type PositionChange = Extract<NodeChange, { type: 'position' }>
    const nodes = rfNodesRef.current
    const positionChanges = changes.filter(
      (change): change is PositionChange => change.type === 'position' && !!change.position,
    )
    let adjusted = changes
    if (positionChanges.length > 0) {
      // 以首个变化节点的提议位置构造吸附对象；其余节点跟随同一位移，保持整组相对关系。
      const movingIds = new Set(positionChanges.map(change => change.id))
      const primary = positionChanges[0]
      const primaryNode = nodes.find(node => node.id === primary.id)
      const width = primaryNode?.measured?.width ?? 280
      const height = primaryNode?.measured?.height ?? 160
      const moving = {
        left: primary.position!.x,
        right: primary.position!.x + width,
        top: primary.position!.y,
        bottom: primary.position!.y + height,
        width,
        height,
      }
      const viewport = visibleFlowRef.current
      const others = nodes
        .filter(node => !movingIds.has(node.id))
        .map(boundsOf)
        .filter(bounds => !viewport
          || (bounds.right >= viewport.left && bounds.left <= viewport.right
            && bounds.bottom >= viewport.top && bounds.top <= viewport.bottom))
      const { snapPosition, horizontal, vertical } = getHelperLines(moving, others, ALIGN_THRESHOLD)
      const dx = snapPosition.x === undefined ? 0 : snapPosition.x - moving.left
      const dy = snapPosition.y === undefined ? 0 : snapPosition.y - moving.top
      if (dx !== 0 || dy !== 0) {
        adjusted = changes.map(change => change.type === 'position' && change.position
          ? { ...change, position: { x: change.position.x + dx, y: change.position.y + dy } }
          : change)
      }
      // 拖动结束后用同一套吸附回写最终位置，避免松手瞬间跳回未吸附的位置。
      const dragEnding = positionChanges.some(change => change.dragging === false)
      const nextLines: HelperLines = dragEnding || (horizontal === undefined && vertical === undefined)
        ? NO_HELPER_LINES
        : { horizontal, vertical }
      setHelperLines(prev => sameHelperLines(prev, nextLines) ? prev : nextLines)
    }
    setRfNodes(current => {
      for (const change of adjusted) {
        if (change.type === 'position' && change.position) retainedPositions.current.set(change.id, change.position)
      }
      return applyNodeChanges(adjusted, current)
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
    return (handle?.startsWith('path-') ? relations.get(nodeId)?.byId.get(handle.slice(5)) : undefined)
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
    const port = relations.get(nodeId)?.byId.get(portId)
    if (!node || !port || !editablePort(nodeId, port)) return
    try {
      // 使用现有写入通道，统一执行校验、配置落盘和撤销历史记录。
      const value = relationValue(port, target.coordinate.id)
      if (port.append) await onCollectionEdit!(node.file_path, node.coordinate, port.path, { kind: 'array_append', value })
      else await onWriteField!(node.file_path, node.coordinate, port.path, value)
      setPicker(null)
    } catch (error) {
      setConnectionError(error instanceof Error ? error.message : String(error))
    }
  }
  function compatibleTarget(nodeId: string, targets: RefTarget[]) {
    const node = currentNodeById.get(nodeId)
    return targets.find(target => target.coordinate.actual_type === node?.actual_type && target.coordinate.id === node?.record_id)
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
      const port = relations.get(graphEdge.source)?.byId.get(graphEdge.field_path)
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
    // 在任何异步查询之前取走重连状态，结束事件随后会清理引用。
    const reconnecting = reconnectEdgeRef.current
    reconnectEdgeRef.current = null
    dragSource.current = null
    if (!source || connectionDone.current) return
    const version = dragVersion.current
    const port = relations.get(source.nodeId)?.byId.get(source.portId)
    if (!port) return
    const point = 'changedTouches' in event ? event.changedTouches[0] : event
    if (!point) return
    const targetId = document.elementFromPoint(point.clientX, point.clientY)?.closest<HTMLElement>('.graph-node')?.dataset.nodeid
    if (!targetId && reconnecting) {
      onEdgesDelete([reconnecting])
      return
    }
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
    setHelperLines(NO_HELPER_LINES)
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
          style: { stroke: 'var(--graph-edge)', strokeWidth: 3.5 },
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
        style: { stroke: 'var(--graph-back-edge)', strokeWidth: 3.5, strokeDasharray: '6 3' },
        zIndex: 1,
        labelStyle: { fill: 'var(--graph-back-edge)', fontSize: 10, fontFamily: 'JetBrains Mono, monospace' },
        labelBgStyle: { fill: 'var(--graph-edge-label-bg)', fillOpacity: 0.92 },
        labelBgPadding: [4, 2] as [number, number],
        labelBgBorderRadius: 3,
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

  // 画布裁剪依据真实端口折线，不能用超高节点的外框代替连线的可见性。
  const edgeGeometry = useMemo(() => {
    const nodes = new Map(rfNodes.map(node => [node.id, node]))
    return rfEdges.map(edge => {
      const source = nodes.get(edge.source), target = nodes.get(edge.target)
      if (!source || !target) return { edge, points: [] }
      const sourceGeometry = (source.data as NodeData).geometry
      const targetGeometry = (target.data as NodeData).geometry
      const from = { x: source.position.x + 280, y: source.position.y + sourceGeometry.center(edge.sourceHandle?.slice(5) ?? '') }
      const to = { x: target.position.x, y: target.position.y + targetGeometry.borderTop + targetGeometry.headerHeight / 2 }
      return { edge, points: [from, { x: from.x + 28, y: from.y }, { x: to.x - 28, y: to.y }, to] }
    })
  }, [rfEdges, rfNodes])
  const drawnEdges = useMemo(() => {
    // 圆角偏离原折线最多 14px，另保留命中宽度和屏幕 overscan；跨屏线段同样命中。
    const margin = 36 / drawingViewport.zoom + 14
    const rect = {
      left: -drawingViewport.x / drawingViewport.zoom - margin,
      right: (drawingSize.width - drawingViewport.x) / drawingViewport.zoom + margin,
      top: -drawingViewport.y / drawingViewport.zoom - margin,
      bottom: (drawingSize.height - drawingViewport.y) / drawingViewport.zoom + margin,
    }
    return edgeGeometry.filter(({ edge, points }) => edge.selected || edge.id === reconnectEdgeRef.current?.id || polylineIntersectsRect(points, rect)).map(({ edge }) => edge)
  }, [edgeGeometry, drawingViewport, drawingSize])

  // ── Imperative hover highlight (zero re-renders) ────────────────────────
  // We manipulate DOM classes directly to avoid the state→rerender→mouseleave
  // flicker cycle. The adjacency map is rebuilt whenever edges change.
  const wrapRef = useRef<HTMLDivElement>(null)
  const [geometryStyleVersion, setGeometryStyleVersion] = useState(0)
  useEffect(() => {
    const invalidate = () => {
      const models = new Set(rfNodesRef.current.map(node => (node.data as NodeData).geometry))
      for (const model of models) model.invalidateMeasurements()
      setGeometryStyleVersion(value => value + 1)
    }
    document.fonts.addEventListener('loadingdone', invalidate)
    const observer = new MutationObserver(invalidate)
    const targets = new Set([document.documentElement, document.body, wrapRef.current?.closest('.app, [data-theme]')].filter((target): target is HTMLElement => target instanceof HTMLElement))
    for (const target of targets) observer.observe(target, { attributes: true, attributeFilter: ['data-theme', 'class', 'style'] })
    return () => { document.fonts.removeEventListener('loadingdone', invalidate); observer.disconnect() }
  }, [])
  useLayoutEffect(() => {
    const root = wrapRef.current
    if (!root) return
    const observer = new ResizeObserver(() => setDrawingSize(previous => previous.width === root.clientWidth && previous.height === root.clientHeight
      ? previous : { width: root.clientWidth, height: root.clientHeight }))
    observer.observe(root)
    return () => observer.disconnect()
  }, [])

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

  // 只记录视口用于重挂载时的默认视口，不再据此自动切换缩略模式。
  const handleViewportChange = useCallback((viewport: { x: number; y: number; zoom: number }) => {
    viewportRef.current = viewport
    setDrawingViewport(previous => previous.x === viewport.x && previous.y === viewport.y && previous.zoom === viewport.zoom ? previous : viewport)
    // 同步换算屏幕对应的 flow 范围，供拖动对齐时筛选屏幕内节点。
    const container = wrapRef.current
    visibleFlowRef.current = container
      ? {
          left: -viewport.x / viewport.zoom,
          top: -viewport.y / viewport.zoom,
          right: (container.clientWidth - viewport.x) / viewport.zoom,
          bottom: (container.clientHeight - viewport.y) / viewport.zoom,
        }
      : null
  }, [])
  const boxSelectRef = useRef<{ x: number; y: number } | null>(null)
  // 右键框选发生过拖动时，抑制随后落在节点上的 contextmenu，避免误弹节点菜单。
  const suppressNodeContextMenu = useRef(false)
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
      // 新的右键拖动开始时先复位，只有真正拖动过才抑制节点菜单。
      suppressNodeContextMenu.current = false
      boxSelectRef.current = { x: event.clientX, y: event.clientY }
      const move = (moveEvent: PointerEvent) => {
        const start = boxSelectRef.current
        if (!start) return
        if (Math.hypot(moveEvent.clientX - start.x, moveEvent.clientY - start.y) > 4) {
          suppressNodeContextMenu.current = true
        }
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
  // 对齐参考线需要覆盖整张图，用节点包围盒外扩一段作为绘制范围。
  const guideBounds = useMemo(() => {
    if (helperLines.horizontal === undefined && helperLines.vertical === undefined) return null
    const all = unionBounds(rfNodes.map(boundsOf))
    if (!all) return null
    const pad = 240
    return { left: all.left - pad, top: all.top - pad, right: all.right + pad, bottom: all.bottom + pad }
  }, [helperLines, rfNodes])
  // 拖动包围盒即整组移动选中节点，落点按 flow 坐标换算，避免受缩放影响。
  const beginGroupDrag = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || selectedNodeIds.size === 0) return
    event.preventDefault()
    event.stopPropagation()
    const instance = reactFlowRef.current
    if (!instance) return
    // 以当前渲染中的坐标为准，保证刚布局完还没落盘的节点也能整组移动。
    const start = new Map<string, { x: number; y: number }>()
    const startBounds: AlignBounds[] = []
    for (const node of rfNodes) {
      if (!selectedNodeIds.has(node.id)) continue
      start.set(node.id, { ...node.position })
      startBounds.push(boundsOf(node))
    }
    if (start.size === 0) return
    const startFlow = instance.screenToFlowPosition({ x: event.clientX, y: event.clientY })
    const move = (moveEvent: PointerEvent) => {
      const flow = reactFlowRef.current?.screenToFlowPosition({ x: moveEvent.clientX, y: moveEvent.clientY })
      if (!flow) return
      let dx = flow.x - startFlow.x
      let dy = flow.y - startFlow.y
      // 整组按包围盒对齐附近节点，参考线同样随拖动更新。
      const moving = unionBounds(startBounds.map(bounds => ({
        ...bounds, left: bounds.left + dx, right: bounds.right + dx,
        top: bounds.top + dy, bottom: bounds.bottom + dy,
      })))
      if (moving) {
        const viewport = visibleFlowRef.current
        const others = rfNodes
          .filter(node => !start.has(node.id))
          .map(boundsOf)
          .filter(bounds => !viewport
            || (bounds.right >= viewport.left && bounds.left <= viewport.right
              && bounds.bottom >= viewport.top && bounds.top <= viewport.bottom))
        const { snapPosition, horizontal, vertical } = getHelperLines(moving, others, ALIGN_THRESHOLD)
        if (snapPosition.x !== undefined) dx += snapPosition.x - moving.left
        if (snapPosition.y !== undefined) dy += snapPosition.y - moving.top
        const nextLines: HelperLines = horizontal === undefined && vertical === undefined
          ? NO_HELPER_LINES : { horizontal, vertical }
        setHelperLines(prev => sameHelperLines(prev, nextLines) ? prev : nextLines)
      }
      for (const [id, base] of start) retainedPositions.current.set(id, { x: base.x + dx, y: base.y + dy })
      setRfNodes(current => current.map(node => {
        const base = start.get(node.id)
        return base ? { ...node, position: { x: base.x + dx, y: base.y + dy } } : node
      }))
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      setHelperLines(NO_HELPER_LINES)
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
  const existingTargetIds = useMemo(
    () => newTarget
      ? graphData.nodes.filter(node => node.coordinate.actual_type === newTarget.targetType).map(node => node.coordinate.id)
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
            <GraphEdgeViewport left={-drawingViewport.x / drawingViewport.zoom - 50} top={-drawingViewport.y / drawingViewport.zoom - 50}
              width={drawingSize.width / drawingViewport.zoom + 100} height={drawingSize.height / drawingViewport.zoom + 100}>
            <ReactFlow
            nodes={rfNodes}
            defaultViewport={viewportRef.current}
            edges={drawnEdges}
            nodeTypes={nodeTypes}
            edgeTypes={edgeTypes}
            nodesDraggable={!layoutBusy && !positionSaving}
            // 活动编辑或拖线期间保留节点壳；内部窗口继续裁剪，避免框架卸载屏外草稿。
            onlyRenderVisibleElements={activeEditorNodes.size === 0 && !connectionActive && !picker && !newTarget}
            onNodeDragStart={(_event, node) => {
              dragStartPositions.current = new Map(retainedPositions.current)
              setSelectedElement({ kind: 'node', id: node.id })
            }}
            onNodeDragStop={() => {
              setHelperLines(NO_HELPER_LINES)
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
              setConnectionActive(true)
              if (reconnectStartRef.current) {
                reconnectStartRef.current = false
                return
              }
              beginConnection(params.nodeId, params.handleId)
            }}
            onConnect={connect}
            onEdgesDelete={onEdgesDelete}
            onConnectEnd={event => { setConnectionActive(false); void endConnection(event) }}
            edgesReconnectable
            edgesFocusable
            elementsSelectable
            deleteKeyCode={["Backspace", "Delete"]}
            onReconnectStart={(_event, edge) => {
              setConnectionActive(true)
              reconnectEdgeRef.current = edge
              reconnectStartRef.current = true
              beginConnection(edge.source, edge.sourceHandle ?? null)
            }}
            onReconnect={(_edge, connection) => connect(connection)}
            onReconnectEnd={event => {
              setConnectionActive(false)
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
              // 右键框选拖动结束后落点若在节点上，不得弹出节点菜单。
              if (suppressNodeContextMenu.current) return
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
            connectionLineType={ConnectionLineType.Straight}
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
            <GraphGeometryProbeHost key={geometryStyleVersion} nodes={rfNodes} />
            <Background color="var(--graph-bg-grid)" gap={24} size={1} />
            {/* 左下角只保留自动布局、回正、缩略/完整模式切换三个按钮。 */}
            <Controls showZoom={false} showFitView={false} showInteractive={false}>
              <ControlButton onClick={() => { void relayout() }} disabled={layoutBusy || positionSaving}
                title="自动布局（重置节点位置）" aria-label="自动布局">
                <Icon name="layout" size={16} />
              </ControlButton>
              <ControlButton onClick={fitGraph} title="回正（显示整张图）" aria-label="回正">
                <Icon name="center" size={16} />
              </ControlButton>
              <ControlButton onClick={toggleCompactNodes}
                title={compactNodes ? '切换到完整模式' : '切换到缩略模式'} aria-label="切换显示模式">
                <Icon name={compactNodes ? "card-detail" : "card-compact"} size={16} />
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
            {/* 拖动对齐参考线与选中包围盒：渲染在 viewport 内，随平移缩放自动贴合。 */}
            <ViewportPortal>
              {guideBounds && helperLines.vertical !== undefined && (
                <div
                  className="graph-align-guide graph-align-guide-v"
                  style={{ left: helperLines.vertical, top: guideBounds.top, height: guideBounds.bottom - guideBounds.top }}
                />
              )}
              {guideBounds && helperLines.horizontal !== undefined && (
                <div
                  className="graph-align-guide graph-align-guide-h"
                  style={{ top: helperLines.horizontal, left: guideBounds.left, width: guideBounds.right - guideBounds.left }}
                />
              )}
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
            </GraphEdgeViewport>
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
              ...(picker.flowPosition && onCreateRecordDraft && onCreateLinkedRecord
                ? [{ value: NEW_TARGET_OPTION, label: '＋ 新建节点', description: `新建 ${picker.targetType} 并连接` }]
                : []),
              ...picker.targets.map((target, index) => ({ value: String(index), label: shortNameLabel(target.coordinate.id, target.short_name),
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
      {newTarget && onCreateRecordDraft && onCreateLinkedRecord && (
        <CreateRecordDialog
          actualType={newTarget.targetType}
          typeOptions={[]}
          existingIds={existingTargetIds}
          onCreateRecordDraft={onCreateRecordDraft}
          onInsertRecord={async (recordId, actualType, fields) => {
            const node = currentNodeById.get(newTarget.nodeId)
            const port = relations.get(newTarget.nodeId)?.byId.get(newTarget.portId)
            if (!node || !port || !onCreateLinkedRecord) throw new Error('引用来源已变化，请重新选择连线起点')
            await onCreateLinkedRecord({ file: filePath, id: recordId, actual_type: actualType, fields,
              source_file: node.file_path, source: node.coordinate, field_path: port.path,
              reference: relationValue(port, recordId), append: !!port.append })
            retainedPositions.current.set(coordinateId({ actual_type: actualType, id: recordId }), newTarget.flowPosition)
            setNewTarget(null)
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
            primaryId: nodeMenu.coordinate.id,
          }}
          groups={recordGroups}
          showOpenRecord
          canRename={!!onRenameRecord && (fileCapabilities?.[nodeMenu.filePath]?.can_edit_id ?? false)}
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
          title="重命名 ID"
          message="输入新的记录 ID"
          initialValue={nodeMenuAction.coordinate.id}
          confirmLabel="重命名"
          onClose={() => setNodeMenuAction(null)}
          onConfirm={async next => {
            if (next !== nodeMenuAction.coordinate.id) {
              await onRenameRecord(nodeMenuAction.filePath, nodeMenuAction.coordinate, next)
            }
            setNodeMenuAction(null)
          }}
        />
      )}
      {nodeMenuAction?.kind === 'delete' && onDeleteRecord && (
        <ConfirmDialog
          title="删除记录"
          message={`确认删除记录 ${nodeMenuAction.coordinate.id}？此操作不可撤销。`}
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
