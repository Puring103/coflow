import { describe, expect, it } from 'vitest'
import {
  canReleaseResizeScrollFloor,
  resizedColumnWidth,
  resizeScrollWidthFloor,
} from './tableColumnResize'

describe('table column resize', () => {
  it('converts pointer movement from screen pixels into table coordinates', () => {
    expect(resizedColumnWidth(120, 300, 330, 1.5, 48)).toBe(140)
    expect(resizedColumnWidth(120, 300, 270, 0.75, 48)).toBe(80)
    expect(resizedColumnWidth(60, 300, 200, 1, 48)).toBe(48)
  })

  it('keeps enough scrollable width to preserve the current viewport', () => {
    expect(resizeScrollWidthFloor(900, 500, 480)).toBe(980)
    expect(resizeScrollWidthFloor(1200, 500, 480)).toBe(1200)
  })

  it('releases the floor only after the resized table covers the viewport', () => {
    expect(canReleaseResizeScrollFloor(900, 500, 480)).toBe(false)
    expect(canReleaseResizeScrollFloor(980, 500, 480)).toBe(true)
  })
})
