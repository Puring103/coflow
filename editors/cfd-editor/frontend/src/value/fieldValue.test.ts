import { describe, expect, it } from 'vitest'
import { parseFieldValueText, recordMatchesSearch, referenceKeyText, summaryOf } from './fieldValue'
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

  it('uses the same key, field-name, and summary search across editor views', () => {
    const record = {
      coordinate: { actual_type: 'Item', key: 'sword' },
      fields: [{ name: 'displayName', value: { kind: 'string', value: 'Excalibur' } }],
    } as RecordRow

    expect(recordMatchesSearch(record, 'swo')).toBe(true)
    expect(recordMatchesSearch(record, 'display')).toBe(true)
    expect(recordMatchesSearch(record, 'calib')).toBe(true)
    expect(recordMatchesSearch(record, 'shield')).toBe(false)
  })
})
