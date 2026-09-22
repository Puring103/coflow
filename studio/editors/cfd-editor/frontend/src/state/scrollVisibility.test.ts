import { describe, expect, it } from 'vitest'
import { visibilityScrollDelta } from './scrollVisibility'

describe('selection visibility scrolling', () => {
  it('keeps a fully visible selection stationary', () => {
    expect(visibilityScrollDelta({ start: 42, end: 242 }, { start: 60, end: 180 })).toBe(0)
  })

  it('reveals either clipped edge with the smallest movement', () => {
    expect(visibilityScrollDelta({ start: 42, end: 242 }, { start: 20, end: 80 })).toBe(-22)
    expect(visibilityScrollDelta({ start: 42, end: 242 }, { start: 200, end: 270 })).toBe(28)
  })

  it('aligns an oversized selection to the visible leading edge', () => {
    expect(visibilityScrollDelta({ start: 42, end: 242 }, { start: 80, end: 320 })).toBe(38)
  })
})
