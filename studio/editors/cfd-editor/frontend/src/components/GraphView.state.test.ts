import { describe, expect, it } from 'vitest'
import type { Node } from '@xyflow/react'
import {
  graphHighlights,
  polylineIntersectsRect,
  reconcileFlowNodes,
  sameGraphValue,
  selectedNodeBounds,
  segmentIntersectsRect,
} from './GraphView.state'

const node = (id: string): Node => ({ id, position: { x: 10, y: 20 },
  data: { value: 12n, expanded: new Set(['items']) }, measured: { width: 280, height: 160 } })

describe('graph incremental state', () => {
  it('preserves unchanged nodes and the entire unchanged array', () => {
    const previous = [node('a'), node('b')]
    expect(reconcileFlowNodes(previous, structuredClone(previous))).toBe(previous)
  })
  it('updates only changed content without losing measured geometry', () => {
    const previous = [node('a'), node('b')]
    const incoming = previous.map(({ measured: _, ...item }) => structuredClone(item))
    incoming[0].data.value = 13n
    const next = reconcileFlowNodes(previous, incoming)
    expect(next[1]).toBe(previous[1])
    expect(next[0].measured).toBe(previous[0].measured)
    expect(next[0].position).toBe(previous[0].position)
  })
  it('retains card data when only coordinates change', () => {
    const previous = [node('a')]
    const next = reconcileFlowNodes(previous, [{ ...node('a'), position: { x: 300, y: 20 } }])
    expect(next[0].data).toBe(previous[0].data)
    expect(next[0].position.x).toBe(300)
  })
  it('compares bigint and expansion sets without serialization', () => {
    expect(sameGraphValue({ value: 12n }, { value: 13n })).toBe(false)
    expect(sameGraphValue(new Set(['a']), new Set(['b']))).toBe(false)
  })
  it('unions hover and selection, including IDs with spaces', () => {
    const edges = [{ id: 'edge one', source: 'node a', target: 'b' }, { id: 'two', source: 'c', target: 'd' }]
    const focus = graphHighlights(edges, { kind: 'edge', id: 'edge one' }, { kind: 'node', id: 'c' })
    expect([...focus.nodes].sort()).toEqual(['b', 'c', 'd', 'node a'])
    expect([...focus.edges]).toEqual(['edge one', 'two'])
    expect(graphHighlights(edges, { kind: 'edge', id: 'edge one' }, null).nodes).toEqual(new Set(['node a', 'b']))
  })
})

describe('graph selection geometry', () => {
  it('computes the bounding box of selected nodes using measured sizes', () => {
    const nodes: Node[] = [
      { id: 'a', position: { x: 0, y: 0 }, measured: { width: 100, height: 40 }, data: {} },
      { id: 'b', position: { x: 200, y: 120 }, measured: { width: 80, height: 60 }, data: {} },
      { id: 'c', position: { x: 999, y: 999 }, measured: { width: 10, height: 10 }, data: {} },
    ]
    expect(selectedNodeBounds(nodes, new Set(['a', 'b']), { width: 280, height: 160 }))
      .toEqual({ left: 0, top: 0, right: 280, bottom: 180 })
    expect(selectedNodeBounds(nodes, new Set(), { width: 280, height: 160 })).toBeNull()
  })

  it('falls back to the default node size before measurement lands', () => {
    const nodes: Node[] = [{ id: 'a', position: { x: 10, y: 20 }, data: {} }]
    expect(selectedNodeBounds(nodes, new Set(['a']), { width: 280, height: 160 }))
      .toEqual({ left: 10, top: 20, right: 290, bottom: 180 })
  })

  it('detects segment and polyline intersections with a selection rectangle', () => {
    const rect = { left: 100, top: 100, right: 200, bottom: 200 }
    expect(segmentIntersectsRect(50, 150, 250, 150, rect)).toBe(true)
    expect(segmentIntersectsRect(50, 50, 150, 90, rect)).toBe(false)
    expect(segmentIntersectsRect(0, 0, 300, 300, rect)).toBe(true)
    expect(polylineIntersectsRect([{ x: 0, y: 0 }, { x: 50, y: 150 }, { x: 250, y: 150 }], rect)).toBe(true)
    expect(polylineIntersectsRect([{ x: 0, y: 0 }, { x: 50, y: 50 }, { x: 90, y: 90 }], rect)).toBe(false)
  })
})
