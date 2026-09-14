import type { GraphEdge } from '../bindings/GraphEdge'
import type { GraphNode } from '../bindings/GraphNode'
import { coordinateId } from './ids'

export type GraphNodeView = GraphNode & {
  id: string
  key: string
  actual_type: string
}

export type GraphEdgeView = {
  source: string
  target: string
  field_path: string
  raw: GraphEdge
}

export function graphNodeView(node: GraphNode): GraphNodeView {
  return {
    ...node,
    id: coordinateId(node.coordinate),
    key: node.coordinate.key,
    actual_type: node.coordinate.actual_type,
  }
}

export function graphEdgeView(edge: GraphEdge): GraphEdgeView {
  return {
    source: coordinateId(edge.source),
    target: coordinateId(edge.target),
    field_path: edge.field_path,
    raw: edge,
  }
}

