import { describe, expect, it, vi } from 'vitest'
import type { GraphNodeView } from '../wire'
import { graphTopologySignature, layoutGraph, retainGraphPositions, type GraphLayoutResult } from './GraphView.layout'

function node(value: string): GraphNodeView {
  return {
    id: 'Item::one',
    key: 'one',
    actual_type: 'Item',
    coordinate: { actual_type: 'Item', key: 'one' },
    file_path: 'data/items.cfd',
    in_focus_file: true,
    is_collapsed: false,
    fields: [{
      name: 'name',
      value: { kind: 'string', value },
      missing: false,
      annotation: null,
    }],
    field_diagnostics: [],
    diagnostic_severity: null,
  }
}

describe('graph topology signature', () => {
  it('ignores scalar content edits', () => {
    expect(graphTopologySignature({ nodes: [node('old')], edges: [] }))
      .toBe(graphTopologySignature({ nodes: [node('new')], edges: [] }))
  })

  it('ignores field visibility and non-reference collection changes', () => {
    const original = node('old')
    const changed = { ...original, fields: [{ ...original.fields[0],
      value: { kind: 'array' as const, value: [{ kind: 'string' as const, value: 'new' }] } }] }
    const signature = graphTopologySignature({ nodes: [original], edges: [] })
    expect(graphTopologySignature({ nodes: [changed], edges: [] })).toBe(signature)
    expect(graphTopologySignature({ nodes: [{ ...original, fields: [] }], edges: [] })).toBe(signature)
  })

  it('changes when reference edges change', () => {
    const graph = { nodes: [node('same')], edges: [] }
    expect(graphTopologySignature(graph)).not.toBe(graphTopologySignature({
      ...graph,
      edges: [{
        source: 'Item::one',
        target: 'Item::two',
        field_path: 'next',
        raw: {
          source: { actual_type: 'Item', key: 'one' },
          target: { actual_type: 'Item', key: 'two' },
          field_path: 'next',
        },
      }],
    }))
  })
})

describe('graph refresh positions', () => {
  it('restores saved positions and places new nodes without running the layout engine', async () => {
    const a = node('a')
    const b = { ...node('b'), id: 'Item::two' }
    const c = { ...node('c'), id: 'Item::three' }
    const edge = (target: GraphNodeView) => ({ source: a.id, target: target.id, field_path: 'next',
      raw: { source: a.coordinate, target: target.coordinate, field_path: 'next' } })
    const saved = new Map([[a.id, { x: -500, y: 200 }], [b.id, { x: 60, y: 200 }]])
    const engine = vi.fn()
    const result = await layoutGraph({ nodes: [a, b, c], edges: [edge(b), edge(c)] },
      new Set(['next']), undefined, new Map(), new Map(), engine, saved)
    expect(engine).not.toHaveBeenCalled()
    expect(result.positions.get(a.id)).toEqual(saved.get(a.id))
    expect(result.positions.get(b.id)).toEqual(saved.get(b.id))
    expect(result.positions.get(c.id)!.y).toBeGreaterThan(saved.get(b.id)!.y)
  })
  it('retains dragged coordinates and places new targets below measured occupied nodes', () => {
    const a = node('a')
    const b = { ...node('b'), id: 'Item::two' }
    const c = { ...node('c'), id: 'Item::three' }
    const retained = new Map([[a.id, { x: 100, y: 200 }], [b.id, { x: 660, y: 200 }]])
    const layout: GraphLayoutResult = {
      visibleNodes: [a, b, c], positions: new Map([[a.id, { x: 0, y: 0 }], [b.id, { x: 0, y: 90 }], [c.id, { x: 0, y: 180 }]]),
      forwardEdges: [{ source: a.id, target: c.id, field_path: 'next',
        raw: { source: a.coordinate, target: c.coordinate, field_path: 'next' } }], backEdges: [],
    }
    const positions = retainGraphPositions(layout, retained, new Map(), new Map(), new Map([[b.id, 700]]))
    expect(positions.get(a.id)).toEqual(retained.get(a.id))
    expect(positions.get(b.id)).toEqual(retained.get(b.id))
    expect(positions.get(c.id)).toEqual({ x: 660, y: 990 })
    expect(layout.positions.get(a.id)).toEqual({ x: 0, y: 0 })
  })

  it('restores a removed node at its previous position after undo', () => {
    const a = node('a')
    const retained = new Map([[a.id, { x: -300, y: 450 }]])
    const layout: GraphLayoutResult = { visibleNodes: [a], positions: new Map([[a.id, { x: 0, y: 0 }]]),
      forwardEdges: [], backEdges: [] }
    expect(retainGraphPositions(layout, retained, new Map(), new Map()).get(a.id)).toEqual({ x: -300, y: 450 })
  })
})
