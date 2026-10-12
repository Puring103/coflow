import { describe, expect, it } from 'vitest'
import { expandedPathsFor, updateExpandedPath, type ExpandedPathMap } from './expandedPaths'

describe('expanded path state', () => {
  it('keeps expansion isolated by record owner', () => {
    let state: ExpandedPathMap = new Map()
    state = updateExpandedPath(state, 'items:Item.sword', 'stats', true)
    state = updateExpandedPath(state, 'items:Item.shield', 'effects[0]', true)

    expect([...expandedPathsFor(state, 'items:Item.sword')]).toEqual(['stats'])
    expect([...expandedPathsFor(state, 'items:Item.shield')]).toEqual(['effects[0]'])
  })

  it('removes empty owner entries and preserves no-op identity', () => {
    let state: ExpandedPathMap = new Map()
    state = updateExpandedPath(state, 'items:Item.sword', 'stats', true)
    expect(updateExpandedPath(state, 'items:Item.sword', 'stats', true)).toBe(state)

    state = updateExpandedPath(state, 'items:Item.sword', 'stats', false)
    expect(state.has('items:Item.sword')).toBe(false)
  })
})
