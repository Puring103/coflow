import { describe, expect, it } from 'vitest'
import { scrubNumericValue } from './numericScrub'

describe('numeric scrubbing', () => {
  it('moves integers in stable whole-number steps', () => {
    expect(scrubNumericValue({ kind: 'int', value: 12n }, 8, { shiftKey: false, altKey: false }))
      .toEqual({ kind: 'int', value: 16n })
  })

  it('supports coarse and fine float adjustments', () => {
    expect(scrubNumericValue({ kind: 'float', value: 1.5 }, 10, { shiftKey: true, altKey: false }).value)
      .toBe(11.5)
    expect(scrubNumericValue({ kind: 'float', value: 1.5 }, 10, { shiftKey: false, altKey: true }).value)
      .toBe(1.6)
  })
})
