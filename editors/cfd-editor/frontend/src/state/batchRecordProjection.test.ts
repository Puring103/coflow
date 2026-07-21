import { describe, expect, it } from 'vitest'
import type { FieldCell } from '../bindings/FieldCell'
import type { RecordRow } from '../bindings/RecordRow'
import { fieldValuesEqual, projectBatchRecordFields } from './batchRecordProjection'

const cell = (name: string, value: FieldCell['value'], declaredType = 'string'): FieldCell => ({
  name,
  value,
  annotation: {
    spread_info: null,
    ref_target_file: null,
    enum_int_value: null,
    declared_type: declaredType,
    ref_target_type: null,
    enum_type: null,
    nullable: false,
    read_only: false,
    item_annotation: null,
    polymorphic_types: [],
    object_type: null,
    field_order: [],
    children: {},
  },
})
const row = (key: string, fields: FieldCell[]): RecordRow => ({
  coordinate: { actual_type: 'Item', key },
  display_path: 'data/items.cfd',
  container_index: 0,
  container_size: 1,
  fields,
  field_index: {},
  field_summaries: {},
  field_diagnostics: [],
  diagnostic_severity: null,
})

describe('batch record projection', () => {
  it('keeps common compatible fields and marks different values as mixed', () => {
    const fields = projectBatchRecordFields([
      row('a', [cell('name', { kind: 'string', value: 'A' }), cell('price', { kind: 'int', value: 2n }, 'int')]),
      row('b', [cell('name', { kind: 'string', value: 'B' }), cell('price', { kind: 'int', value: 2n }, 'int')]),
    ])

    expect(fields.map(field => [field.cell.name, field.state])).toEqual([
      ['name', 'mixed'],
      ['price', 'same'],
    ])
  })

  it('compares nested values without serializing bigint values', () => {
    const left = { kind: 'array' as const, value: [{ kind: 'int' as const, value: 2n }] }
    const right = { kind: 'array' as const, value: [{ kind: 'int' as const, value: 2n }] }
    expect(fieldValuesEqual(left, right)).toBe(true)
  })

  it('omits fields missing from any record or using incompatible declarations', () => {
    const fields = projectBatchRecordFields([
      row('a', [cell('name', { kind: 'string', value: 'A' })]),
      row('b', [cell('name', { kind: 'string', value: 'B' }, 'LocalizedText')]),
    ])
    expect(fields).toEqual([])
  })
})
