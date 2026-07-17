import { describe, expect, it } from 'vitest'
import { moveDimensionCell } from './DimensionTableView'

describe('moveDimensionCell', () => {
  it('moves in all four directions', () => {
    expect(moveDimensionCell({ row: 1, column: 1 }, 'ArrowLeft', 3, 4)).toEqual({ row: 1, column: 0 })
    expect(moveDimensionCell({ row: 1, column: 1 }, 'ArrowRight', 3, 4)).toEqual({ row: 1, column: 2 })
    expect(moveDimensionCell({ row: 1, column: 1 }, 'ArrowUp', 3, 4)).toEqual({ row: 0, column: 1 })
    expect(moveDimensionCell({ row: 1, column: 1 }, 'ArrowDown', 3, 4)).toEqual({ row: 2, column: 1 })
  })

  it('stays inside table boundaries', () => {
    expect(moveDimensionCell({ row: 0, column: 0 }, 'ArrowLeft', 2, 3)).toEqual({ row: 0, column: 0 })
    expect(moveDimensionCell({ row: 0, column: 0 }, 'ArrowUp', 2, 3)).toEqual({ row: 0, column: 0 })
    expect(moveDimensionCell({ row: 1, column: 2 }, 'ArrowRight', 2, 3)).toEqual({ row: 1, column: 2 })
    expect(moveDimensionCell({ row: 1, column: 2 }, 'ArrowDown', 2, 3)).toEqual({ row: 1, column: 2 })
  })
})
