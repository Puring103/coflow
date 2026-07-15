import { describe, expect, it } from 'vitest'
import { moveRecordItem, type VisibleRecordItem } from './recordItemNavigation'

const items: VisibleRecordItem[] = [
  { id: 'stats', depth: 0, expandable: true },
  { id: 'stats.health', depth: 1, expandable: false },
  { id: 'name', depth: 0, expandable: false },
]

describe('record item navigation', () => {
  it('moves through visible items with up and down', () => {
    expect(moveRecordItem(items, 'stats', 'ArrowDown')).toEqual({ kind: 'select', id: 'stats.health' })
    expect(moveRecordItem(items, 'stats.health', 'ArrowUp')).toEqual({ kind: 'select', id: 'stats' })
  })

  it('returns to the closest visible parent with left', () => {
    expect(moveRecordItem(items, 'stats.health', 'ArrowLeft')).toEqual({ kind: 'select', id: 'stats' })
    expect(moveRecordItem(items, 'stats', 'ArrowLeft')).toEqual({ kind: 'boundary', edge: 'parent' })
  })

  it('reports the boundary above the first visible item', () => {
    expect(moveRecordItem(items, 'stats', 'ArrowUp')).toEqual({ kind: 'boundary', edge: 'before' })
  })

  it('toggles expandable items with right', () => {
    expect(moveRecordItem(items, 'stats', 'ArrowRight')).toEqual({ kind: 'toggle', id: 'stats' })
    expect(moveRecordItem(items, 'name', 'ArrowRight')).toBeNull()
  })
})
