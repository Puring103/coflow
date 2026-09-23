import { describe, expect, it } from 'vitest'
import type { FieldCell } from '../bindings/FieldCell'
import type { GraphData } from '../bindings/GraphData'
import type { RecordRow } from '../bindings/RecordRow'
import { graphCacheKey, projectGraphRows } from './appSupport'

const row = (actualType: string, key: string): RecordRow => ({
  coordinate: { actual_type: actualType, key }, display_path: 'data/items.cfd',
  container_index: 0, container_size: 1, fields: [], field_index: {}, formatted_previews: {}, field_summaries: {},
  field_diagnostics: [], diagnostic_severity: null,
})

describe('graph projection identities', () => {
  it('distinguishes coordinates containing the old delimiter', () => {
    const target = row('A\u001fB', 'C')
    const unrelated = row('A', 'B\u001fC')
    unrelated.fields = [{ name: 'different', value: { kind: 'string', value: 'other' }, missing: false, annotation: null } satisfies FieldCell]
    const graph: GraphData = {
      revision: 1, nodes: [{ coordinate: target.coordinate, file_path: 'data/items.cfd',
        in_focus_file: true, is_collapsed: false, fields: target.fields,
        field_diagnostics: target.field_diagnostics, diagnostic_severity: null }],
      edges: [], available_fields: [],
    }
    const result = projectGraphRows({ graph }, 2, [unrelated])
    expect(result.graph.nodes[0].fields).toBe(target.fields)
    expect(result.graph.nodes[0].field_diagnostics).toBe(target.field_diagnostics)
  })

  it('keeps graph cache paths intact through the structured key', () => {
    expect(JSON.parse(graphCacheKey('data/name::2::3.cfd', 2, 100)))
      .toEqual(['data/name::2::3.cfd', 2, 100])
  })
})
