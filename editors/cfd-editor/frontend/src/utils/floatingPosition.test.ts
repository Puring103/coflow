import { describe, expect, it } from 'vitest'
import { fitViewportPosition } from './floatingPosition'

describe('fitViewportPosition', () => {
  it('keeps a fully visible menu at its anchor', () => {
    expect(fitViewportPosition(
      { x: 100, y: 120 },
      { width: 196, height: 200 },
      { width: 800, height: 600 },
    )).toEqual({ x: 100, y: 120 })
  })

  it('moves a menu away from the bottom and right viewport edges', () => {
    expect(fitViewportPosition(
      { x: 750, y: 550 },
      { width: 196, height: 300 },
      { width: 800, height: 600 },
    )).toEqual({ x: 596, y: 292 })
  })

  it('pins an oversized menu to the viewport padding', () => {
    expect(fitViewportPosition(
      { x: 40, y: 50 },
      { width: 900, height: 700 },
      { width: 800, height: 600 },
    )).toEqual({ x: 8, y: 8 })
  })
})
