import { describe, expect, it } from 'vitest'
import type { Node } from '@xyflow/react'
import { graphHighlights, reconcileFlowNodes, sameGraphValue } from './GraphView.state'

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
