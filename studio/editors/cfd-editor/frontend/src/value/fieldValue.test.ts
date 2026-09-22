import { describe, expect, it } from 'vitest'
import { parseFieldValueText, plainFieldValueText, recordMatchesFullTextSearch, recordMatchesSearch, referenceKeyText, summaryOf } from './fieldValue'
import type { RecordRow } from '../bindings/RecordRow'

describe('FieldValue authoring', () => {
  it('keeps signed 32-bit integer limits', () => {
    const parsed = parseFieldValueText(
      { kind: 'int', value: 0n },
      '2147483647',
    )

    expect(parsed).toEqual({ kind: 'int', value: 2147483647n })
  })

  it('rejects partial or non-finite numeric input', () => {
    expect(parseFieldValueText({ kind: 'int', value: 0n }, '12px')).toBeNull()
    expect(parseFieldValueText({ kind: 'float', value: 0 }, 'Infinity')).toBeNull()
  })

  it('edits wrapped scalar payloads without dropping their variant', () => {
    expect(parseFieldValueText({
      kind: 'option_some',
      value: { kind: 'int', value: 1n },
    }, '2')).toEqual({ kind: 'option_some', value: { kind: 'int', value: 2n } })
  })

  it('provides one summary for table filtering and editor cards', () => {
    expect(summaryOf({
      kind: 'array',
      value: [
        { kind: 'string', value: 'alpha' },
        { kind: 'int', value: 2147483647n },
      ],
    })).toBe('[alpha, 2147483647]')
  })

  it('shows and edits template source without execution', () => {
    const value = { kind: 'formatted_string' as const, value: { source: 'f"{self.name}"' } }
    expect(summaryOf(value)).toBe('f"{self.name}"')
    expect(plainFieldValueText(value)).toBe('f"{self.name}"')
    expect(parseFieldValueText(value, 'f"new {self.name}"')).toEqual({ kind: 'formatted_string', value: { source: 'f"new {self.name}"' } })
    expect(parseFieldValueText(value, 'ordinary')).toBeNull()
  })

  it('keeps braces literal in ordinary strings', () => {
    expect(parseFieldValueText({ kind: 'string', value: '' }, '{self.name}')).toEqual({ kind: 'string', value: '{self.name}' })
    expect(parseFieldValueText({ kind: 'int', value: 0n }, '2147483648')).toBeNull()
  })

  it('renders references as keys without type qualifiers', () => {
    expect(referenceKeyText('&ItemConfig::sword')).toBe('sword')
    expect(summaryOf({ kind: 'ref', value: 'ItemConfig::sword' })).toBe('sword')
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
