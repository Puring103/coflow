import { describe, expect, it } from 'vitest'
import type { GraphNodeView } from '../wire'
import { graphTopologySignature } from './GraphView.layout'

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
