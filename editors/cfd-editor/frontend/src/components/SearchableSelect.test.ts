import { describe, expect, it } from 'vitest'
import {
  filterSearchableOptions,
  moveSearchableOptionIndex,
  type SearchableOption,
} from './SearchableSelect'

const options: SearchableOption[] = [
  { value: 'sword_iron', label: 'Item.sword_iron' },
  { value: 'shield_wood', label: 'Item.shield_wood' },
  { value: 'Rare' },
]

describe('filterSearchableOptions', () => {
  it('matches labels and values without case sensitivity', () => {
    expect(filterSearchableOptions(options, 'SWORD')).toEqual([options[0]])
    expect(filterSearchableOptions(options, 'rare')).toEqual([options[2]])
  })

  it('requires every whitespace-separated search term', () => {
    expect(filterSearchableOptions(options, 'item wood')).toEqual([options[1]])
    expect(filterSearchableOptions(options, 'item rare')).toEqual([])
  })

  it('preserves option order for an empty search', () => {
    expect(filterSearchableOptions(options, '   ')).toEqual(options)
  })
})

describe('moveSearchableOptionIndex', () => {
  it('moves through options and wraps at both ends', () => {
    expect(moveSearchableOptionIndex(1, 3, 1)).toBe(2)
    expect(moveSearchableOptionIndex(2, 3, 1)).toBe(0)
    expect(moveSearchableOptionIndex(0, 3, -1)).toBe(2)
  })

  it('stays at the first index when there are no options', () => {
    expect(moveSearchableOptionIndex(0, 0, 1)).toBe(0)
  })
})
