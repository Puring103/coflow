import { expect, it } from 'vitest'
import type { Edge, Node } from '@xyflow/react'
import { GraphRowGeometry } from './rowGeometry'
import { GraphEdgeGeometry } from './edgeGeometry'

it('reuses edge coordinates for handle window updates and follows row height and position changes', () => {
  const geometry = new GraphRowGeometry([{ name: 'link', value: { kind: 'ref', value: 'target' }, missing: false,
    annotation: { declared_type: '&Item', ref_target_type: 'Item' } }], new Set(), new Set(), true)
  const source: Node = { id: 'source', position: { x: 0, y: 0 }, data: { geometry } }
  const target: Node = { id: 'target', position: { x: 500, y: 0 }, data: { geometry } }
  const edges: Edge[] = [{ id: 'edge', source: source.id, target: target.id, sourceHandle: 'path-link' }]
  const cache = new GraphEdgeGeometry()
  const first = cache.project([source, target], edges)
  expect(cache.project([{ ...source, handles: [] }, target], edges)).toBe(first)
  geometry.measure(new Map([[geometry.rows[0].key, 100]]))
  const measured = cache.project([source, target], edges)
  expect(measured[0].points[0].y).toBeGreaterThan(first[0].points[0].y)
  const moved = cache.project([source, { ...target, position: { x: 700, y: 100 } }], edges)
  expect(moved[0].points[3].x).toBe(700)
  expect(moved[0].points[3].y).toBe(measured[0].points[3].y + 100)
  expect(cache.project([source], edges)[0].points).toEqual([])
})
