import { describe, expect, it } from 'vitest'
import type { GraphData } from '../bindings/GraphData'
import type { RecordCoordinate } from '../bindings/RecordCoordinate'
import { reachableGraph } from './graphFilter'

const coordinate = (key: string): RecordCoordinate => ({ actual_type: 'Node', key })
const graph = (edges: [string, string][]): GraphData => ({
  revision: 1,
  nodes: ['a', 'b', 'c', 'd'].map(key => ({
    coordinate: coordinate(key), file_path: 'data.cfd', in_focus_file: true,
    is_collapsed: false, fields: [], field_diagnostics: [], diagnostic_severity: null,
  })),
  edges: edges.map(([source, target]) => ({
    source: coordinate(source), target: coordinate(target), field_path: 'next',
  })),
  available_fields: ['next'],
})

describe('reachableGraph', () => {
  it('keeps every node reachable from matching roots through cycles', () => {
    const filtered = reachableGraph(graph([['a', 'b'], ['b', 'c'], ['c', 'a']]), value => value.key === 'a')
    expect(filtered.nodes.map(node => node.coordinate.key)).toEqual(['a', 'b', 'c'])
    expect(filtered.edges).toHaveLength(3)
  })

  it('excludes disconnected nodes and edges', () => {
    const filtered = reachableGraph(graph([['a', 'b'], ['c', 'd']]), value => value.key === 'a')
    expect(filtered.nodes.map(node => node.coordinate.key)).toEqual(['a', 'b'])
    expect(filtered.edges.map(edge => edge.source.key)).toEqual(['a'])
  })
})
