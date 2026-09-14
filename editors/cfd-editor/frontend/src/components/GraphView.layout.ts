import type { ElkNode } from 'elkjs/lib/elk-api'
import type { GraphEdgeView, GraphNodeView } from '../wire'
import { topLevelFieldName } from '../wire/paths'
import {
  COLUMN_GAP,
  COMPACT_ZOOM_THRESHOLD,
  COMPONENT_GAP,
  EDITABLE_ROW_HEIGHT,
  HEADER_HEIGHT,
  MORE_BUTTON_HEIGHT,
  NODE_WIDTH,
  ROW_GAP,
  ROW_HEIGHT,
  VERTICAL_PADDING,
  type Position,
} from '../graph/metrics'
import type { GraphLayoutRunner } from '../graph/layoutRunner'
import { bfsReachable, connectedComponents as topologyComponents, detectBackEdges as topologyBackEdges } from '../graph/topology'
import { retainGraphPositions as retainPositionsImpl } from '../graph/retainPositions'
import { NODE_PEEK_FIELDS, countVisibleRows } from './DataCard.geometry'
import { relationPorts } from './GraphView.relations'

export type { Position }
export type { GraphLayoutRunner } from '../graph/layoutRunner'

export interface GraphLayoutResult {
  positions: Map<string, Position>
  visibleNodes: GraphNodeView[]
  forwardEdges: GraphEdgeView[]
  backEdges: GraphEdgeView[]
}

export function estimateNodeHeight(
  node: GraphNodeView,
  expanded: boolean,
  expandedRows: ReadonlySet<string>,
): number {
  if (node.is_collapsed) return HEADER_HEIGHT + 28 + VERTICAL_PADDING
  if (!expanded) {
    const visible = Math.min(NODE_PEEK_FIELDS, node.fields.length)
    const hasMore = node.fields.length > NODE_PEEK_FIELDS
    return HEADER_HEIGHT
      + visible * ROW_HEIGHT
      + (hasMore ? MORE_BUTTON_HEIGHT : 0)
      + VERTICAL_PADDING
  }
  const relations = relationPorts(node.fields).ports
  const rows = countVisibleRows(node.fields, expandedRows) + relations.filter(port => port.append && !port.readOnly).length
  const hasMore = relations.length === 0 && node.fields.length > NODE_PEEK_FIELDS
  return HEADER_HEIGHT
    + rows * (relations.length > 0 ? EDITABLE_ROW_HEIGHT : ROW_HEIGHT)
    + (hasMore ? MORE_BUTTON_HEIGHT : 0)
    + VERTICAL_PADDING
}

export function estimateHandleOffsets(
  node: GraphNodeView,
  outgoingPaths: string[],
  expanded: boolean,
): { headerCenterY: number; pathOffsets: Map<string, number> } {
  const headerCenterY = HEADER_HEIGHT / 2
  const pathOffsets = new Map<string, number>()
  for (const path of outgoingPaths) {
    pathOffsets.set(
      path,
      estimateTopLevelRowCenter(node, topLevelField(path), expanded) ?? headerCenterY,
    )
  }
  return { headerCenterY, pathOffsets }
}

export function sameOffsetMap(
  left: ReadonlyMap<string, number>,
  right: ReadonlyMap<string, number>,
): boolean {
  if (left.size !== right.size) return false
  for (const [key, value] of right) if (left.get(key) !== value) return false
  return true
}

export function isCompactGraphZoom(zoom: number): boolean {
  return zoom < COMPACT_ZOOM_THRESHOLD
}

export function topLevelField(path: string): string {
  return topLevelFieldName(path)
}

export function defaultEnabledFields(
  graph: { nodes: GraphNodeView[]; edges: GraphEdgeView[] },
  availableFields: string[],
  activeType: string | undefined,
): string[] {
  if (!activeType) return availableFields
  const nodeById = new Map(graph.nodes.map(node => [node.id, node]))
  const fields = new Set<string>()
  for (const edge of graph.edges) {
    const source = nodeById.get(edge.source)
    if (source?.actual_type === activeType) {
      fields.add(topLevelField(edge.field_path))
    }
  }
  return availableFields.filter(field => fields.has(field))
}

export function graphEdgeId(
  kind: 'fwd' | 'back',
  edge: { source: string; target: string; field_path: string },
): string {
  return `${kind}:${edge.source}->${edge.target}:${encodeURIComponent(edge.field_path)}`
}

export function graphTopologySignature(graph: {
  nodes: GraphNodeView[]
  edges: GraphEdgeView[]
}): string {
  const nodes = graph.nodes
    .map(node => [
      node.id,
      node.actual_type,
      node.file_path,
      node.in_focus_file ? '1' : '0',
      node.is_collapsed ? '1' : '0',
    ].join(':'))
    .sort()
  const edges = graph.edges
    .map(edge => `${edge.source}>${edge.target}:${edge.field_path}`)
    .sort()
  return `${nodes.join('|')}\u001e${edges.join('|')}`
}

export async function layoutGraph(
  graph: { nodes: GraphNodeView[]; edges: GraphEdgeView[] },
  enabledFields: ReadonlySet<string>,
  activeType: string | undefined,
  nodeExpanded: ReadonlyMap<string, boolean>,
  rowExpanded: ReadonlyMap<string, ReadonlySet<string>>,
  runLayout: GraphLayoutRunner,
  savedPositions?: ReadonlyMap<string, Position>,
  measuredHeights?: ReadonlyMap<string, number>,
): Promise<GraphLayoutResult> {
  let activeEdges = graph.edges.filter(edge => enabledFields.has(topLevelField(edge.field_path)))
  const touched = new Set<string>()
  for (const edge of activeEdges) {
    touched.add(edge.source)
    touched.add(edge.target)
  }

  let visibleIds: Set<string>
  if (activeType) {
    const roots = graph.nodes
      .filter(node => node.in_focus_file && node.actual_type === activeType && touched.has(node.id))
      .map(node => node.id)
    visibleIds = reachableNodes(roots, activeEdges)
    activeEdges = activeEdges.filter(
      edge => visibleIds.has(edge.source) && visibleIds.has(edge.target),
    )
  } else {
    visibleIds = touched
  }

  const visibleNodes = graph.nodes.filter(node => visibleIds.has(node.id))
  const backEdgeKeys = detectBackEdges(visibleNodes, activeEdges)
  const forwardEdges = activeEdges.filter(
    edge => !backEdgeKeys.has(backEdgeKey(edge.source, edge.target)),
  )
  const backEdges = activeEdges.filter(
    edge => backEdgeKeys.has(backEdgeKey(edge.source, edge.target)),
  )
  // 已保存的视图只补齐新节点，关系变化不调用布局引擎。
  if (savedPositions?.size) {
    const result = { visibleNodes, forwardEdges, backEdges,
      positions: new Map(visibleNodes.map(node => [node.id, { x: 0, y: 0 }])) }
    result.positions = retainGraphPositions(result, savedPositions, nodeExpanded, rowExpanded, measuredHeights)
    return result
  }
  const nodeById = new Map(visibleNodes.map(node => [node.id, node]))
  const forcedRoots = sameTypeRoots(visibleNodes, forwardEdges, nodeById, activeType)
  const components = connectedComponents(visibleNodes.map(node => node.id), activeEdges)
  const nodeToComponent = new Map<string, number>()
  components.forEach((component, index) => {
    component.forEach(id => nodeToComponent.set(id, index))
  })
  components.sort((left, right) => {
    if (right.length !== left.length) return right.length - left.length
    return left[0].localeCompare(right[0])
  })

  const positions = new Map<string, Position>()
  let yOffset = 0
  for (const component of components) {
    const componentNodes = component
      .map(id => nodeById.get(id))
      .filter((node): node is GraphNodeView => node !== undefined)
    const componentId = nodeToComponent.get(componentNodes[0]?.id ?? '')
    const componentEdges = forwardEdges.filter(
      edge => visibleIds.has(edge.source)
        && visibleIds.has(edge.target)
        && nodeToComponent.get(edge.source) === componentId,
    )
    const componentRoots = new Set(component.filter(id => forcedRoots.has(id)))
    const localPositions = await layoutComponent(
      componentNodes,
      componentEdges,
      componentRoots,
      nodeExpanded,
      rowExpanded,
      runLayout,
    )

    let minY = Infinity
    let maxY = -Infinity
    for (const [id, { y }] of localPositions) {
      const node = nodeById.get(id)
      const height = node ? nodeHeight(node, nodeExpanded, rowExpanded) : 0
      minY = Math.min(minY, y)
      maxY = Math.max(maxY, y + height)
    }
    const shift = yOffset - minY
    for (const [id, position] of localPositions) {
      positions.set(id, { x: position.x, y: position.y + shift })
    }
    yOffset += maxY - minY + COMPONENT_GAP
  }

  return { positions, visibleNodes, forwardEdges, backEdges }
}

function estimateTopLevelRowCenter(
  node: GraphNodeView,
  fieldName: string,
  expanded: boolean,
): number | null {
  if (node.is_collapsed) return null
  const maxRows = expanded ? node.fields.length : Math.min(NODE_PEEK_FIELDS, node.fields.length)
  const index = node.fields.slice(0, maxRows).findIndex(field => field.name === fieldName)
  return index === -1 ? null : HEADER_HEIGHT + index * ROW_HEIGHT + ROW_HEIGHT / 2
}

function reachableNodes(roots: string[], edges: GraphEdgeView[]): Set<string> {
  return bfsReachable(roots, edges)
}

function sameTypeRoots(
  nodes: GraphNodeView[],
  edges: GraphEdgeView[],
  nodeById: ReadonlyMap<string, GraphNodeView>,
  activeType: string | undefined,
): Set<string> {
  const roots = new Set<string>()
  if (!activeType) return roots
  const targets = new Set<string>()
  for (const edge of edges) {
    const source = nodeById.get(edge.source)
    const target = nodeById.get(edge.target)
    if (
      source?.in_focus_file
      && target?.in_focus_file
      && source.actual_type === activeType
      && target.actual_type === activeType
    ) {
      targets.add(target.id)
    }
  }
  for (const node of nodes) {
    if (node.in_focus_file && node.actual_type === activeType && !targets.has(node.id)) {
      roots.add(node.id)
    }
  }
  return roots
}

function connectedComponents(
  nodeIds: string[],
  edges: { source: string; target: string }[],
): string[][] {
  return topologyComponents(nodeIds, edges)
}

function detectBackEdges(
  nodes: { id: string }[],
  edges: { source: string; target: string }[],
): Set<string> {
  return topologyBackEdges(nodes, edges)
}

function backEdgeKey(source: string, target: string): string {
  return `${source}\u2192${target}`
}

function sortedGraphNodes(nodes: GraphNodeView[]): GraphNodeView[] {
  return [...nodes].sort((left, right) => {
    if (left.in_focus_file !== right.in_focus_file) return left.in_focus_file ? -1 : 1
    if (left.file_path !== right.file_path) return left.file_path.localeCompare(right.file_path)
    if (left.key !== right.key) return left.key.localeCompare(right.key)
    return left.id.localeCompare(right.id)
  })
}

function sourcePortId(nodeId: string, fieldPath: string): string {
  return `${nodeId}:out:${encodeURIComponent(fieldPath)}`
}

function targetPortId(nodeId: string): string {
  return `${nodeId}:in`
}

function nodeHeight(
  node: GraphNodeView,
  nodeExpanded: ReadonlyMap<string, boolean>,
  rowExpanded: ReadonlyMap<string, ReadonlySet<string>>,
): number {
  // 缩略覆盖层沿用完整卡片尺寸，不额外改变布局高度。
  return estimateNodeHeight(
    node,
    nodeExpanded.get(node.id) ?? false,
    rowExpanded.get(node.id) ?? new Set<string>(),
  )
}

async function layoutComponent(
  nodes: GraphNodeView[],
  edges: GraphEdgeView[],
  forcedRoots: ReadonlySet<string>,
  nodeExpanded: ReadonlyMap<string, boolean>,
  rowExpanded: ReadonlyMap<string, ReadonlySet<string>>,
  runLayout: GraphLayoutRunner,
): Promise<Map<string, Position>> {
  const outgoingPaths = new Map<string, string[]>()
  for (const edge of edges) {
    const paths = outgoingPaths.get(edge.source) ?? []
    if (!paths.includes(edge.field_path)) paths.push(edge.field_path)
    outgoingPaths.set(edge.source, paths)
  }

  const elkGraph: ElkNode = {
    id: 'root',
    layoutOptions: {
      'elk.algorithm': 'layered',
      'elk.direction': 'RIGHT',
      'elk.spacing.nodeNode': `${ROW_GAP}`,
      'elk.layered.spacing.nodeNodeBetweenLayers': `${COLUMN_GAP}`,
      'elk.layered.crossingMinimization.strategy': 'LAYER_SWEEP',
      'elk.layered.nodePlacement.strategy': 'BRANDES_KOEPF',
      // 自动布局统一以上边缘对齐，减少同层节点的锯齿状偏移。
      'elk.layered.nodePlacement.bk.fixedAlignment': 'TOP',
      'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
      'elk.portConstraints': 'FIXED_ORDER',
      'elk.edgeRouting': 'SPLINES',
    },
    children: sortedGraphNodes(nodes).map(node => {
      const paths = [...(outgoingPaths.get(node.id) ?? [])].sort((left, right) => {
        const leftIndex = node.fields.findIndex(field => field.name === topLevelField(left))
        const rightIndex = node.fields.findIndex(field => field.name === topLevelField(right))
        if (leftIndex !== rightIndex) {
          return (leftIndex === -1 ? Number.MAX_SAFE_INTEGER : leftIndex)
            - (rightIndex === -1 ? Number.MAX_SAFE_INTEGER : rightIndex)
        }
        return left.localeCompare(right)
      })
      return {
        id: node.id,
        width: NODE_WIDTH,
        height: nodeHeight(node, nodeExpanded, rowExpanded),
        layoutOptions: {
          ...(forcedRoots.has(node.id)
            ? { 'elk.layered.layering.layerConstraint': 'FIRST' }
            : {}),
          'elk.portConstraints': 'FIXED_ORDER',
        },
        ports: [
          {
            id: targetPortId(node.id),
            width: 1,
            height: 1,
            layoutOptions: { 'elk.port.side': 'WEST', 'elk.port.index': '0' },
          },
          ...paths.map((path, index) => ({
            id: sourcePortId(node.id, path),
            width: 1,
            height: 1,
            layoutOptions: { 'elk.port.side': 'EAST', 'elk.port.index': `${index + 1}` },
          })),
        ],
      }
    }),
    edges: edges.map(edge => ({
      id: graphEdgeId('fwd', edge),
      sources: [sourcePortId(edge.source, edge.field_path)],
      targets: [targetPortId(edge.target)],
    })),
  }

  return runLayout(elkGraph)
}

// 增量刷新只安置新增节点，已存在节点（包括用户拖动的位置）保持不变。
export function retainGraphPositions(
  layout: GraphLayoutResult,
  retained: ReadonlyMap<string, Position>,
  expanded: ReadonlyMap<string, boolean>,
  expandedRows: ReadonlyMap<string, ReadonlySet<string>>,
  measuredHeights: ReadonlyMap<string, number> = new Map(),
): Map<string, Position> {
  const nodes = new Map(layout.visibleNodes.map(node => [node.id, node]))
  // 缺失实测高度时直接用估计值并注明，不在热循环内分支。
  const heightOf = (id: string): number => measuredHeights.get(id)
    ?? (nodes.has(id) ? nodeHeight(nodes.get(id)!, expanded, expandedRows) : HEADER_HEIGHT)
  return retainPositionsImpl(layout, retained, heightOf)
}
