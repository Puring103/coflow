import { describe, expect, it } from 'vitest'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import type { RecordRow } from '../bindings/RecordRow'
import { recordsSupportGraph } from './graphSupport'

function record(value: RecordRow['fields'][number]['value'], annotation: FieldAnnotation | null = null): RecordRow {
  return {
    coordinate: { actual_type: 'Npc', key: 'Npc_001' },
    display_path: 'Npc.Npc_001',
    fields: [{ name: 'reward', value, annotation }],
    field_index: { reward: 0 },
    field_summaries: { reward: '' },
    field_diagnostics: [],
    diagnostic_severity: null,
  }
}

describe('recordsSupportGraph', () => {
  it('recognizes a ref value even when derived annotations are unavailable', () => {
    expect(recordsSupportGraph([record({ kind: 'ref', value: 'Item.Item_001' })])).toBe(true)
  })

  it('recognizes nested ref values', () => {
    expect(recordsSupportGraph([record({
      kind: 'array',
      value: [{
        kind: 'object',
        value: {
          actual_type: 'Drop',
          fields: { item: { kind: 'ref', value: 'Item.Item_001' } },
        },
      }],
    })])).toBe(true)
  })

  it('recognizes schema ref annotations for empty values', () => {
    const annotation = {
      ref_target_type: 'Item',
      item_annotation: null,
      children: {},
    } as FieldAnnotation
    expect(recordsSupportGraph([record({ kind: 'null' }, annotation)])).toBe(true)
  })

  it('rejects records without reference values or annotations', () => {
    expect(recordsSupportGraph([record({ kind: 'string', value: 'Item.Item_001' })])).toBe(false)
  })
})
