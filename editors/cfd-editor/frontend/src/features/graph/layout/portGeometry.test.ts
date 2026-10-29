import { describe, expect, it } from 'vitest'
import type { Edge, InternalNode, Node } from '@xyflow/react'
import { GraphRowGeometry } from './rowGeometry'
import { geometryHandles, geometryIsPublished, GraphPortWindow } from './portGeometry'

function fixture(count = 10000): Node {
  const geometry = new GraphRowGeometry([{ name: 'links', missing: false,
    value: { kind: 'array', value: Array.from({ length: count }, () => ({ kind: 'ref', value: 'same' })) },
    annotation: { declared_type: '[&Item]', item_annotation: { declared_type: '&Item', ref_target_type: 'Item' } },
  }], new Set(['links']), new Set(), true)
  return { id: 'source', position: { x: 100, y: 100 },
    data: { geometry, outgoingPaths: Array.from({ length: count }, (_, index) => `links[${index}]`) } }
}
const viewport = { x: 0, y: 0, zoom: 1 }

describe('graph port window', () => {
  it('skips unchanged publication while detecting dropped offscreen bounds and stale measured heights', () => {
    const geometry = fixture().data.geometry as GraphRowGeometry
    const handles = geometryHandles(geometry, ['links[0]', 'links[9000]'])
    const node = { width: 280, height: geometry.height, measured: { width: 280, height: geometry.height },
      internals: { handleBounds: { source: handles.filter(handle => handle.type === 'source'), target: [handles[0]] } } } as InternalNode
    expect(geometryIsPublished(node, geometry, handles)).toBe(true)
    node.internals.handleBounds!.source!.pop()
    expect(geometryIsPublished(node, geometry, handles)).toBe(false)
    node.internals.handleBounds!.source = handles.filter(handle => handle.type === 'source') as NonNullable<InternalNode['internals']['handleBounds']>['source']
    node.measured.height = 100
    expect(geometryIsPublished(node, geometry, handles)).toBe(false)
  })
  it('bounds framework handles while preserving complete geometry and offscreen edge/drag sources', () => {
    const node = fixture(), geometry = node.data.geometry as GraphRowGeometry
    const edges: Edge[] = [{ id: 'crossing', source: 'source', sourceHandle: 'path-links[9000]', target: 'target' }]
    const result = new GraphPortWindow().project([node], edges, viewport, 900, { nodeId: 'source', portId: 'path-links[9999]' })
    expect(result.nodes[0].handles!.length).toBeLessThan(60)
    expect(result.paths.get('source')).toContain('links[9000]')
    expect(result.paths.get('source')).toContain('links[9999]')
    expect(result.nodes[0].data).toBe(node.data)
    expect(geometry.rows).toHaveLength(10001)
    expect(result.nodes[0].handles!.find(handle => handle.id === 'path-links[9000]')!.y).toBe(geometry.center('links[9000]') - 8)
  })

  it('reuses a stable window and recalculates coordinates after row height changes', () => {
    const node = fixture(), window = new GraphPortWindow()
    const first = window.project([node], [], viewport, 900)
    const same = window.project([node], [], viewport, 900)
    expect(same.nodes).toBe(first.nodes)
    expect(same.paths).toBe(first.paths)
    const geometry = node.data.geometry as GraphRowGeometry
    const before = first.nodes[0].handles!.find(handle => handle.id === 'path-links[1]')!.y
    geometry.measure(new Map([[geometry.rows[1].key, 80]]))
    const measured = window.project([node], [], viewport, 900)
    expect(measured.nodes[0].handles!.find(handle => handle.id === 'path-links[1]')!.y).toBeGreaterThan(before)
    expect(window.project([], [], viewport, 900).paths.size).toBe(0)
    const restored = window.project([node], [], viewport, 900)
    expect(restored.nodes[0]).not.toBe(measured.nodes[0])
  })
})
