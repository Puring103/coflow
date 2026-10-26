import { describe, expect, it } from 'vitest'
import { boundsOf, getHelperLines, unionBounds, type AlignBounds } from './GraphView.alignment'

const b = (x: number, y: number, width = 100, height = 50): AlignBounds => ({
  left: x,
  right: x + width,
  top: y,
  bottom: y + height,
  width,
  height,
})

describe('getHelperLines', () => {
  it('snaps to the nearest edge within the threshold', () => {
    const result = getHelperLines(b(13, 0), [b(10, 200)], 5)

    expect(result.snapPosition.x).toBe(10)
    expect(result.vertical).toBe(10)
  })

  it('does not snap when every line is beyond the threshold', () => {
    const result = getHelperLines(b(20, 0), [b(10, 200)], 5)

    expect(result.snapPosition.x).toBeUndefined()
    expect(result.vertical).toBeUndefined()
  })

  it('snaps vertical edges and horizontal edges independently', () => {
    const result = getHelperLines(b(0, 103), [b(200, 100)], 5)

    expect(result.snapPosition.y).toBe(100)
    expect(result.horizontal).toBe(100)
  })

  it('aligns center lines to center lines', () => {
    // 两边边缘都超出阈值，只有中心线对齐。
    const result = getHelperLines(b(0, 0, 100, 50), [b(46, 200, 8, 50)], 5)

    expect(result.snapPosition.x).toBe(0)
    expect(result.vertical).toBe(50)
  })
})

describe('unionBounds', () => {
  it('returns the bounding box of several nodes', () => {
    expect(unionBounds([b(0, 0, 100, 50), b(120, 80, 100, 50)])).toEqual({
      left: 0,
      right: 220,
      top: 0,
      bottom: 130,
      width: 220,
      height: 130,
    })
  })

  it('returns null when there is nothing to bound', () => {
    expect(unionBounds([])).toBeNull()
  })
})

describe('boundsOf', () => {
  it('uses measured size when available and falls back to defaults otherwise', () => {
    expect(boundsOf({ position: { x: 11, y: 22 }, measured: { width: 30, height: 40 } })).toEqual({
      left: 11,
      right: 41,
      top: 22,
      bottom: 62,
      width: 30,
      height: 40,
    })
    expect(boundsOf({ position: { x: 0, y: 0 } })).toEqual({
      left: 0,
      right: 280,
      top: 0,
      bottom: 160,
      width: 280,
      height: 160,
    })
  })
})
