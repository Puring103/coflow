import { describe, expect, it } from 'vitest'
import type { FileRecords } from '../bindings/FileRecords'
import { fieldPathField, fieldPathIndex } from '../wire'
import {
  projectFieldValue,
  projectFieldValueAtRevision,
  sameFieldValue,
} from './fieldProjection'

const coordinate = { actual_type: 'Item', key: 'sword' }
const records = {
  revision: 1,
  file_path: 'data/items.cfd',
  records: [{
    coordinate,
    display_path: 'data/items.cfd',
    fields: [{
      name: 'values',
      value: { kind: 'array', value: [{ kind: 'int', value: 1n }] },
      annotation: null,
    }],
    field_index: { values: 0 },
    field_summaries: { values: '[1]' },
    field_diagnostics: [],
    diagnostic_severity: null,
  }],
} as unknown as FileRecords

describe('field projection', () => {
  it('filters deep no-op values', () => {
    const result = projectFieldValue(
      records,
      coordinate,
      [fieldPathField('values')],
      { kind: 'array', value: [{ kind: 'int', value: 1n }] },
    )

    expect(result.changed).toBe(false)
    expect(result.records).toBe(records)
  })

  it('does not filter no-ops against a stale file generation', () => {
    expect(projectFieldValueAtRevision(
      records,
      2,
      coordinate,
      [fieldPathField('values')],
      { kind: 'array', value: [{ kind: 'int', value: 1n }] },
    )).toBeUndefined()
  })

  it('projects nested edits without mutating the previous generation', () => {
    const result = projectFieldValue(
      records,
      coordinate,
      [fieldPathField('values'), fieldPathIndex(0)],
      { kind: 'int', value: 2n },
    )

    expect(result.changed).toBe(true)
    expect(result.records).not.toBe(records)
    expect(result.row?.fields[0].value).toEqual({
      kind: 'array',
      value: [{ kind: 'int', value: 2n }],
    })
    expect(result.oldValue).toEqual({ kind: 'int', value: 1n })
    expect(records.records[0].fields[0].value).toEqual({
      kind: 'array',
      value: [{ kind: 'int', value: 1n }],
    })
  })

  it('compares bigint-backed values by value', () => {
    expect(sameFieldValue(
      { kind: 'int', value: 1n },
      { kind: 'int', value: BigInt('1') },
    )).toBe(true)
  })
})
