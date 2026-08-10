import { describe, expect, it } from 'vitest'
import { everyFlagMask, selectedFlagVariantNames, toggleFlagMask } from './DataCard'

const variants = [
  { name: 'None', value: 0n },
  { name: 'Read', value: 1n },
  { name: 'Write', value: 2n },
  { name: 'Execute', value: 4n },
]

describe('flag enum selection', () => {
  it('maps a mask to selected variants in schema order', () => {
    expect(selectedFlagVariantNames(variants, 5n)).toEqual(['Read', 'Execute'])
    expect(selectedFlagVariantNames(variants, 0n)).toEqual(['None'])
  })

  it('toggles individual bits and treats the zero variant as clear', () => {
    expect(toggleFlagMask(1n, 2n)).toBe(3n)
    expect(toggleFlagMask(3n, 1n)).toBe(2n)
    expect(toggleFlagMask(3n, 0n)).toBe(0n)
  })

  it('builds the automatic Every mask from all declared bits', () => {
    expect(everyFlagMask(variants)).toBe(7n)
    expect(everyFlagMask([])).toBe(0n)
  })
})
