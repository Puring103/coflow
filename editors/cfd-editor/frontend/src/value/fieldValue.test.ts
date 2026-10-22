import { describe, expect, it } from 'vitest'
import { parseFieldValueText, plainFieldValueText, recordMatchesFullTextSearch, recordMatchesSearch, referenceIdText, summaryOf } from './fieldValue'
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
        { kind: 'int', value: 9007199254740993n },
      ],
    })).toBe('[alpha, 9007199254740993]')
  })

  it('displays formatted output but edits the original source', () => {
    const value = {
      kind: 'formatted_string' as const,
      value: {
        source: 'f"<b>{&Item::sword.name}</b>"',
        rendered: '<b>Iron Sword</b>',
      },
    }
    expect(summaryOf(value)).toBe('<b>Iron Sword</b>')
    expect(plainFieldValueText(value)).toBe('<b>{&Item::sword.name}</b>')
  })

  it('keeps ordinary strings literal and preserves the declared template kind', () => {
    const text = '当前 {self.name}，跨类型 {&Item::sword.name}'
    expect(parseFieldValueText({ kind: 'string', value: '' }, text)).toEqual({ kind: 'string', value: text })
    expect(parseFieldValueText({ kind: 'formatted_string', value: { source: 'f"old"', rendered: 'old' } }, text)).toEqual({
      kind: 'formatted_string', value: { source: `f${JSON.stringify(text)}`, rendered: 'old' },
    })
    expect(parseFieldValueText({ kind: 'formatted_string', value: { source: 'f"old"', rendered: 'old' } }, 'plain text')).toEqual({
      kind: 'formatted_string', value: { source: 'f"plain text"', rendered: 'old' },
    })
  })

  it('renders references as keys without type qualifiers', () => {
    expect(referenceIdText('&ItemConfig.sword')).toBe('sword')
    expect(summaryOf({ kind: 'ref', value: 'ItemConfig.sword' })).toBe('sword')
    expect(referenceIdText('plain_key')).toBe('plain_key')
  })

  it('matches only record keys in the standard search mode', () => {
    const record = {
      coordinate: { actual_type: 'Item', id: 'sword' },
      fields: [{ name: 'displayName', value: { kind: 'string', value: 'Excalibur' } }],
    } as unknown as RecordRow

    expect(recordMatchesSearch(record, 'swo')).toBe(true)
    expect(recordMatchesSearch(record, 'display')).toBe(false)
    expect(recordMatchesSearch(record, 'calib')).toBe(false)
    expect(recordMatchesSearch(record, 'shield')).toBe(false)
  })

  it('searches values nested in arrays, dictionaries, and objects in full-text mode', () => {
    const record = {
      coordinate: { actual_type: 'Item', id: 'sword' },
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
