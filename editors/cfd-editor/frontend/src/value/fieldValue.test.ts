import { describe, expect, it } from 'vitest'
import { parseFieldValueText, recordMatchesFullTextSearch, recordMatchesSearch, referenceKeyText, summaryOf } from './fieldValue'
import type { RecordRow } from '../bindings/RecordRow'

describe('FieldValue authoring', () => {
  it('parses integers without passing through a JavaScript number', () => {
    const parsed = parseFieldValueText(
      { kind: 'int', value: 0n },
      '9007199254740993123456789',
    )

    expect(parsed).toEqual({ kind: 'int', value: 9007199254740993123456789n })
  })

  it('rejects partial or non-finite numeric input', () => {
    expect(parseFieldValueText({ kind: 'int', value: 0n }, '12px')).toBeNull()
    expect(parseFieldValueText({ kind: 'float', value: 0 }, 'Infinity')).toBeNull()
  })

  it('provides one summary for table filtering and editor cards', () => {
    expect(summaryOf({
      kind: 'array',
      value: [
        { kind: 'string', value: 'alpha' },
        { kind: 'int', value: 9007199254740993n },
      ],
    })).toBe('[alpha, 9007199254740993]')
  })

  it('renders references as keys without type qualifiers', () => {
    expect(referenceKeyText('&ItemConfig.sword')).toBe('sword')
    expect(summaryOf({ kind: 'ref', value: 'ItemConfig.sword' })).toBe('sword')
    expect(referenceKeyText('plain_key')).toBe('plain_key')
  })

  it('matches only record keys in the standard search mode', () => {
    const record = {
      coordinate: { actual_type: 'Item', key: 'sword' },
      fields: [{ name: 'displayName', value: { kind: 'string', value: 'Excalibur' } }],
    } as unknown as RecordRow

    expect(recordMatchesSearch(record, 'swo')).toBe(true)
    expect(recordMatchesSearch(record, 'display')).toBe(false)
    expect(recordMatchesSearch(record, 'calib')).toBe(false)
    expect(recordMatchesSearch(record, 'shield')).toBe(false)
  })

  it('searches values nested in arrays, dictionaries, and objects in full-text mode', () => {
    const record = {
      coordinate: { actual_type: 'Item', key: 'sword' },
      fields: [{
        name: 'metadata',
        value: {
          kind: 'dict',
          value: [[
            { kind: 'string', value: 'lore' },
            { kind: 'array', value: [{
              kind: 'object',
              value: { actual_type: 'Description', fields: { text: { kind: 'string', value: 'Forged beneath the moon' } } },
            }] },
          ]],
        },
      }],
    } as unknown as RecordRow

    expect(recordMatchesSearch(record, 'moon')).toBe(false)
    expect(recordMatchesFullTextSearch(record, 'lore')).toBe(true)
    expect(recordMatchesFullTextSearch(record, 'moon')).toBe(true)
    expect(recordMatchesFullTextSearch(record, 'description')).toBe(true)
  })
})
