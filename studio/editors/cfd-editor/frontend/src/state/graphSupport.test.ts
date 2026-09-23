import { describe, expect, it } from 'vitest'
import type { FileRecords } from '../bindings/FileRecords'
import type { FieldAnnotation } from '../bindings/FieldAnnotation'
import type { RecordRow } from '../bindings/RecordRow'
import { graphSupportForFile, recordsSupportGraph } from './graphSupport'

function record(value: RecordRow['fields'][number]['value'], annotation: FieldAnnotation | null = null): RecordRow {
  return {
    coordinate: { actual_type: 'Npc', key: 'Npc_001' },
    display_path: 'Npc.Npc_001',
    container_index: 0,
    container_size: 1,
    fields: [{ name: 'reward', value, missing: false, annotation }],
    field_index: { reward: 0 },
    formatted_previews: {},
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
    expect(recordsSupportGraph([record({ kind: 'option_none' }, annotation)])).toBe(true)
  })

  it('rejects records without reference values or annotations', () => {
    expect(recordsSupportGraph([record({ kind: 'string', value: 'Item.Item_001' })])).toBe(false)
  })
})


describe('graph support snapshot cache', () => {
  it('does not reuse a result for another project with the same file path and revision', () => {
    const file = (value: RecordRow['fields'][number]['value']): FileRecords => ({
      file_path: 'data/items.cfd', revision: 1, type_names: [], columns: [],
      records: [record(value)], capabilities: {} as FileRecords['capabilities'],
    })
    const withReference = file({ kind: 'ref', value: 'Item.Item_001' })
    const withoutReference = file({ kind: 'string', value: 'ordinary' })
    expect(graphSupportForFile(withReference)).toBe(true)
    expect(graphSupportForFile(withoutReference)).toBe(false)
    expect(graphSupportForFile(withReference)).toBe(true)
  })
})
