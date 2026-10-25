import { describe, expect, it } from 'vitest'
import type { FieldCell } from '../bindings/FieldCell'
import type { RecordRow } from '../bindings/RecordRow'
import { projectBatchCells, projectBatchRecordFields } from './batchRecordProjection'
import { sameFieldValue } from './fieldProjection'

const cell = (name: string, value: FieldCell['value'], declaredType = 'string'): FieldCell => ({
  name,
  value,
  missing: false,
  annotation: {
    enum_int_value: null,
    declared_type: declaredType,
    ref_target_type: null,
    enum_type: null,
    enum_is_flag: false,
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
  coordinate: { actual_type: 'Item', id: key},
  display_path: 'data/items.cfd',
  container_index: 0,
  container_size: 1,
  fields,
  field_index: Object.fromEntries(fields.map((field, index) => [field.name, index])),
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
    expect(sameFieldValue(left, right)).toBe(true)
  })

  it('omits fields missing from any record or using incompatible declarations', () => {
    const fields = projectBatchRecordFields([
      row('a', [cell('name', { kind: 'string', value: 'A' })]),
      row('b', [cell('name', { kind: 'string', value: 'B' }, 'LocalizedText')]),
    ])
    expect(fields).toEqual([])
  })

  it('projects a same-type cell range and rejects differing editor annotations', () => {
    const left = cell('first', { kind: 'string', value: 'A' })
    const right = cell('second', { kind: 'string', value: 'B' })
    expect(projectBatchCells([left, right])).toMatchObject({ state: 'mixed', editable: true })

    right.annotation = { ...right.annotation!, ref_target_type: 'Other' }
    expect(projectBatchCells([left, right])).toBeNull()
  })
})

it('stops at incompatible cells and preserves readonly aggregation for compatible cells', () => {
  let visits = 0
  const source = { *[Symbol.iterator]() {
    visits++; yield cell('a', { kind: 'string', value: 'a' })
    visits++; yield cell('b', { kind: 'int', value: 1n }, 'int')
    throw new Error('must not scan after incompatible type')
  } }
  expect(projectBatchCells(source)).toBeNull()
  expect(visits).toBe(2)
  const locked = cell('b', { kind: 'string', value: 'b' })
  locked.annotation!.read_only = true
  expect(projectBatchCells([locked, locked])).toMatchObject({ editable: false, count: 2, state: 'same' })
})
